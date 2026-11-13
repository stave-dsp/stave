//! Fingerprints (DSP.md R18): a model's T0 characterisation, committed
//! as data, so any change in its sound — even one inside every budget —
//! fails CI until it is re-blessed on purpose.
//!
//! A fingerprint is the model's own tables (the R17 table format, one
//! per rate: oversampled and at 48 kHz) plus the R19 perceptual metrics
//! computed from them, and the points the model catalogue plots:
//!
//! ```text
//! # stave fingerprint v1: <case>
//! model <what ran>                    Circuit<Dk> on spice/<name>.cir, a hand topology, a stage
//! catalogue <stave_dsp path>          the catalogued model it pins (optional)
//! tier t0
//! knee <volts> <criterion>            a black box's knee (circuits keep theirs in the plan)
//! settle <seconds>
//! reference <what the metrics compare against, or "none">
//! playback <dB SPL> <full scale>      the level the ear model heard it at
//! versions <metric/version …>         the R19 implementations
//! metric <rate> <run> <name> <value>  NMR, ERB error, Rnonlin, GedLee per run
//! plot <kind> <series> <x> <y…>       response, harmonics, compression, aliasing, anmr
//! table <rate> <oversampled|shipping|both>
//! <the R17 table at that rate>
//! end
//! ```
//!
//! **Diffing.** Each T0 run re-measures the model and compares its
//! tables with the committed ones characteristic by characteristic
//! ([`super::compare::compare`], the committed table as the
//! reference), each against a numerical tolerance ([`TOLERANCES`]):
//! platform `libm` noise passes, any change in sound fails, and the
//! failure lists every characteristic that moved, by how much and where.
//! The metrics and plots are derived from the tables, so the diff needs
//! only the tables; a separate test recomputes them from the committed
//! tables (a metric's new version shows as a stale fingerprint, never as
//! a moved model). `STAVE_BLESS=1` writes the fingerprint instead.

use std::fmt::Write as _;
use std::path::PathBuf;

use stave_measure::perceptual::erb::{ErbBands, ErbError};
use stave_measure::perceptual::gedlee::GedLee;
use stave_measure::perceptual::nmr::Nmr;
use stave_measure::perceptual::rnonlin::rnonlin;
use stave_measure::perceptual::{tiled, versions, Playback, PLAYBACK_DB_SPL};
use stave_measure::suite::analysis::{gain, phasor, waveform, Harmonics, Spectrum};
use stave_measure::suite::levels::{Criterion, Knee};
use stave_measure::suite::{db, Complex};

use super::compare::{compare, Check};
use super::plan::{Kind, Plan, Run, Tier, HARMONICS};
use super::table::{fixtures, Table};
use super::Rates;

/// The first line of every fingerprint.
pub const HEADER: &str = "# stave fingerprint v1: ";

/// How far a re-measured table may sit from the committed one before the
/// fingerprint has moved: `(check, first, second)`, in the check's units
/// (dB, degrees, volts, seconds). Wide enough for one platform's `libm`
/// against another's, far below anything audible or any budget.
pub const TOLERANCES: [(Check, f64, f64); 12] = [
    (Check::OperatingPoint, 2e-5, f64::NAN),
    (Check::Response, 1e-3, 0.01),
    (Check::GroupDelay, 1e-7, f64::NAN),
    (Check::Bla, 0.01, 0.1),
    (Check::Detection, 0.1, f64::NAN),
    (Check::Compression, 1e-3, 0.01),
    (Check::Harmonics, 0.05, 0.5),
    (Check::ThdN, 0.01, f64::NAN),
    (Check::Intermodulation, 0.05, 0.5),
    (Check::Dynamics, 2e-5, 0.01),
    // Aliasing and ANMR as their own level's change, at floors below
    // which nothing is heard: dBc above −110, dB NMR above −60.
    (Check::Aliasing, 0.5, f64::NAN),
    (Check::Anmr, 0.5, f64::NAN),
];

/// Below these, a level is the floor: two floors agree.
const ALIAS_FLOOR_DB: f64 = -110.0;
const ANMR_FLOOR_DB: f64 = -60.0;

/// How a fingerprint's metrics and plots compare when recomputed:
/// relative, for every metric but the NMR's.
pub const METRIC_TOLERANCE: f64 = 1e-6;
/// The NMR's, in dB: its noise is a difference of two nearly equal
/// magnitudes, so a model close to its reference carries one platform's
/// rounding against another's (0.03 dB seen between macOS and Linux;
/// 0.1 dB a hundred dB down), and NMRs below [`NMR_FLOOR_DB`] compare
/// as that floor.
pub const NMR_TOLERANCE_DB: f64 = 0.1;
/// Far below audibility, an NMR is rounding: these compare equal.
pub const NMR_FLOOR_DB: f64 = -60.0;

/// A table's role.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Oversampled,
    Shipping,
    /// One rate that is both (a memoryless circuit, a black box).
    Both,
}

impl Role {
    fn word(self) -> &'static str {
        match self {
            Role::Oversampled => "oversampled",
            Role::Shipping => "shipping",
            Role::Both => "both",
        }
    }

    pub fn shipping(self) -> bool {
        self != Role::Oversampled
    }

    pub fn class(self) -> Rates {
        if self == Role::Oversampled {
            Rates::Oversampled
        } else {
            Rates::Shipping
        }
    }
}

/// One metric of one run at one rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Metric {
    pub rate: f64,
    pub run: String,
    pub name: String,
    pub value: f64,
}

/// One plotted point: `kind` (response, harmonics, compression,
/// aliasing, anmr), `series` (a rate in Hz, or `reference`), its values.
#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    pub kind: String,
    pub series: String,
    pub values: Vec<f64>,
}

/// A fingerprint.
#[derive(Clone, Debug, PartialEq)]
pub struct Fingerprint {
    pub case: String,
    pub model: String,
    pub catalogue: Option<String>,
    pub tier: Tier,
    pub knee: Option<Knee>,
    pub settle: Option<f64>,
    pub reference: String,
    pub playback: Playback,
    pub versions: String,
    pub tables: Vec<(f64, Role, Table)>,
    pub metrics: Vec<Metric>,
    pub plots: Vec<Plot>,
}

/// `crates/stave-test/fingerprints/<case>.fingerprint`.
pub fn path(case: &str) -> PathBuf {
    fixtures().join(format!("fingerprints/{case}.fingerprint"))
}

/// Re-bless on purpose: `STAVE_BLESS=1`.
pub fn blessing() -> bool {
    std::env::var_os("STAVE_BLESS").is_some()
}

fn criterion_word(c: Criterion) -> &'static str {
    match c {
        Criterion::Compression => "compression",
        Criterion::Distortion => "distortion",
        Criterion::Unreached => "unreached",
    }
}

impl Fingerprint {
    /// A fingerprint of `tables` (the model's, per rate), described by
    /// `reference` (what its metrics compare against, if anything). The
    /// metrics and plots are computed by [`derive`](Self::derive).
    pub fn new(
        case: &str,
        model: &str,
        catalogue: Option<&str>,
        tables: Vec<(f64, Role, Table)>,
        reference: Option<&str>,
        knee: Option<Knee>,
        settle: Option<f64>,
    ) -> Self {
        Fingerprint {
            case: case.into(),
            model: model.into(),
            catalogue: catalogue.map(str::to_string),
            tier: Tier::T0,
            knee,
            settle,
            reference: reference.unwrap_or("none").to_string(),
            playback: Playback::default(),
            versions: versions(),
            tables,
            metrics: Vec::new(),
            plots: Vec::new(),
        }
    }

    /// With its metrics and plots computed over `plan`'s T0 runs against
    /// `reference`.
    pub fn derived(mut self, plan: &Plan, reference: Option<&Table>) -> Self {
        self.derive(plan, reference);
        self
    }

    /// The table at the first shipping rate.
    pub fn shipping(&self) -> Option<&Table> {
        self.tables.iter().find(|t| t.1.shipping()).map(|t| &t.2)
    }

    /// Compute the playback level, metrics and plots from the tables.
    pub fn derive(&mut self, plan: &Plan, reference: Option<&Table>) {
        self.versions = versions();
        self.metrics.clear();
        self.plots.clear();
        let runs: Vec<&Run> = plan.runs(Tier::T0).collect();
        // One playback scale for the whole fingerprint: the largest peak
        // (about its mean) of the reference's T0 runs at the shipping
        // rate — or the model's, with no reference — plays at 92 dB SPL.
        let scale_table = reference.or(self.shipping());
        let ship_rate = self.tables.iter().find(|t| t.1.shipping()).map_or(48_000.0, |t| t.0);
        let mut full_scale = 0.0f64;
        if let Some(t) = scale_table {
            for run in &runs {
                if let Some(m) = t.runs.get(&run.id) {
                    let x = ac(&waveform(&clip(&m.periods[0], run, ship_rate), run.period(), ship_rate));
                    full_scale = full_scale.max(x.iter().fold(0.0f64, |p, v| p.max(v.abs())));
                }
            }
        }
        self.playback = Playback {
            full_scale: significant(if full_scale > 0.0 { full_scale } else { 1.0 }),
            db_spl: PLAYBACK_DB_SPL,
        };
        for (rate, role, table) in self.tables.clone() {
            for run in &runs {
                let Some(m) = table.runs.get(&run.id) else { continue };
                let spectrum = &m.periods[0];
                let mut put = |name: &str, value: f64| {
                    self.metrics.push(Metric {
                        rate,
                        run: run.id.clone(),
                        name: name.into(),
                        value: significant(value),
                    });
                };
                if let Some(r) = reference.and_then(|r| r.runs.get(&run.id)) {
                    // The reference on the model's bins, so both carry the same lines.
                    let shared = Spectrum {
                        bins: spectrum.bins.iter().filter_map(|(k, _)| Some((*k, r.periods[0].get(*k)?))).collect(),
                    };
                    let mine = Spectrum {
                        bins: spectrum.bins.iter().filter(|(k, _)| shared.get(*k).is_some()).copied().collect(),
                    };
                    let (x, y) = (ac(&waveform(&shared, run.period(), rate)), ac(&waveform(&mine, run.period(), rate)));
                    let len = (0.25 * rate) as usize;
                    let nmr = Nmr::new(self.playback).between(&tiled(&x, len), &tiled(&y, len), rate);
                    put("nmr_total_db", nmr.total_db);
                    put("nmr_max_db", nmr.max_db);
                    put("nmr_disturbed", nmr.disturbed);
                    let bands = ErbBands::new(rate);
                    let lines = |s: &Spectrum| {
                        bands.powers(s.bins.iter().filter(|(k, _)| *k > 0).map(|(k, c)| (run.period().hz(*k), c.norm_sqr() / 2.0)))
                    };
                    let e = ErbError::of(&bands, &lines(&shared), &lines(&mine));
                    put("erb_max_db", e.max_db);
                    put("erb_rms_db", e.rms_db);
                }
                match run.kind {
                    Kind::Tone { bin, .. } => {
                        let h = Harmonics::of(spectrum, bin, HARMONICS);
                        let c: Vec<Complex<f64>> = (1..=HARMONICS).map(|k| h.get(k).unwrap_or(Complex::new(0.0, 0.0))).collect();
                        if c[0].norm() > 0.0 {
                            put("gedlee", GedLee::of_harmonics(&c).gm);
                        }
                    }
                    // Rnonlin on the broadband runs, at a shipping rate
                    // (where it is heard): what went in against what came out.
                    Kind::Multisine { .. } if role.shipping() => {
                        let input = Spectrum {
                            bins: run.excitation.tones.iter().map(|t| (t.bin, phasor(t))).collect(),
                        };
                        let x = waveform(&clip(&input, run, rate), run.period(), rate);
                        let y = ac(&waveform(spectrum, run.period(), rate));
                        let len = (0.2 * rate) as usize;
                        put("rnonlin", rnonlin(&tiled(&x, len), &tiled(&y, len), rate).value);
                    }
                    _ => {}
                }
            }
        }
        self.plots = plots(&self.tables, reference, &runs);
    }

    /// The text form.
    pub fn describe(&self) -> String {
        let mut s = format!("{HEADER}{}\n", self.case);
        s.push_str("# Written by STAVE_BLESS=1 (the characterise tests); CI diffs it (DSP.md R18).\n");
        let _ = writeln!(s, "model {}", self.model);
        if let Some(c) = &self.catalogue {
            let _ = writeln!(s, "catalogue {c}");
        }
        let _ = writeln!(s, "tier {}", if self.tier == Tier::T0 { "t0" } else { "t1" });
        if let Some(k) = self.knee {
            let _ = writeln!(s, "knee {:e} {}", k.amplitude, criterion_word(k.criterion));
        }
        if let Some(t) = self.settle {
            let _ = writeln!(s, "settle {t:e}");
        }
        let _ = writeln!(s, "reference {}", self.reference);
        let _ = writeln!(s, "playback {} {:e}", self.playback.db_spl, self.playback.full_scale);
        let _ = writeln!(s, "versions {}", self.versions);
        for m in &self.metrics {
            let _ = writeln!(s, "metric {} {} {} {:e}", m.rate, m.run, m.name, m.value);
        }
        for p in &self.plots {
            let v: Vec<String> = p.values.iter().map(|x| format!("{x:e}")).collect();
            let _ = writeln!(s, "plot {} {} {}", p.kind, p.series, v.join(" "));
        }
        for (rate, role, t) in &self.tables {
            let _ = writeln!(s, "table {rate} {}", role.word());
            s.push_str(&t.describe());
            s.push_str("end\n");
        }
        s
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.replace("\r\n", "\n");
        let mut lines = text.lines();
        let case = lines
            .next()
            .and_then(|l| l.strip_prefix(HEADER))
            .ok_or("not a stave fingerprint (no header)")?
            .trim()
            .to_string();
        let mut fp = Fingerprint {
            case,
            model: String::new(),
            catalogue: None,
            tier: Tier::T0,
            knee: None,
            settle: None,
            reference: "none".into(),
            playback: Playback::default(),
            versions: String::new(),
            tables: Vec::new(),
            metrics: Vec::new(),
            plots: Vec::new(),
        };
        let num = |w: &str| w.parse::<f64>().map_err(|e| format!("fingerprint: {w}: {e}"));
        while let Some(line) = lines.next() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
            let w: Vec<&str> = rest.split_whitespace().collect();
            match head {
                "model" => fp.model = rest.to_string(),
                "catalogue" => fp.catalogue = Some(rest.to_string()),
                "tier" => fp.tier = if rest == "t1" { Tier::T1 } else { Tier::T0 },
                "knee" => {
                    fp.knee = Some(Knee {
                        amplitude: num(w[0])?,
                        criterion: match w.get(1) {
                            Some(&"compression") => Criterion::Compression,
                            Some(&"distortion") => Criterion::Distortion,
                            _ => Criterion::Unreached,
                        },
                    })
                }
                "settle" => fp.settle = Some(num(rest)?),
                "reference" => fp.reference = rest.to_string(),
                "playback" => {
                    fp.playback = Playback {
                        db_spl: num(w[0])?,
                        full_scale: num(w[1])?,
                    }
                }
                "versions" => fp.versions = rest.to_string(),
                "metric" => fp.metrics.push(Metric {
                    rate: num(w[0])?,
                    run: w[1].to_string(),
                    name: w[2].to_string(),
                    value: num(w[3])?,
                }),
                "plot" => fp.plots.push(Plot {
                    kind: w[0].to_string(),
                    series: w[1].to_string(),
                    values: w[2..].iter().map(|x| num(x)).collect::<Result<_, _>>()?,
                }),
                "table" => {
                    let rate = num(w[0])?;
                    let role = match w.get(1) {
                        Some(&"oversampled") => Role::Oversampled,
                        Some(&"shipping") => Role::Shipping,
                        _ => Role::Both,
                    };
                    let mut body = String::new();
                    for l in lines.by_ref() {
                        if l == "end" {
                            break;
                        }
                        body.push_str(l);
                        body.push('\n');
                    }
                    fp.tables.push((rate, role, Table::parse(&body)?));
                }
                other => return Err(format!("fingerprint: unknown line `{other}`")),
            }
        }
        Ok(fp)
    }

    /// The committed fingerprint of `case`, if any.
    pub fn committed(case: &str) -> Option<Result<Self, String>> {
        let text = std::fs::read_to_string(path(case)).ok()?;
        Some(Self::parse(&text))
    }

    pub fn write(&self) {
        let p = path(&self.case);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, self.describe()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }

    /// Hold this (freshly measured) fingerprint to the committed one:
    /// with `STAVE_BLESS=1`, write it; otherwise panic listing every
    /// characteristic that moved beyond its tolerance.
    #[track_caller]
    pub fn pin(self, plan: &Plan, reference: Option<&Table>) {
        if blessing() {
            self.derived(plan, reference).write();
            return;
        }
        let committed = match Self::committed(&self.case) {
            Some(Ok(c)) => c,
            Some(Err(e)) => panic!("fingerprint `{}`: {e}", self.case),
            None => panic!(
                "fingerprint `{}`: none committed at {} (write it with STAVE_BLESS=1)",
                self.case,
                path(&self.case).display()
            ),
        };
        let moved = diff(&committed, &self, plan);
        assert!(
            moved.is_empty(),
            "fingerprint `{}` moved — the model sounds different from the one pinned. \
             If that is intended, re-bless on purpose with STAVE_BLESS=1 and review the diff:\n  {}",
            self.case,
            moved.join("\n  ")
        );
    }

    /// The committed fingerprint's metrics and plots, recomputed from its
    /// own tables with the current metric implementations: the stale
    /// lines, if any.
    pub fn stale(&self, plan: &Plan, reference: Option<&Table>) -> Vec<String> {
        let mut fresh = self.clone();
        fresh.derive(plan, reference);
        let mut out = Vec::new();
        if fresh.versions != self.versions {
            out.push(format!("metric versions: committed `{}`, now `{}`", self.versions, fresh.versions));
        }
        if (fresh.playback.full_scale / self.playback.full_scale - 1.0).abs() > METRIC_TOLERANCE {
            out.push(format!("playback full scale: {} vs {}", self.playback.full_scale, fresh.playback.full_scale));
        }
        if fresh.metrics.len() != self.metrics.len() {
            out.push(format!("{} metrics committed, {} now", self.metrics.len(), fresh.metrics.len()));
        }
        for (a, b) in self.metrics.iter().zip(&fresh.metrics) {
            let close = if a.name.starts_with("nmr_") && a.name.ends_with("_db") {
                (a.value.max(NMR_FLOOR_DB) - b.value.max(NMR_FLOOR_DB)).abs() <= NMR_TOLERANCE_DB
            } else {
                (a.value - b.value).abs() <= METRIC_TOLERANCE * (1.0 + a.value.abs()) || (a.value.is_infinite() && a.value == b.value)
            };
            if a.name != b.name || a.run != b.run || !close {
                out.push(format!("metric {} {} {}: {} vs {}", a.rate, a.run, a.name, a.value, b.value));
            }
        }
        if fresh.plots.len() != self.plots.len() {
            out.push(format!("{} plot points committed, {} now", self.plots.len(), fresh.plots.len()));
        } else if self.plots.iter().zip(&fresh.plots).any(|(a, b)| {
            a.kind != b.kind
                || a.series != b.series
                || a.values.iter().zip(&b.values).any(|(x, y)| (x - y).abs() > METRIC_TOLERANCE * (1.0 + x.abs()))
        }) {
            out.push("the plot points differ".into());
        }
        out
    }
}

/// The settling a black box gets before each run: 50 ms (DC blockers
/// and envelope followers past their slowest constant).
pub const STAGE_SETTLE_SECONDS: f64 = 0.05;

/// A black box's plan: the circuits' T0 plan at `knee` (its input name
/// is a placeholder; a ticker takes the stimulus directly).
pub fn stage_plan(case: &str, knee: Knee, settle: f64) -> Plan {
    Plan::circuit(case, super::plan::Input::new("in", 0.0), knee, settle)
}

/// A black box's T0 fingerprint at `rate` (DSP.md R18: a catalogued
/// stage, a plugin): `make(rate)` returns a fresh ticker (the stimulus
/// in, the output out, sample by sample). Its knee is found at 1 kHz
/// ([`stage_knee`](super::model::stage_knee)); the plan runs at `knee`
/// when given (a committed fingerprint's, so a re-measurement compares
/// like with like), else at the knee found. No reference: the metrics
/// are its own (Rnonlin, GedLee, ANMR).
pub fn of_stage(
    case: &str,
    model: &str,
    catalogue: Option<&str>,
    make: impl Fn(f64) -> super::model::Ticker + 'static,
    rate: f64,
    knee: Option<Knee>,
) -> (Fingerprint, Plan) {
    of_black_box(case, model, catalogue, move |r| (make(r), 0), rate, knee)
}

/// [`of_stage`] for a black box that answers late: `make(rate)` returns
/// a fresh ticker and its delay in samples (a hosted plugin through
/// [`black_box::maker`](super::black_box::maker)).
pub fn of_black_box(
    case: &str,
    model: &str,
    catalogue: Option<&str>,
    make: impl Fn(f64) -> (super::model::Ticker, usize) + 'static,
    rate: f64,
    knee: Option<Knee>,
) -> (Fingerprint, Plan) {
    let settle = STAGE_SETTLE_SECONDS;
    let found = super::model::delayed_knee(&make, rate, settle);
    let plan = stage_plan(case, knee.unwrap_or(found), settle);
    let m = super::model::Model::Delayed { make: Box::new(make) };
    let (table, _) = m.measure(&plan, &stave_dsp::circuit::netlist::Netlist::new(), Tier::T0, rate, false, &[]);
    let fp = Fingerprint::new(case, model, catalogue, vec![(rate, Role::Both, table)], None, Some(found), Some(settle));
    (fp, plan)
}

/// [`of_stage`] held to (or, blessing, written as) the committed
/// fingerprint of `case`, at 48 kHz.
#[track_caller]
pub fn pin_stage(case: &str, model: &str, catalogue: Option<&str>, make: impl Fn(f64) -> super::model::Ticker + 'static) {
    let committed = if blessing() {
        None
    } else {
        Fingerprint::committed(case).map(|c| c.unwrap_or_else(|e| panic!("fingerprint `{case}`: {e}")))
    };
    let (fp, plan) = of_stage(case, model, catalogue, make, 48_000.0, committed.as_ref().and_then(|c| c.knee));
    fp.pin(&plan, None);
}

/// Every characteristic of `now` that moved from `committed` beyond
/// [`TOLERANCES`], as lines for a failure message.
pub fn diff(committed: &Fingerprint, now: &Fingerprint, plan: &Plan) -> Vec<String> {
    let mut moved = Vec::new();
    if committed.knee.map(|k| k.amplitude) != now.knee.map(|k| k.amplitude) {
        let (a, b) = (committed.knee.map_or(0.0, |k| k.amplitude), now.knee.map_or(0.0, |k| k.amplitude));
        moved.push(format!("knee: {a:e} → {b:e} ({:+.2} dB)", db(b / a)));
    }
    for (rate, role, table) in &now.tables {
        let Some((_, _, old)) = committed.tables.iter().find(|t| t.0 == *rate) else {
            moved.push(format!("{:.1} kHz: no committed table at this rate", rate / 1e3));
            continue;
        };
        let at = format!("{:.1} kHz {}", rate / 1e3, role.word());
        if old.runs.keys().ne(table.runs.keys()) || old.op.len() != table.op.len() || old.ac.len() != table.ac.len() {
            moved.push(format!("{at}: the runs or points differ (the plan changed?)"));
            continue;
        }
        for (check, first, second) in TOLERANCES {
            match check {
                Check::Aliasing | Check::Anmr => {
                    for run in plan.runs(Tier::T0) {
                        let (Some(a), Some(b)) = (old.runs.get(&run.id), table.runs.get(&run.id)) else { continue };
                        let (x, y, floor) = if check == Check::Aliasing {
                            (a.floor.map(db), b.floor.map(db), ALIAS_FLOOR_DB)
                        } else {
                            (a.anmr, b.anmr, ANMR_FLOOR_DB)
                        };
                        let (Some(x), Some(y)) = (x, y) else {
                            if x.is_some() != y.is_some() {
                                moved.push(format!("{} {at}: {} has it, the other not", check.name(), run.id));
                            }
                            continue;
                        };
                        let e = (x.max(floor) - y.max(floor)).abs();
                        if e > first || e.is_nan() {
                            moved.push(format!(
                                "{} {at}: {:+.2} {} (tolerance {first}) at {}: {x:.2} → {y:.2}",
                                check.name(),
                                y.max(floor) - x.max(floor),
                                check.unit(),
                                run.id
                            ));
                        }
                    }
                }
                _ => {
                    let Some(m) = compare(check, plan, Tier::T0, old, table) else { continue };
                    let over = |e: f64, t: f64| e > t || e.is_nan();
                    if over(m.first.error, first) {
                        moved.push(format!(
                            "{} {at}: {:.3e} {} (tolerance {first:e}) at {}",
                            check.name(),
                            m.first.error,
                            check.unit(),
                            m.first.at
                        ));
                    }
                    if let Some(unit) = check.second_unit() {
                        if !second.is_nan() && over(m.second.error, second) {
                            moved.push(format!(
                                "{} {at}: {:.3e} {unit} (tolerance {second:e}) at {}",
                                check.name(),
                                m.second.error,
                                m.second.at
                            ));
                        }
                    }
                }
            }
        }
    }
    moved
}

/// The spectrum's bins below `rate`'s Nyquist (a reference simulated
/// at a higher rate holds bins a shipping rate cannot carry).
fn clip(s: &Spectrum, run: &Run, rate: f64) -> Spectrum {
    let n = run.period().samples(rate);
    Spectrum {
        bins: s.bins.iter().filter(|(k, _)| 2 * (*k as usize) < n).copied().collect(),
    }
}

/// `x` about its mean.
fn ac(x: &[f64]) -> Vec<f64> {
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    x.iter().map(|v| v - mean).collect()
}

/// Nine significant digits: the text form reads back exactly.
fn significant(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.8e}").parse().unwrap()
}

/// The plotted points: the small-signal response, each T0 tone's
/// harmonics, its gain (compression) and its aliasing, per rate and for
/// the reference.
fn plots(tables: &[(f64, Role, Table)], reference: Option<&Table>, runs: &[&Run]) -> Vec<Plot> {
    let mut out = Vec::new();
    let mut series: Vec<(String, &Table)> = tables.iter().map(|(r, _, t)| (format!("{r}"), t)).collect();
    if let Some(r) = reference {
        series.push(("reference".into(), r));
    }
    let mut put = |kind: &str, s: &str, values: Vec<f64>| {
        out.push(Plot {
            kind: kind.into(),
            series: s.into(),
            values: values.into_iter().map(significant).collect(),
        })
    };
    for (name, t) in &series {
        for (hz, c) in &t.ac {
            put("response", name, vec![*hz, db(c.norm())]);
        }
        for run in runs {
            let Kind::Tone { bin, offset_db } = run.kind else { continue };
            let Some(m) = t.runs.get(&run.id) else { continue };
            let h = Harmonics::of(&m.periods[0], bin, HARMONICS);
            let Some(f) = h.get(1) else { continue };
            put("compression", name, vec![offset_db, db(gain(&run.excitation.tones[0], f).norm())]);
            for k in 2..=HARMONICS {
                if let Some(d) = h.dbc(k) {
                    put("harmonics", name, vec![offset_db, f64::from(k), d.max(-160.0)]);
                }
            }
            if name != "reference" {
                if let Some(fl) = m.floor {
                    put("aliasing", name, vec![offset_db, db(fl).max(-160.0)]);
                }
                if let Some(a) = m.anmr {
                    put("anmr", name, vec![offset_db, a]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characterise::plan::Input;
    use crate::characterise::table::Measured;

    fn plan() -> Plan {
        let knee = Knee {
            amplitude: 0.5,
            criterion: Criterion::Compression,
        };
        Plan::circuit("x", Input::new("in", 0.0), knee, 0.01)
    }

    /// A table of a memoryless curve through the plan's T0 runs.
    fn table(plan: &Plan, curve: impl Fn(f64) -> f64) -> Table {
        let mut t = Table {
            circuit: "x".into(),
            source: "a test".into(),
            ..Table::default()
        };
        for run in plan.runs(Tier::T0) {
            let rate = 48_000.0;
            let n = run.period().samples(rate);
            let periods: Vec<Vec<f64>> =
                (0..2).map(|_| (0..n).map(|i| curve(run.excitation.tones.iter().map(|tn| tn.amplitude * (std::f64::consts::TAU * f64::from(tn.bin) * i as f64 / n as f64 + tn.phase).sin()).sum())).collect()).collect();
            let m: Measured = crate::characterise::model::measure_run(run, rate, &periods);
            t.runs.insert(run.id.clone(), m);
        }
        t
    }

    fn print(plan: &Plan, curve: impl Fn(f64) -> f64, reference: Option<&Table>) -> Fingerprint {
        let t = table(plan, curve);
        Fingerprint::new("x", "a test curve", None, vec![(48_000.0, Role::Both, t)], reference.map(|_| "the reference"), None, None)
            .derived(plan, reference)
    }

    #[test]
    fn fingerprints_round_trip_through_text() {
        let plan = plan();
        let fp = print(&plan, |x| (2.0 * x).tanh(), None);
        let back = Fingerprint::parse(&fp.describe()).unwrap();
        assert_eq!(back.describe(), fp.describe());
        assert!(back.stale(&plan, None).is_empty(), "{:?}", back.stale(&plan, None));
    }

    /// The same model: nothing moved. A slightly harder drive: the
    /// harmonics, compression and THD+N moved, each named with its size.
    #[test]
    fn a_change_in_sound_is_named() {
        let plan = plan();
        let a = print(&plan, |x| (2.0 * x).tanh(), None);
        assert!(diff(&a, &a, &plan).is_empty());
        let b = print(&plan, |x| (2.02 * x).tanh() / 1.01, None);
        let moved = diff(&a, &b, &plan);
        let text = moved.join("\n");
        assert!(text.contains("C5 harmonics") && text.contains("C4 compression") && text.contains("dB"), "{text}");
    }

    /// Against a reference, the metrics are there: the curve against
    /// itself is at the NMR floor with no ERB error; a different one is not.
    #[test]
    fn metrics_compare_against_the_reference() {
        let plan = plan();
        let reference = table(&plan, |x| (2.0 * x).tanh());
        let same = print(&plan, |x| (2.0 * x).tanh(), Some(&reference));
        let other = print(&plan, |x| (3.0 * x).tanh() / 1.4, Some(&reference));
        let get = |fp: &Fingerprint, run: &str, name: &str| fp.metrics.iter().find(|m| m.run == run && m.name == name).unwrap().value;
        assert!(get(&same, "tone/1k/+12", "nmr_total_db") < -90.0);
        assert!(get(&same, "tone/1k/+12", "erb_max_db") < 1e-9);
        assert!(get(&other, "tone/1k/+12", "nmr_total_db") > get(&same, "tone/1k/+12", "nmr_total_db") + 30.0);
        assert!(get(&other, "tone/1k/+12", "gedlee") > get(&same, "tone/1k/+12", "gedlee"));
        assert!(get(&other, "multisine/+12/r1", "rnonlin") < get(&same, "multisine/+12/r1", "rnonlin"));
    }
}
