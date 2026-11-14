//! Fitting (DSP.md G4): the values of a design's free parameters that
//! best meet a target — a magnitude or phase response (a datasheet
//! table, a measurement, another model), a harmonic-vs-level curve, or
//! a capture pair — with the error in budgets as R1 reports it, the
//! R16 audibility verdict where the target is audio, and, on request,
//! the parts snapped to preferred values with the error that costs.
//!
//! ```
//! use stave_dsp::circuit::netlist::tolerance::Tolerances;
//! use stave_dsp::circuit::netlist::Netlist;
//! use stave_measure::design::{fit, Bench, Fitting, Free, Parts, Quantity, Target, Budget};
//!
//! // A 4k7/33n low-pass into a 10k load: its response is the target.
//! let deck = |r: &str, c: &str| {
//!     Netlist::parse_spice(&format!(
//!         "* rc\nV1 in 0 AC 1\nR1 in out {r}\nC1 out 0 {c}\nR2 out 0 10k\n.control\nwrdata o v(out)\n.endc"
//!     ))
//!     .unwrap()
//! };
//! let bands: Vec<Quantity> = (0..10).map(|i| Quantity::ResponseDb { hz: 50.0 * 2f64.powi(i) }).collect();
//! let truth = Parts::new(&deck("4.7k", "33n"), &Tolerances::new());
//! let target = Target::measured(&truth, &[4.7e3, 33e-9, 10e3], &bands, &Bench::default(), Budget::default()).unwrap();
//! // The same circuit, built wrong: free R1 and C1 within physical bounds.
//! let built = Parts::new(&deck("1k", "100n"), &Tolerances::new());
//! let free = [Free::new("r1", 100.0, 1e6), Free::new("c1", 1e-9, 1e-6)];
//! let f = fit(&built, &free, &target, &Fitting::default()).unwrap();
//! assert!(f.passed(), "{f}");
//! assert!((f.value("r1").unwrap() / 4.7e3 - 1.0).abs() < 1e-6);
//! assert!((f.value("c1").unwrap() / 33e-9 - 1.0).abs() < 1e-6);
//! ```
//!
//! # Models
//!
//! A fit is a [`Design`] (a netlist's [`Parts`](super::Parts), pot
//! rotations among them by [`Parts::setting`](super::Parts::setting),
//! or any model's [`Parameters`](super::Parameters)), the parameters to
//! free with their physical bounds ([`Free`]; the rest stay at their
//! nominal values), and a [`Target`]:
//!
//! - **Points**: quantities and the values they should read, each with
//!   its budget ([`Point`]) — a response or phase table
//!   ([`Target::response`], [`Target::phase`]), a harmonic or THD
//!   against level ([`Target::harmonic`], [`Target::thd`]), or another
//!   model measured at any quantities ([`Target::measured`]). The
//!   error is the worst |model − target| over its budget, R1's
//!   worst-point form, so a fit within 1 is a fit within budget.
//! - **A capture**: an input and the output it gave ([`Target::capture`]),
//!   the model run on the input; the error is the normalised RMS
//!   difference over its budget (R1's `NormalizedRms`), and the
//!   residual is judged by R16's null NMR (level and latency aligned).
//!
//! # Derivation
//!
//! [`levenberg_marquardt`](crate::levenberg_marquardt) — E11's solver,
//! shared: least squares in the bounded (logistic, log-scaled across
//! decades) coordinates, then for points the L_p continuation toward
//! the minimax fit the worst-point budget judges (a capture's error is
//! an RMS, so least squares is its fit). Several starts on request
//! ([`Fitting::starts`]): the first from the given or nominal values,
//! the rest spread over the bounds by a Halton sequence; the best by
//! error wins. Snapping ([`Free::snap`]): each snapped part's two
//! preferred values either side of its fit, every combination tried
//! (up to 2⁸; else the nearest each), the best kept, and any free
//! parameter that is not snapped (a pot's rotation) refitted around
//! them; the report gives both errors and the difference.
//!
//! # Verified
//!
//! `crates/stave-measure/src/design/fit/tests.rs`: an RC into a load
//! recovers 4.7 kΩ and 33 nF from its response and phase to 1e-6 (the
//! error 1.8e-14 budgets); a fit of 4.62 kΩ and 34.1 nF snapped to E24
//! and E12 keeps the best of the four neighbouring pairs, 4.7 kΩ and
//! 33 nF, at the cost it reports (13.5 budgets of 0.01 dB); a tanh's
//! drive recovered from its third-harmonic-vs-level curve to 1e-6; a
//! one-pole's cutoff and gain recovered from a capture to 1e-6, the
//! residual inaudible by NMR (−159 dB total), and with bounds that
//! exclude the truth, pinned at a bound, over budget and AUDIBLE.
//! `crates/stave-test/tests/design_fit.rs`: the Fender AB763 tone stack
//! recovers its treble and bass rotations, slope resistor and mid
//! capacitor from its own response to 1e-6; fitted to the Marshall
//! 2203's ngspice response by least squares it meets it within 1.5e-12
//! dB (14.0 dB as drawn), 0.188 dB at E12, each held to the 2203's deck
//! by the R1 harness itself.
//!
//! # Cost
//!
//! A fit evaluates the design `2n + 1` times an iteration or more (`n`
//! free parameters; the Jacobian's columns over [`Bench::threads`]),
//! up to 150 iterations a stage, four stages for points, times the
//! starts. A circuit's response is its linearisation (a build and a
//! solve per evaluation); a sine quantity runs the bench's settle and
//! window; a capture runs its length.
//!
//! # Aliasing
//!
//! None: an analysis. A capture is compared at its own rate.
//!
//! # Valid ranges
//!
//! Bounds finite, `lo < hi`, positive when they span more than a
//! decade (they are then searched by ratio). A local method: a target
//! the design can meet several ways (an RC's response alone fixes only
//! the product RC) fits to one of them; free what the target
//! determines. The audibility verdict needs a capture at 32 kHz or
//! more.

use std::fmt;

use super::{measure, Bench, Design, Error, Quantity, Series, Subject};
use crate::levenberg_marquardt::{self as lm, Bounded, MINIMAX};
use crate::perceptual::null::{self, Null};
use crate::perceptual::Playback;

/// A parameter a fit frees: its bounds, where to start, and the series
/// it is bought in.
#[derive(Clone, Debug, PartialEq)]
pub struct Free {
    pub name: String,
    pub lo: f64,
    pub hi: f64,
    /// The starting value (the design's nominal, clamped inside the
    /// bounds, when none).
    pub start: Option<f64>,
    /// Snap the fitted value to this series afterwards.
    pub series: Option<Series>,
}

impl Free {
    pub fn new(name: &str, lo: f64, hi: f64) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
            lo,
            hi,
            start: None,
            series: None,
        }
    }

    /// Start from `value`.
    pub fn from(mut self, value: f64) -> Self {
        self.start = Some(value);
        self
    }

    /// Snap the fitted value to `series` and report what it costs.
    pub fn snap(mut self, series: Series) -> Self {
        self.series = Some(series);
        self
    }
}

/// One value a quantity should read, within `budget` (its unit's).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Point {
    pub quantity: Quantity,
    pub value: f64,
    pub budget: f64,
}

/// The budgets a measured target holds each quantity to.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Budget {
    /// Responses, gains, THD and harmonics, dB.
    pub db: f64,
    /// Phases, degrees.
    pub degrees: f64,
}

impl Budget {
    pub fn of(&self, quantity: &Quantity) -> f64 {
        match quantity {
            Quantity::PhaseDegrees { .. } => self.degrees,
            _ => self.db,
        }
    }
}

impl Default for Budget {
    /// 0.1 dB and 1°: a just-noticeable level difference with margin.
    fn default() -> Self {
        Self { db: 0.1, degrees: 1.0 }
    }
}

/// What a fit aims at.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Quantities and the values they should read.
    Points(Vec<Point>),
    /// An input, the output it gave at `sample_rate`, and the budget
    /// on the normalised RMS difference; the first `skip` samples (the
    /// settling) are not compared.
    Capture {
        input: Vec<f64>,
        output: Vec<f64>,
        sample_rate: f64,
        budget: f64,
        skip: usize,
    },
}

impl Target {
    /// A magnitude response: `(hz, dB)` within `budget_db`.
    pub fn response(table: &[(f64, f64)], budget_db: f64) -> Self {
        Self::Points(table.iter().map(|&(hz, value)| Point { quantity: Quantity::ResponseDb { hz }, value, budget: budget_db }).collect())
    }

    /// A phase response: `(hz, degrees)` within `budget_degrees`.
    pub fn phase(table: &[(f64, f64)], budget_degrees: f64) -> Self {
        Self::Points(
            table
                .iter()
                .map(|&(hz, value)| Point { quantity: Quantity::PhaseDegrees { hz }, value, budget: budget_degrees })
                .collect(),
        )
    }

    /// Harmonic `harmonic` of a sine at `hz` against its level:
    /// `(amplitude, dBc)` within `budget_db`.
    pub fn harmonic(hz: f64, harmonic: u32, table: &[(f64, f64)], budget_db: f64) -> Self {
        Self::Points(
            table
                .iter()
                .map(|&(amplitude, value)| Point { quantity: Quantity::HarmonicDb { hz, amplitude, harmonic }, value, budget: budget_db })
                .collect(),
        )
    }

    /// THD of a sine at `hz` against its level: `(amplitude, dB)`.
    pub fn thd(hz: f64, table: &[(f64, f64)], budget_db: f64) -> Self {
        Self::Points(
            table
                .iter()
                .map(|&(amplitude, value)| Point { quantity: Quantity::ThdDb { hz, amplitude }, value, budget: budget_db })
                .collect(),
        )
    }

    /// Another model (`design` at `values`) measured at `quantities`.
    pub fn measured<D: Design>(design: &D, values: &[f64], quantities: &[Quantity], bench: &Bench, budget: Budget) -> Result<Self, Error> {
        let mut subject = design.build(values)?;
        let read = measure(&mut subject, quantities, bench);
        Ok(Self::Points(
            quantities
                .iter()
                .zip(read)
                .map(|(q, value)| Point { quantity: *q, value, budget: budget.of(q) })
                .collect(),
        ))
    }

    /// A capture pair: `input` and the `output` it gave at
    /// `sample_rate`, within `budget` normalised RMS.
    pub fn capture(input: &[f64], output: &[f64], sample_rate: f64, budget: f64) -> Self {
        Self::Capture {
            input: input.to_vec(),
            output: output.to_vec(),
            sample_rate,
            budget,
            skip: 0,
        }
    }

    /// Leave the first `samples` of a capture out of the comparison.
    pub fn skipping(self, samples: usize) -> Self {
        match self {
            Self::Capture { input, output, sample_rate, budget, .. } => Self::Capture { input, output, sample_rate, budget, skip: samples },
            points => points,
        }
    }

    /// Both targets' points at once (a response and its phase).
    pub fn and(self, other: Target) -> Self {
        match (self, other) {
            (Self::Points(mut a), Self::Points(b)) => {
                a.extend(b);
                Self::Points(a)
            }
            (a, _) => a,
        }
    }
}

/// How a fit runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitting {
    /// What the sine quantities are measured on, and the threads.
    pub bench: Bench,
    /// The L_p stages for a points target (a capture's fit is least
    /// squares alone).
    pub exponents: Vec<f64>,
    /// Starts: the given values, then Halton points over the bounds.
    pub starts: usize,
}

impl Default for Fitting {
    fn default() -> Self {
        Self {
            bench: Bench::default(),
            exponents: MINIMAX.to_vec(),
            starts: 1,
        }
    }
}

/// One point as fitted: what it should read, what the model reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub unit: &'static str,
    pub target: f64,
    pub model: f64,
    pub budget: f64,
}

impl Row {
    /// The error in budgets (phase wrapped to ±180°).
    pub fn error(&self) -> f64 {
        let mut d = self.model - self.target;
        if self.unit == "°" {
            d = (d + 180.0).rem_euclid(360.0) - 180.0;
        }
        d / self.budget
    }
}

/// A freed parameter's outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub name: String,
    pub value: f64,
    pub start: f64,
    pub lo: f64,
    pub hi: f64,
    /// Pinned at a bound: the target wants more than the bounds allow.
    pub at_bound: bool,
}

/// The fit with its parts at preferred values.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    /// Every parameter's value, snapped (and the rest refitted).
    pub values: Vec<f64>,
    /// `(name, series, fitted, snapped)` for each snapped part.
    pub parts: Vec<(String, Series, f64, f64)>,
    pub rows: Vec<Row>,
    /// The error in budgets at the snapped values.
    pub error: f64,
    /// What snapping costs: `error` less the fit's.
    pub cost: f64,
}

/// A finished fit: the values, the error in budgets, where it is
/// worst, its audibility, and the snapped alternative.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub name: String,
    /// Every parameter of the design, in order, and its value.
    pub parameters: Vec<String>,
    pub values: Vec<f64>,
    pub free: Vec<Fitted>,
    /// Each point as fitted (empty for a capture).
    pub rows: Vec<Row>,
    /// The error in budgets: the worst point's |error| / budget, or a
    /// capture's normalised RMS / budget. Within budget at ≤ 1.
    pub error: f64,
    /// A capture's normalised RMS difference and its budget.
    pub capture: Option<(f64, f64)>,
    /// R16's null verdict on a capture's residual, or why there is none.
    pub audibility: Result<Null, String>,
    pub snapped: Option<Snapped>,
    /// Each stage's exponent and error (in budgets), the best start's.
    pub stages: Vec<(f64, f64)>,
}

impl Fit {
    /// A fitted (or fixed) parameter's value.
    pub fn value(&self, name: &str) -> Option<f64> {
        let name = name.to_ascii_lowercase();
        self.parameters.iter().position(|p| *p == name).map(|i| self.values[i])
    }

    /// Within budget: an error of at most 1.
    pub fn passed(&self) -> bool {
        self.error <= 1.0
    }

    /// Name the fit for its report.
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// Print the report and panic if over budget; return the error.
    #[track_caller]
    pub fn assert(self) -> f64 {
        println!("{self}");
        assert!(self.passed(), "{self}");
        self.error
    }

    fn worst_row(rows: &[Row]) -> Option<&Row> {
        rows.iter().max_by(|a, b| a.error().abs().total_cmp(&b.error().abs()))
    }
}

/// Exponent notation, short.
fn e(x: f64) -> String {
    format!("{x:.6e}")
}

impl fmt::Display for Fit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.passed() { "ok" } else { "FAILED" };
        let relation = if self.passed() { "≤" } else { ">" };
        match self.capture {
            Some((nrms, budget)) => writeln!(
                f,
                "fit `{}` {verdict}: normalized RMS error {nrms:.3e} = {:.3e} budgets {relation} 1 (budget {budget:e})",
                self.name, self.error
            )?,
            None => writeln!(
                f,
                "fit `{}` {verdict}: max |error| / budget {:.3e} {relation} 1 ({} points)",
                self.name,
                self.error,
                self.rows.len()
            )?,
        }
        if let Some(w) = Self::worst_row(&self.rows) {
            writeln!(
                f,
                "  worst     {}: target {:.6} {u}, model {:.6} {u}, error {:+.3e} {u} (budget {} {u})",
                w.label,
                w.target,
                w.model,
                w.error() * w.budget,
                w.budget,
                u = w.unit
            )?;
        }
        match &self.audibility {
            Ok(n) => writeln!(
                f,
                "  audible   {}: null NMR {:.1} dB total, {:.1} dB worst band ({:.0} Hz); aligned {:+.4} dB, {:+.3} µs",
                null::verdict(n.nmr.total_db, n.nmr.max_db),
                n.nmr.total_db,
                n.nmr.max_db,
                n.nmr.max_hz,
                n.alignment.gain_db(),
                n.alignment.delay * 1e6
            )?,
            Err(why) => writeln!(f, "  audible   no verdict: {why}")?,
        }
        for (i, p) in self.free.iter().enumerate() {
            let head = if i == 0 { "  fitted   " } else { "           " };
            writeln!(
                f,
                "{head} {} = {} in [{:e}, {:e}] from {}{}",
                p.name,
                e(p.value),
                p.lo,
                p.hi,
                e(p.start),
                if p.at_bound { "  (at a bound)" } else { "" }
            )?;
        }
        if let Some(s) = &self.snapped {
            let parts: Vec<String> = s.parts.iter().map(|(n, series, _, v)| format!("{n} {} ({})", e(*v), series.name())).collect();
            writeln!(f, "  snapped   {}: error {:.3e} budgets, cost {:+.3e}", parts.join(", "), s.error, s.cost)?;
        }
        let stages: Vec<String> = self.stages.iter().map(|(p, err)| format!("p = {p}: {err:.3e}")).collect();
        write!(f, "  stages    {}", stages.join("; "))
    }
}

/// The error in budgets of a set of residuals: the worst for points,
/// the root sum of squares for a capture (its residuals are scaled so).
fn score(target: &Target, residuals: &[f64]) -> f64 {
    if residuals.iter().any(|r| !r.is_finite()) {
        return f64::INFINITY;
    }
    match target {
        Target::Points(_) => residuals.iter().fold(0.0, |m: f64, r| m.max(r.abs())),
        Target::Capture { .. } => residuals.iter().map(|r| r * r).sum::<f64>().sqrt(),
    }
}

/// A capture's input run through a subject at the capture's rate.
fn run(subject: &mut dyn Subject, input: &[f64], rate: f64) -> Vec<f64> {
    subject.prepare(rate);
    subject.reset();
    input.iter().map(|&x| subject.tick(x)).collect()
}

/// The model's readings at `values`: the points' quantities, or the
/// capture's output. `None` when it cannot be built.
fn readings<D: Design>(design: &D, values: &[f64], target: &Target, bench: &Bench) -> Option<Vec<f64>> {
    let mut subject = design.build(values).ok()?;
    Some(match target {
        Target::Points(points) => {
            let quantities: Vec<Quantity> = points.iter().map(|p| p.quantity).collect();
            measure(&mut subject, &quantities, bench)
        }
        Target::Capture { input, sample_rate, .. } => run(&mut subject, input, *sample_rate),
    })
}

/// The residuals, in budgets, of the readings against the target.
fn residuals(target: &Target, read: Option<&[f64]>) -> Vec<f64> {
    match target {
        Target::Points(points) => match read {
            Some(read) => points.iter().zip(read).map(|(p, &m)| row(p, m).error()).collect(),
            None => vec![f64::NAN; points.len()],
        },
        Target::Capture { output, budget, skip, .. } => {
            let compared = output.len().saturating_sub(*skip).max(1);
            let rms = (output[(*skip).min(output.len())..].iter().map(|v| v * v).sum::<f64>() / compared as f64).sqrt();
            let scale = 1.0 / (rms.max(f64::MIN_POSITIVE) * budget * (compared as f64).sqrt());
            match read {
                Some(read) => output.iter().zip(read).skip(*skip).map(|(o, m)| (m - o) * scale).collect(),
                None => vec![f64::NAN; compared],
            }
        }
    }
}

fn row(p: &Point, model: f64) -> Row {
    Row {
        label: p.quantity.label(),
        unit: p.quantity.unit(),
        target: p.value,
        model,
        budget: p.budget,
    }
}

/// The `i`th point of the Halton sequence in base `b`.
fn halton(mut i: usize, b: usize) -> f64 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= b as f64;
        r += f * (i % b) as f64;
        i /= b;
    }
    r
}

const PRIMES: [usize; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];

/// Fit `design`'s `free` parameters to `target` (see the module docs).
pub fn fit<D: Design>(design: &D, free: &[Free], target: &Target, fitting: &Fitting) -> Result<Fit, Error> {
    if free.is_empty() {
        return Err(Error::Empty("free parameters"));
    }
    if matches!(target, Target::Points(p) if p.is_empty()) || matches!(target, Target::Capture { input, .. } if input.is_empty()) {
        return Err(Error::Empty("target"));
    }
    if let Target::Capture { input, output, .. } = target {
        if input.len() != output.len() {
            return Err(Error::Build(format!("the capture's input has {} samples, its output {}", input.len(), output.len())));
        }
    }
    let nominal = super::nominal(design);
    let index: Vec<usize> = free.iter().map(|p| super::index_of(design, &p.name)).collect::<Result<_, _>>()?;
    for p in free {
        if p.lo >= p.hi || !p.lo.is_finite() || !p.hi.is_finite() {
            return Err(Error::Build(format!("{}: bounds [{}, {}]", p.name, p.lo, p.hi)));
        }
    }
    let bench = &fitting.bench;
    let full = |z: &[f64], base: &[f64], which: &[usize]| -> Vec<f64> {
        let mut v = base.to_vec();
        for (&i, &x) in which.iter().zip(z) {
            v[i] = x;
        }
        v
    };
    let bounded = |starts: &[f64]| -> Vec<Bounded> { free.iter().zip(starts).map(|(p, &s)| Bounded::new(s, p.lo, p.hi)).collect() };
    let exponents: Vec<f64> = match target {
        Target::Points(_) => fitting.exponents.clone(),
        Target::Capture { .. } => vec![2.0],
    };
    let first: Vec<f64> = free
        .iter()
        .zip(&index)
        .map(|(p, &i)| p.start.unwrap_or(nominal[i]).clamp(p.lo, p.hi))
        .collect();
    let template = bounded(&first);
    type Best = (f64, lm::Solution, Vec<(f64, f64)>);
    let mut best: Option<Best> = None;
    for s in 0..fitting.starts.max(1) {
        let starts: Vec<f64> = if s == 0 {
            first.clone()
        } else {
            template
                .iter()
                .enumerate()
                .map(|(k, b)| {
                    let u = halton(s, PRIMES[k % PRIMES.len()]).clamp(1e-3, 1.0 - 1e-3);
                    b.value((u / (1.0 - u)).ln())
                })
                .collect()
        };
        let objective = |z: &[f64]| -> Vec<f64> {
            let v = full(z, &nominal, &index);
            residuals(target, readings(design, &v, target, bench).as_deref())
        };
        let mut stages = Vec::new();
        let solution = lm::continuation(&bounded(&starts), &exponents, &objective, bench.threads, &mut |p, s| {
            stages.push((p, score(target, &s.residuals)));
        });
        let err = score(target, &solution.residuals);
        if best.as_ref().is_none_or(|(b, ..)| err < *b) {
            best = Some((err, solution, stages));
        }
    }
    let (error, solution, stages) = best.expect("at least one start");
    let values = full(&solution.values, &nominal, &index);
    let read = readings(design, &values, target, bench);
    let rows = rows_of(target, read.as_deref());
    let audibility = audibility(target, read.as_deref());
    let fitted = free
        .iter()
        .zip(&solution.values)
        .zip(&template)
        .map(|((p, &value), b)| Fitted {
            name: p.name.clone(),
            value,
            start: b.start,
            lo: p.lo,
            hi: p.hi,
            at_bound: b.at_bound(value),
        })
        .collect();
    let capture = match target {
        Target::Capture { budget, .. } => Some((error * budget, *budget)),
        Target::Points(_) => None,
    };
    let snapped = snap(design, free, &index, &values, target, fitting, error);
    Ok(Fit {
        name: "fit".into(),
        parameters: design.parameters().iter().map(|p| p.name.clone()).collect(),
        values,
        free: fitted,
        rows,
        error,
        capture,
        audibility,
        snapped,
        stages,
    })
}

fn rows_of(target: &Target, read: Option<&[f64]>) -> Vec<Row> {
    match (target, read) {
        (Target::Points(points), Some(read)) => points.iter().zip(read).map(|(p, &m)| row(p, m)).collect(),
        (Target::Points(points), None) => points.iter().map(|p| row(p, f64::NAN)).collect(),
        (Target::Capture { .. }, _) => Vec::new(),
    }
}

/// R16's verdict on a capture's residual (as R1 judges a time-domain
/// case: the swing about the capture's mean as the masker, full scale
/// at its peak, aligned within ±1 ms).
fn audibility(target: &Target, read: Option<&[f64]>) -> Result<Null, String> {
    let Target::Capture { output, sample_rate, skip, .. } = target else {
        return Err("not an audio-domain comparison (a response or a curve)".into());
    };
    let read = read.ok_or("the model could not be built")?;
    if *sample_rate < 32_000.0 {
        return Err(format!("captured at {sample_rate:.0} Hz, under the ear model's 32 kHz"));
    }
    let (o, m) = (&output[(*skip).min(output.len())..], &read[(*skip).min(read.len())..]);
    let mean = o.iter().sum::<f64>() / o.len().max(1) as f64;
    let r: Vec<f64> = o.iter().map(|v| v - mean).collect();
    let m: Vec<f64> = m.iter().map(|v| v - mean).collect();
    let peak = r.iter().fold(0.0f64, |p, v| p.max(v.abs()));
    if peak.is_nan() || peak <= 0.0 || m.iter().any(|v| !v.is_finite()) {
        return Err("a silent capture or a non-finite model".into());
    }
    Ok(null::between(&r, &m, *sample_rate, Playback::full_scale(peak), 1e-3))
}

/// Snap the parts that ask for it (module docs); `None` when none do.
fn snap<D: Design>(design: &D, free: &[Free], index: &[usize], values: &[f64], target: &Target, fitting: &Fitting, error: f64) -> Option<Snapped> {
    let snapped: Vec<(usize, Series)> = free.iter().enumerate().filter_map(|(k, p)| p.series.map(|s| (k, s))).collect();
    if snapped.is_empty() {
        return None;
    }
    let bench = &fitting.bench;
    let evaluate = |v: &[f64]| score(target, &residuals(target, readings(design, v, target, bench).as_deref()));
    let brackets: Vec<(f64, f64)> = snapped.iter().map(|&(k, s)| s.bracket(values[index[k]])).collect();
    let combos: Vec<Vec<f64>> = if snapped.len() <= 8 {
        (0..1usize << snapped.len())
            .map(|mask| brackets.iter().enumerate().map(|(j, &(lo, hi))| if mask >> j & 1 == 0 { lo } else { hi }).collect())
            .collect()
    } else {
        vec![snapped.iter().map(|&(k, s)| s.nearest(values[index[k]])).collect()]
    };
    let with = |combo: &[f64]| -> Vec<f64> {
        let mut v = values.to_vec();
        for (&(k, _), &x) in snapped.iter().zip(combo) {
            v[index[k]] = x;
        }
        v
    };
    let mut best = combos
        .iter()
        .map(|c| (evaluate(&with(c)), c))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| with(c))
        .expect("a combination");
    // Refit what is free but not snapped (a pot's rotation) around them.
    let rest: Vec<usize> = (0..free.len()).filter(|k| free[*k].series.is_none()).collect();
    if !rest.is_empty() {
        let at: Vec<usize> = rest.iter().map(|&k| index[k]).collect();
        let params: Vec<Bounded> = rest.iter().map(|&k| Bounded::new(best[index[k]], free[k].lo, free[k].hi)).collect();
        let base = best.clone();
        let objective = |z: &[f64]| -> Vec<f64> {
            let mut v = base.clone();
            for (&i, &x) in at.iter().zip(z) {
                v[i] = x;
            }
            residuals(target, readings(design, &v, target, bench).as_deref())
        };
        let exponents: Vec<f64> = match target {
            Target::Points(_) => fitting.exponents.clone(),
            Target::Capture { .. } => vec![2.0],
        };
        let s = lm::continuation(&params, &exponents, &objective, bench.threads, &mut |_, _| {});
        for (&i, &x) in at.iter().zip(&s.values) {
            best[i] = x;
        }
    }
    let read = readings(design, &best, target, bench);
    let snapped_error = score(target, &residuals(target, read.as_deref()));
    Some(Snapped {
        parts: snapped
            .iter()
            .map(|&(k, s)| (free[k].name.clone(), s, values[index[k]], best[index[k]]))
            .collect(),
        rows: rows_of(target, read.as_deref()),
        error: snapped_error,
        cost: snapped_error - error,
        values: best,
    })
}

#[cfg(test)]
mod tests;
