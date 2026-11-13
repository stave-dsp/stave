//! C12 in the characterisation (DSP.md M6): a model's or a black box's
//! nonlinear kernels from synchronized exponential sweeps at levels
//! relative to its knee — the harmonic responses, generalised Hammerstein
//! kernels per level and the low-order Volterra diagonals across them
//! ([`stave_measure::identify::kernels`]). An analysis, never a runtime
//! model: a compact picture of weak nonlinearity to set models, black
//! boxes and captures side by side, kept in a fingerprint when a case
//! asks for it ([`Characterise::kernels`](super::Characterise::kernels),
//! [`Fingerprint::kernels`](super::Fingerprint)) and drawn on the model
//! catalogue.
//!
//! What a fingerprint keeps ([`KernelSet::record`]): each kernel up to
//! [`KernelPlan::reported`] at third-octave points, as dB re 1 V^{1−m}
//! and degrees. Plots and diffs read them as each kernel's contribution
//! at its level, `|Gₘ|·(a/2)^{m−1}` re the linear kernel's peak (for the
//! Volterra diagonals, at the knee): what the harmonic of a sine at that
//! level reads in dBc.
//!
//! # Valid ranges
//!
//! Those of [`stave_measure::identify::kernels`]: weak nonlinearity
//! (levels below the knee: [`KernelPlan::levels_db`] defaults to −30,
//! −20 and −10 dB re it), time invariance, memory within the windows, the
//! band from `2·order·f₁` to `0.9·f₂`, and a model run at its host rate
//! carries its own aliasing into the kernels (as its R17 table does).
//!
//! # Verified
//!
//! `tests/kernels.rs`: harmonic levels of the R17 tones read from the
//! kernels at the same level, against the model's own table at 48 kHz and
//! against ngspice's, on the common cathode, the diode clipper and the
//! Tube Screamer gain stage.

use stave_measure::identify::hammerstein::stimulus;
use stave_measure::identify::kernels::{level_dependence, Kernel, Kernels, Volterra};
use stave_measure::suite::sweep::Sweep;
use stave_measure::suite::db;

use super::model::Model;
use super::plan::Plan;

/// The sweeps a characterisation plays.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelPlan {
    /// The sweeps' peaks, dB re the knee's sine (principle 3).
    pub levels_db: Vec<f64>,
    /// The sweep's start, Hz: kernels are measured from `2·order·low_hz`.
    pub low_hz: f64,
    /// The sweep's length before synchronization rounds it, seconds.
    pub seconds: f64,
    /// The highest harmonic separated (the expansions are solved to it).
    pub order: usize,
    /// The highest order a fingerprint keeps.
    pub reported: usize,
    /// The rate the model runs at.
    pub rate: f64,
}

impl Default for KernelPlan {
    /// −30, −20 and −10 dB re knee, from 2 Hz (kernels from 20 Hz; the
    /// low edge's leakage below −100 dB re the linear response from about
    /// 60 Hz), 4.5 s (synchronized: 4.67 s, `L = 0.5 s`), order 5, the
    /// first three kept, at 48 kHz.
    fn default() -> Self {
        Self {
            levels_db: vec![-30.0, -20.0, -10.0],
            low_hz: 2.0,
            seconds: 4.5,
            order: 5,
            reported: 3,
            rate: 48_000.0,
        }
    }
}

impl KernelPlan {
    /// The sweep at `level_db` re a knee of `knee` (a sine's peak).
    pub fn sweep(&self, knee: f64, level_db: f64) -> Sweep {
        Sweep::synchronized(self.low_hz, 0.47 * self.rate, self.seconds, knee * 10f64.powf(level_db / 20.0))
    }
}

/// What kind of kernel a row holds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum KernelKind {
    /// A generalised Hammerstein kernel from the sweep `level_db` re knee.
    Hammerstein { level_db: f64 },
    /// A Volterra diagonal, from every level.
    Volterra,
}

/// One kernel at one frequency (output frequency, Hz), as a fingerprint
/// keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelRow {
    pub kind: KernelKind,
    pub order: usize,
    pub hz: f64,
    /// `20·log10|value|`, dB re 1 V^{1−m} (or full scale's).
    pub db: f64,
    pub degrees: f64,
}

/// A characterisation's kernels as committed: the sweep and knee they
/// were measured with, and the rows.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelRecord {
    pub rate: f64,
    /// The knee's sine peak the levels are relative to.
    pub knee: f64,
    pub low_hz: f64,
    pub high_hz: f64,
    pub seconds: f64,
    pub order: usize,
    pub rows: Vec<KernelRow>,
}

impl KernelRecord {
    /// The rows of `kind` and `order`, ascending in frequency.
    pub fn kernel(&self, kind: KernelKind, order: usize) -> impl Iterator<Item = &KernelRow> {
        self.rows.iter().filter(move |r| r.kind == kind && r.order == order)
    }

    /// The amplitude a row's kernel was measured at (the knee for a
    /// Volterra diagonal).
    pub fn amplitude(&self, kind: KernelKind) -> f64 {
        match kind {
            KernelKind::Hammerstein { level_db } => self.knee * 10f64.powf(level_db / 20.0),
            KernelKind::Volterra => self.knee,
        }
    }

    /// A row as its contribution at its level: `|K|·(a/2)^{m−1}` re the
    /// peak of the same kind's linear kernel, dB.
    pub fn contribution_db(&self, row: &KernelRow) -> f64 {
        let linear = self.kernel(row.kind, 1).map(|r| r.db).fold(f64::NEG_INFINITY, f64::max);
        row.db + db((0.5 * self.amplitude(row.kind)).powi(row.order as i32 - 1)) - linear
    }
}

/// Third-octave centres (base 2) from 20 Hz to 20 kHz: where a
/// fingerprint samples each kernel.
pub fn third_octaves() -> Vec<f64> {
    (-17..=13).map(|k| 1_000.0 * 2f64.powf(f64::from(k) / 3.0)).collect()
}

/// The kernels a characterisation measured: one [`Kernels`] per level and
/// the Volterra diagonals across them.
#[derive(Clone, Debug)]
pub struct KernelSet {
    pub plan: KernelPlan,
    pub knee: f64,
    /// `(dB re knee, kernels)`, as the plan lists the levels.
    pub levels: Vec<(f64, Kernels)>,
    /// `None` if the sweeps could not part the orders (one level, or two
    /// equal).
    pub volterra: Option<Volterra>,
}

/// `model`'s kernels: each of `kernels`' sweeps through
/// [`Model::respond`] at `kernels.rate`, the levels re `plan`'s knee (a
/// black box's plan carries the knee it was found at). `None` when the
/// plan has no knee or a sweep cannot be separated.
pub fn measure(model: &Model, plan: &Plan, kernels: &KernelPlan) -> Option<KernelSet> {
    let knee = plan.knee?.amplitude;
    let mut levels = Vec::with_capacity(kernels.levels_db.len());
    for &level in &kernels.levels_db {
        let sweep = kernels.sweep(knee, level);
        let input = stimulus(&sweep, kernels.rate, 0.01, 0.2);
        let output = model.respond(plan, kernels.rate, &input);
        // The operating point is no kernel: the output about its mean.
        let mean = output.iter().sum::<f64>() / output.len() as f64;
        let output: Vec<f64> = output.iter().map(|v| v - mean).collect();
        levels.push((level, Kernels::measure(&sweep, kernels.rate, &output, kernels.order)?));
    }
    let all: Vec<Kernels> = levels.iter().map(|(_, k)| k.clone()).collect();
    Some(KernelSet {
        plan: kernels.clone(),
        knee,
        volterra: Volterra::from_levels(&all),
        levels,
    })
}

impl KernelSet {
    /// The kernels at `level_db` re knee, if measured.
    pub fn at(&self, level_db: f64) -> Option<&Kernels> {
        self.levels.iter().find(|(l, _)| *l == level_db).map(|(_, k)| k)
    }

    /// [`level_dependence`] of `Gₘ` across the levels over `band`.
    pub fn level_dependence(&self, m: usize, band: (f64, f64)) -> Option<f64> {
        let all: Vec<Kernels> = self.levels.iter().map(|(_, k)| k.clone()).collect();
        level_dependence(&all, m, band)
    }

    /// What a fingerprint keeps: every kernel up to the plan's
    /// `reported` order at [`third_octaves`] inside its band.
    pub fn record(&self) -> KernelRecord {
        let hz = third_octaves();
        let mut rows = Vec::new();
        let mut put = |kind: KernelKind, kernel: &Kernel| {
            for (f, v) in kernel.sampled(&hz) {
                rows.push(KernelRow {
                    kind,
                    order: kernel.order,
                    hz: significant(f),
                    db: significant(db(v.norm())),
                    degrees: significant(v.arg().to_degrees()),
                });
            }
        };
        for (level, k) in &self.levels {
            for kernel in k.hammerstein.iter().take(self.plan.reported) {
                put(KernelKind::Hammerstein { level_db: *level }, kernel);
            }
        }
        if let Some(v) = &self.volterra {
            for kernel in v.kernels.iter().take(self.plan.reported) {
                put(KernelKind::Volterra, kernel);
            }
        }
        let sweep = self.plan.sweep(self.knee, 0.0);
        KernelRecord {
            rate: self.plan.rate,
            knee: self.knee,
            low_hz: sweep.low_hz,
            high_hz: sweep.high_hz,
            seconds: significant(sweep.seconds),
            order: self.plan.order,
            rows,
        }
    }
}

/// Nine significant digits: the text form reads back exactly.
fn significant(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.8e}").parse().unwrap()
}

/// How far a re-measured record sits from the committed one before the
/// kernels have moved: dB and degrees, on rows whose contribution is above
/// [`KERNEL_FLOOR_DB`]. Wide enough for one platform's `libm` against
/// another's.
pub const KERNEL_TOLERANCE: (f64, f64) = (0.05, 0.5);
/// Below this contribution (dB re the linear kernel), a row is the floor:
/// two floors agree.
pub const KERNEL_FLOOR_DB: f64 = -100.0;

/// Every kernel row of `now` that moved from `committed`, as lines for a
/// failure message.
pub fn diff(committed: &KernelRecord, now: &KernelRecord) -> Vec<String> {
    let mut moved = Vec::new();
    let header = |r: &KernelRecord| (r.rate, r.knee, r.low_hz, r.high_hz, r.seconds, r.order, r.rows.len());
    if header(committed) != header(now) {
        moved.push(format!(
            "C12 kernels: measured differently (rate, knee, sweep, order or rows: {:?} → {:?})",
            header(committed),
            header(now)
        ));
        return moved;
    }
    for (a, b) in committed.rows.iter().zip(&now.rows) {
        if a.kind != b.kind || a.order != b.order || a.hz != b.hz {
            moved.push("C12 kernels: the rows differ (the plan changed?)".into());
            return moved;
        }
        let (ca, cb) = (committed.contribution_db(a), now.contribution_db(b));
        if ca < KERNEL_FLOOR_DB && cb < KERNEL_FLOOR_DB {
            continue;
        }
        let dphase = (a.degrees - b.degrees + 540.0).rem_euclid(360.0) - 180.0;
        let (dd, dp) = ((a.db - b.db).abs(), dphase.abs());
        if dd > KERNEL_TOLERANCE.0 || dp > KERNEL_TOLERANCE.1 || dd.is_nan() {
            let what = match a.kind {
                KernelKind::Hammerstein { level_db } => format!("G{} at {level_db:+} dB re knee", a.order),
                KernelKind::Volterra => format!("V{}", a.order),
            };
            moved.push(format!(
                "C12 kernels: {what} at {:.0} Hz moved {:+.3} dB, {:+.2}° (tolerance {} dB, {}°)",
                a.hz,
                b.db - a.db,
                -dphase,
                KERNEL_TOLERANCE.0,
                KERNEL_TOLERANCE.1
            ));
        }
    }
    moved
}
