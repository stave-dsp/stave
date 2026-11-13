//! The reference side of a characterisation, in one table format for
//! every kind of reference: ngspice's simulation of the plan, a pinned
//! fingerprint (the result chosen by ear), a measured target (a
//! digitised datasheet curve, DSP.md R23) or a hardware capture. A
//! [`Target`] says where the table lives and how it is regenerated.
//!
//! ```text
//! # stave characterisation table v1: <circuit>
//! op <node> <volts>                       C1: the operating point
//! ac <hz> <re> <im>                       C2: the small-signal response
//! run <id> steady <x> thdn <x> floor <x> [anmr <x>]
//!                                         per run: steady state, THD+N, inharmonic floor,
//!                                         and a model's ANMR at a shipping rate
//! <k> <re> <im>                           then each bin's complex amplitude (8 digits)
//! period <j>                              a burst's next measured period, its bins after
//! ```
//!
//! Only measurements, never waveforms: a T1 table is a few hundred
//! kilobytes at most.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use stave_measure::suite::analysis::Spectrum;
use stave_measure::suite::Complex;

/// One run's measurements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    /// [`steady_state`](stave_measure::suite::analysis::steady_state) of
    /// the first measured period against the one before.
    pub steady: f64,
    /// A tone run's THD+N (audible band), as a ratio.
    pub thdn: Option<f64>,
    /// A tone run's inharmonic floor (audible band, off every harmonic),
    /// as a ratio: aliasing for a model, numerical noise for a simulator.
    pub floor: Option<f64>,
    /// A tone run's ANMR at a shipping rate (≤ 96 kHz), dB: its
    /// inharmonic remainder under the harmonics' mask, A-weighted
    /// ([`anmr`](stave_measure::suite::analysis::anmr)). A model's own
    /// figure; references carry none.
    pub anmr: Option<f64>,
    /// Each measured period's spectrum.
    pub periods: Vec<Spectrum>,
}

/// A characterisation's measurements: the reference's, or a model's
/// (its fingerprint).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub circuit: String,
    /// Where it came from, for messages.
    pub source: String,
    pub op: Vec<(String, f64)>,
    pub ac: Vec<(f64, Complex<f64>)>,
    pub runs: BTreeMap<String, Measured>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut t = Table::default();
        let num = |w: Option<&&str>| -> Result<f64, String> {
            match w {
                Some(&"-") => Ok(f64::NAN),
                Some(w) => w.parse::<f64>().map_err(|e| format!("table: {w}: {e}")),
                None => Err("table: truncated line".into()),
            }
        };
        let opt = |x: f64| (!x.is_nan()).then_some(x);
        let mut current: Option<String> = None;
        let mut period = 0usize;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# stave characterisation table v1: ") {
                t.circuit = rest.trim().to_string();
                continue;
            }
            if let Some(rest) = line.strip_prefix("# source: ") {
                t.source = rest.trim().to_string();
                continue;
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let w: Vec<&str> = line.split_whitespace().collect();
            match w[0] {
                "op" => t.op.push((w[1].to_string(), num(w.get(2))?)),
                "ac" => t.ac.push((num(w.get(1))?, Complex::new(num(w.get(2))?, num(w.get(3))?))),
                "run" => {
                    let m = t.runs.entry(w[1].to_string()).or_default();
                    m.steady = num(w.get(3))?;
                    m.thdn = opt(num(w.get(5))?);
                    m.floor = opt(num(w.get(7))?);
                    m.anmr = match w.get(8) {
                        Some(&"anmr") => opt(num(w.get(9))?),
                        _ => None,
                    };
                    current = Some(w[1].to_string());
                    period = 0;
                }
                "period" => period = num(w.get(1))? as usize,
                k if k.starts_with(|c: char| c.is_ascii_digit()) => {
                    let id = current.as_ref().ok_or("table: a bin before any run")?;
                    let m = t.runs.get_mut(id).expect("the current run");
                    while m.periods.len() <= period {
                        m.periods.push(Spectrum::default());
                    }
                    let k = num(w.first())? as u32;
                    m.periods[period]
                        .bins
                        .push((k, Complex::new(num(w.get(1))?, num(w.get(2))?)));
                }
                other => return Err(format!("table: unknown line `{other}`")),
            }
        }
        Ok(t)
    }

    /// The text form, numbers to 10 significant digits.
    pub fn describe(&self) -> String {
        let mut s = format!(
            "# stave characterisation table v1: {}\n# source: {}\n",
            self.circuit, self.source
        );
        let f = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x:.9e}"));
        for (node, v) in &self.op {
            let _ = writeln!(s, "op {node} {v:.9e}");
        }
        for (hz, c) in &self.ac {
            let _ = writeln!(s, "ac {hz:.9e} {:.9e} {:.9e}", c.re, c.im);
        }
        for (id, m) in &self.runs {
            let _ = write!(
                s,
                "run {id} steady {} thdn {} floor {}",
                f(Some(m.steady)),
                f(m.thdn),
                f(m.floor)
            );
            if let Some(a) = m.anmr {
                let _ = write!(s, " anmr {a:.6e}");
            }
            s.push('\n');
            for (j, p) in m.periods.iter().enumerate() {
                if j > 0 {
                    let _ = writeln!(s, "period {j}");
                }
                for (k, c) in &p.bins {
                    let _ = writeln!(s, "{k} {:.7e} {:.7e}", c.re, c.im);
                }
            }
        }
        s
    }

    /// How far `other` is from this table: the largest deviation of any
    /// entry relative to its group's scale (the operating point's largest
    /// node, the response's largest point, a run's largest bin), and
    /// where; infinite when their entries differ. THD+N, the floor and
    /// steady state are derived from the bins and not compared again.
    pub fn deviation(&self, other: &Table) -> (f64, String) {
        let mut worst = (0.0, String::new());
        let mut offer = |d: f64, at: &dyn Fn() -> String| {
            if d > worst.0 || d.is_nan() {
                worst = (d, at());
            }
        };
        if self.op.len() != other.op.len() || self.ac.len() != other.ac.len() {
            return (f64::INFINITY, "the operating point or response differ in points".into());
        }
        let op_scale = self.op.iter().fold(0.0, |m: f64, (_, v)| m.max(v.abs())).max(1e-300);
        for ((n, a), (_, b)) in self.op.iter().zip(&other.op) {
            offer((a - b).abs() / op_scale, &|| format!("op {n}: {a:e} vs {b:e}"));
        }
        let ac_scale = self.ac.iter().fold(0.0, |m: f64, (_, c)| m.max(c.norm())).max(1e-300);
        for ((hz, a), (_, b)) in self.ac.iter().zip(&other.ac) {
            offer((*a - *b).norm() / ac_scale, &|| format!("ac {hz} Hz"));
        }
        for (id, a) in &self.runs {
            let Some(b) = other.runs.get(id) else {
                return (f64::INFINITY, format!("run {id} is missing"));
            };
            let scale = a
                .periods
                .iter()
                .flat_map(|p| p.bins.iter().map(|(_, c)| c.norm()))
                .fold(0.0, f64::max)
                .max(1e-300);
            for (j, (pa, pb)) in a.periods.iter().zip(&b.periods).enumerate() {
                if pa.bins.len() != pb.bins.len() {
                    return (f64::INFINITY, format!("run {id} period {j}: bins differ"));
                }
                for ((k, ca), (_, cb)) in pa.bins.iter().zip(&pb.bins) {
                    offer((*ca - *cb).norm() / scale, &|| format!("run {id} period {j} bin {k}"));
                }
            }
        }
        if other.runs.len() != self.runs.len() {
            return (f64::INFINITY, "the runs differ".into());
        }
        worst
    }
}

/// Where a characterisation's reference comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `spice/<name>.table`: ngspice on `spice/<name>.cir`, run through
    /// `spice/<name>.plan` by `scripts/ngspice-simulate.py`.
    Ngspice { name: String },
    /// `fingerprints/<name>.fingerprint`'s shipping-rate table: a model's
    /// own results, pinned on purpose (`STAVE_BLESS=1`), so a change in
    /// sound fails review (DSP.md R18).
    Fingerprint { name: String },
    /// A measured target in the table format — a digitised datasheet
    /// curve or a published response — committed with its source cited.
    Measured { path: PathBuf, source: String },
    /// A hardware capture's table, outside git: `$STAVE_CAPTURES/<path>`.
    /// Absent, the case is skipped with a message.
    Capture { path: PathBuf },
}

/// `crates/stave-test/`.
pub(crate) fn fixtures() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

impl Target {
    pub fn ngspice(name: &str) -> Self {
        Self::Ngspice { name: name.into() }
    }

    pub fn fingerprint(name: &str) -> Self {
        Self::Fingerprint { name: name.into() }
    }

    pub fn path(&self) -> Option<PathBuf> {
        match self {
            Target::Ngspice { name } => Some(fixtures().join(format!("spice/{name}.table"))),
            Target::Fingerprint { name } => Some(super::fingerprint::path(name)),
            Target::Measured { path, .. } => Some(fixtures().join(path)),
            Target::Capture { path } => {
                std::env::var_os("STAVE_CAPTURES").map(|root| PathBuf::from(root).join(path))
            }
        }
    }

    /// How the table is regenerated, for messages.
    pub fn regenerate(&self) -> String {
        match self {
            Target::Ngspice { .. } => "scripts/ngspice-simulate.py".into(),
            Target::Fingerprint { .. } => {
                "STAVE_BLESS=1 cargo test -p stave-test --test characterise (on purpose)".into()
            }
            Target::Measured { source, .. } => format!("digitised from {source}"),
            Target::Capture { .. } => "the capture rig (DSP.md R20)".into(),
        }
    }

    /// The table, or why it is absent: a capture outside git is skipped,
    /// every other target must exist.
    pub fn load(&self) -> Result<Table, String> {
        let Some(path) = self.path() else {
            return Err("skipped: STAVE_CAPTURES is not set (captures live outside git)".into());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) if matches!(self, Target::Fingerprint { .. }) => super::fingerprint::Fingerprint::parse(&text)
                .and_then(|f| f.shipping().cloned().ok_or_else(|| "no shipping table".to_string()))
                .map_err(|e| format!("{}: {e}", path.display())),
            Ok(text) => Table::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if matches!(self, Target::Capture { .. }) => {
                Err(format!("skipped: {}: {e}", path.display()))
            }
            Err(e) => panic!(
                "{}: {e} (regenerate with {})",
                path.display(),
                self.regenerate()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_round_trip() {
        let mut t = Table {
            circuit: "x".into(),
            source: "a test".into(),
            op: vec![("out".into(), 4.5)],
            ac: vec![(20.0, Complex::new(1.0, -0.5))],
            ..Table::default()
        };
        t.runs.insert(
            "tone/1k/+0".into(),
            Measured {
                steady: 1e-9,
                thdn: Some(0.01),
                floor: None,
                anmr: Some(-42.5),
                periods: vec![Spectrum {
                    bins: vec![(0, Complex::new(0.1, 0.0)), (21, Complex::new(0.25, -0.75))],
                }],
            },
        );
        assert_eq!(Table::parse(&t.describe()).unwrap(), t);
    }

    #[test]
    fn an_absent_capture_skips() {
        let t = Target::Capture {
            path: "no/such/capture.table".into(),
        };
        let e = t.load().unwrap_err();
        assert!(e.starts_with("skipped:"), "{e}");
    }
}
