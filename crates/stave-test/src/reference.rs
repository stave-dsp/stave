//! Model vs reference: one harness for every test that holds a model to
//! something outside the code — an ngspice simulation of a netlist, or a
//! file a script solved from the defining equations.
//!
//! A [`Case`] names its reference, the stimulus, the comparison
//! [`Metric`] and an explicit budget; the model under test runs inside
//! it ([`Case::stage`] for a ticking stage, [`Case::pointwise`] for a
//! static transfer or a frequency response, [`Case::compare`] for
//! anything else). [`Outcome::assert`] fails with the metric, the budget,
//! where the worst error is, and the path of a plot-friendly dump
//! (`target/reference-diff/<case>.tsv`: abscissa, reference, model and
//! error columns).
//!
//! A reference can also be a published curve, digitised from a
//! manufacturer's datasheet ([`Datasheet`], DSP.md R23): the only
//! hardware truth where no capture exists.
//!
//! ```
//! use stave_dsp::{Coupler, Stage};
//! use stave_test::reference::{Case, Drive, Metric, Stimulus};
//!
//! // 22 nF into a 1 MΩ grid leak.
//! let mut coupler = Coupler::new((1.0 / (std::f64::consts::TAU * 22e-9 * 1e6)) as f32);
//! coupler.prepare(48_000.0);
//! // spice/coupler.cir, simulated into spice/coupler.ngspice.txt
//! Case::ngspice("coupler")
//!     .stimulus(Stimulus::sine(1.0, 50.0))
//!     .budget(Metric::NormalizedRms, 2e-6)
//!     .stage(Drive::oversampled(1), |x| f64::from(coupler.tick(x as f32)))
//!     .assert();
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

use stave_measure::waveform::Waveform;

mod datasheet;
pub use datasheet::{Curve, Datasheet, Uncertainty};

/// Where the committed references live: `crates/stave-test/`.
const FIXTURES: &str = env!("CARGO_MANIFEST_DIR");

// ------------------------------------------------------------ Reference

/// What a reference's first column is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Seconds: a transient analysis or a time-stepped solution.
    Time,
    /// Hertz: an AC analysis (the values are magnitudes).
    Frequency,
    /// The swept source of a DC analysis.
    Sweep,
}

impl Axis {
    fn symbol(self) -> &'static str {
        match self {
            Axis::Time => "t",
            Axis::Frequency => "f",
            Axis::Sweep => "x",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Axis::Time => " s",
            Axis::Frequency => " Hz",
            Axis::Sweep => "",
        }
    }
}

/// Where a reference came from, and so how it is regenerated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// `spice/<name>.cir`, simulated by `scripts/ngspice-simulate.py`.
    /// The same netlist will build the stave circuit (DSP.md N1).
    Ngspice { netlist: PathBuf },
    /// `reference/<name>.txt`, written by a script in `scripts/`.
    Generated { script: &'static str },
    /// `datasheet/<name>.txt`, digitised once from a published figure
    /// (see [`Datasheet`]); never regenerated.
    Datasheet { document: String, figure: String },
}

/// A committed reference waveform: two columns, the abscissa (see
/// [`Axis`]) and the value.
#[derive(Clone, Debug)]
pub struct Reference {
    name: String,
    source: Source,
    path: PathBuf,
    axis: Axis,
    waveform: Waveform,
}

impl Reference {
    /// `spice/<name>.ngspice.txt`, the simulation of `spice/<name>.cir`.
    /// The axis follows the netlist's analysis (`.ac`, `.dc`, else time).
    pub fn ngspice(name: &str) -> Self {
        let dir = Path::new(FIXTURES).join("spice");
        let netlist = dir.join(format!("{name}.cir"));
        let text = std::fs::read_to_string(&netlist)
            .unwrap_or_else(|e| panic!("{}: {e}", netlist.display()));
        let analysis = |card: &str| {
            text.lines()
                .any(|l| l.trim_start().to_ascii_lowercase().starts_with(card))
        };
        let axis = if analysis(".ac ") {
            Axis::Frequency
        } else if analysis(".dc ") {
            Axis::Sweep
        } else {
            Axis::Time
        };
        Self::load(
            name,
            Source::Ngspice { netlist },
            dir.join(format!("{name}.ngspice.txt")),
            axis,
        )
    }

    /// `reference/<name>.txt`, a time series written by `script`
    /// (a path from the workspace root, such as `scripts/ja-reference.py`).
    pub fn generated(name: &str, script: &'static str) -> Self {
        let path = Path::new(FIXTURES).join(format!("reference/{name}.txt"));
        Self::load(name, Source::Generated { script }, path, Axis::Time)
    }

    fn load(name: &str, source: Source, path: PathBuf, axis: Axis) -> Self {
        let regenerate = match &source {
            Source::Ngspice { .. } => "scripts/ngspice-simulate.py",
            Source::Generated { script } => script,
            Source::Datasheet { .. } => "nothing: a digitised table is restored, not regenerated",
        };
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (regenerate with {regenerate})", path.display()));
        let waveform = Waveform::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Self {
            name: name.to_string(),
            source,
            path,
            axis,
            waveform,
        }
    }

    /// A reference from points already read (a [`Datasheet`] curve).
    fn from_points(
        name: String,
        source: Source,
        path: PathBuf,
        axis: Axis,
        abscissa: Vec<f64>,
        values: Vec<f64>,
    ) -> Self {
        Self {
            name,
            source,
            path,
            axis,
            waveform: Waveform {
                time_s: abscissa,
                value: values,
            },
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The netlist an ngspice reference simulates.
    pub fn netlist(&self) -> Option<&Path> {
        match &self.source {
            Source::Ngspice { netlist } => Some(netlist),
            Source::Generated { .. } | Source::Datasheet { .. } => None,
        }
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// Times, frequencies or swept values: the first column.
    pub fn abscissa(&self) -> &[f64] {
        &self.waveform.time_s
    }

    pub fn values(&self) -> &[f64] {
        &self.waveform.value
    }

    pub fn len(&self) -> usize {
        self.waveform.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waveform.value.is_empty()
    }

    fn describe(&self) -> String {
        let file = self
            .path
            .strip_prefix(workspace_root())
            .unwrap_or(&self.path)
            .display()
            .to_string();
        match &self.source {
            Source::Ngspice { .. } => {
                format!("{file} (ngspice; regenerate with scripts/ngspice-simulate.py)")
            }
            Source::Generated { script } => format!("{file} (regenerate with {script})"),
            Source::Datasheet { document, figure } => {
                format!("{file} (digitised from {document}, {figure})")
            }
        }
    }
}

// ------------------------------------------------------------- Stimulus

/// The signal that drives the model: a function of the reference's
/// abscissa (time for a stage; the frequency or swept value for a
/// pointwise model), with a description for failure messages.
pub struct Stimulus {
    describe: String,
    signal: Box<dyn Fn(f64) -> f64>,
}

impl Stimulus {
    pub fn new(describe: impl Into<String>, signal: impl Fn(f64) -> f64 + 'static) -> Self {
        Self {
            describe: describe.into(),
            signal: Box::new(signal),
        }
    }

    /// `amplitude · sin(2π hz t)`.
    pub fn sine(amplitude: f64, hz: f64) -> Self {
        Self::new(format!("sine, {amplitude} peak at {hz} Hz"), move |t| {
            amplitude * (std::f64::consts::TAU * hz * t).sin()
        })
    }

    /// `start + slope · x`: a linear sweep.
    pub fn ramp(start: f64, slope: f64) -> Self {
        Self::new(format!("ramp from {start}, {slope} per unit"), move |x| {
            start + slope * x
        })
    }

    /// The abscissa itself: the frequency of an AC point, the time of a
    /// closed-form solution.
    pub fn abscissa() -> Self {
        Self::new("the abscissa", |x| x)
    }

    pub fn at(&self, x: f64) -> f64 {
        (self.signal)(x)
    }
}

impl fmt::Debug for Stimulus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe)
    }
}

// --------------------------------------------------------------- Metric

/// How far the model is from the reference. Every error is ≥ 0, and
/// smaller is better.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Metric {
    /// The largest |model − reference|, in the reference's units.
    MaxAbs,
    /// The RMS of the difference, in the reference's units.
    Rms,
    /// The RMS of the difference over the RMS of the reference.
    NormalizedRms,
    /// The largest |model − reference| over the reference's peak |value|.
    MaxRelative,
    /// The largest |20 log10(model / reference)|, in dB: for magnitude
    /// responses.
    MaxDb,
    /// The largest difference of a harmonic's amplitude (1 through
    /// `harmonics`), over the reference's fundamental — a spectral error.
    /// Measured over the whole cycles at the end of the waveform.
    Harmonics {
        fundamental_hz: f64,
        harmonics: usize,
    },
    /// |THD(model) − THD(reference)| in dB, THD over harmonics 2 through
    /// `harmonics`, measured like [`Metric::Harmonics`].
    ThdDelta {
        fundamental_hz: f64,
        harmonics: usize,
    },
}

impl Metric {
    fn describe(self) -> String {
        match self {
            Metric::MaxAbs => "max |error|".into(),
            Metric::Rms => "RMS error".into(),
            Metric::NormalizedRms => "normalized RMS error".into(),
            Metric::MaxRelative => "max |error| / reference peak".into(),
            Metric::MaxDb => "max |error| in dB".into(),
            Metric::Harmonics {
                fundamental_hz,
                harmonics,
            } => format!(
                "max harmonic-amplitude error / fundamental ({harmonics} harmonics of {fundamental_hz} Hz)"
            ),
            Metric::ThdDelta {
                fundamental_hz,
                harmonics,
            } => format!("THD delta in dB ({harmonics} harmonics of {fundamental_hz} Hz)"),
        }
    }

    /// The error, and where the worst of it is (an index into the series,
    /// or for the spectral metrics a harmonic number).
    fn measure(self, abscissa: &[f64], reference: &[f64], model: &[f64]) -> (f64, Worst) {
        let largest = |error: &dyn Fn(usize) -> f64| {
            (0..reference.len())
                .map(|i| (error(i), i))
                .fold((0.0, 0), |best, (e, i)| {
                    if e > best.0 || (e.is_nan() && !best.0.is_nan()) {
                        (e, i)
                    } else {
                        best
                    }
                })
        };
        let abs_error = |i: usize| (model[i] - reference[i]).abs();
        let (worst_abs, at) = largest(&abs_error);
        let sample = Worst::Sample(at);
        let mean_square = |v: &mut dyn Iterator<Item = f64>| {
            v.map(|x| x * x).sum::<f64>() / reference.len() as f64
        };
        match self {
            Metric::MaxAbs => (worst_abs, sample),
            Metric::Rms => (
                mean_square(&mut (0..reference.len()).map(|i| model[i] - reference[i])).sqrt(),
                sample,
            ),
            Metric::NormalizedRms => {
                let error = mean_square(&mut (0..reference.len()).map(|i| model[i] - reference[i]));
                let power = mean_square(&mut reference.iter().copied());
                let ratio = if power > 0.0 { error / power } else { error };
                (ratio.sqrt(), sample)
            }
            Metric::MaxRelative => {
                let peak = reference.iter().fold(0.0, |p: f64, v| p.max(v.abs()));
                (worst_abs / peak, sample)
            }
            Metric::MaxDb => {
                let (e, i) = largest(&|i| (20.0 * (model[i] / reference[i]).log10()).abs());
                let e = if e.is_nan() { f64::INFINITY } else { e };
                (e, Worst::Sample(i))
            }
            Metric::Harmonics {
                fundamental_hz,
                harmonics,
            } => {
                let r = harmonic_amplitudes(abscissa, reference, fundamental_hz, harmonics);
                let m = harmonic_amplitudes(abscissa, model, fundamental_hz, harmonics);
                let (e, k) = (0..harmonics)
                    .map(|k| ((m[k] - r[k]).abs() / r[0], k))
                    .fold((0.0, 0), |b, x| if x.0 > b.0 { x } else { b });
                (e, Worst::Harmonic(k + 1))
            }
            Metric::ThdDelta {
                fundamental_hz,
                harmonics,
            } => {
                let thd_db = |v: &[f64]| {
                    let a = harmonic_amplitudes(abscissa, v, fundamental_hz, harmonics);
                    let distortion = a[1..].iter().map(|x| x * x).sum::<f64>().sqrt();
                    20.0 * (distortion / a[0]).log10()
                };
                ((thd_db(model) - thd_db(reference)).abs(), Worst::Whole)
            }
        }
    }
}

/// The amplitudes of harmonics 1..=count of `fundamental_hz` over the
/// whole cycles at the end of a uniformly sampled series (a DFT bin each:
/// exact for a periodic signal, no window).
fn harmonic_amplitudes(time_s: &[f64], v: &[f64], fundamental_hz: f64, count: usize) -> Vec<f64> {
    assert!(time_s.len() >= 2, "a spectral metric needs a time series");
    let step = (time_s[time_s.len() - 1] - time_s[0]) / (time_s.len() - 1) as f64;
    let per_cycle = 1.0 / (fundamental_hz * step);
    let cycles = ((time_s.len() as f64 - 1.0) / per_cycle).floor();
    assert!(cycles >= 1.0, "a spectral metric needs a whole cycle");
    let n = (cycles * per_cycle).round() as usize;
    let tail = &v[v.len() - n..];
    (1..=count)
        .map(|k| {
            let w = std::f64::consts::TAU * k as f64 * cycles / n as f64;
            let (re, im) = tail
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(re, im), (i, x)| {
                    let p = w * i as f64;
                    (re + x * p.cos(), im - x * p.sin())
                });
            2.0 * (re * re + im * im).sqrt() / n as f64
        })
        .collect()
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Worst {
    Sample(usize),
    Harmonic(usize),
    Whole,
}

// ---------------------------------------------------------------- Drive

/// How a ticking stage runs against a time-domain reference: at
/// `oversample`× the reference's rate, read at the reference's instants
/// (a decimation without anti-alias filtering: ngspice samples a
/// continuous waveform, so this is the like-for-like comparison).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Drive {
    pub oversample: usize,
    /// Tick `i` reads the stimulus at the end of its step, `(i + 1) / rate`,
    /// and the reference's first point is one reference step in (a
    /// stage that integrates over the step, such as a supply under load).
    /// Otherwise tick `i` reads it at `i / rate` and the reference
    /// starts at zero.
    pub at_step_end: bool,
}

impl Drive {
    pub fn oversampled(oversample: usize) -> Self {
        assert!(oversample >= 1);
        Self {
            oversample,
            at_step_end: false,
        }
    }

    pub fn at_step_end(self) -> Self {
        Self {
            at_step_end: true,
            ..self
        }
    }
}

// ----------------------------------------------------------------- Case

type Map = Box<dyn Fn(f64) -> f64>;

/// One model held to one reference within a budget.
pub struct Case {
    name: String,
    reference: Reference,
    stimulus: Stimulus,
    metric: Metric,
    budget: f64,
    rate: f64,
    about: Option<f64>,
    reference_map: Option<(String, Map)>,
    up_to_scale: bool,
}

impl Case {
    /// A case against `spice/<name>.cir`'s simulation.
    pub fn ngspice(name: &str) -> Self {
        Self::new(name, Reference::ngspice(name))
    }

    /// A case against `reference/<name>.txt`, written by `script`.
    pub fn generated(name: &str, script: &'static str) -> Self {
        Self::new(name, Reference::generated(name, script))
    }

    /// A case against any reference. The budget must be set
    /// ([`Case::budget`]); the stimulus defaults to the abscissa and the
    /// reference's rate to 48 kHz.
    pub fn new(name: &str, reference: Reference) -> Self {
        Self {
            name: name.to_string(),
            reference,
            stimulus: Stimulus::abscissa(),
            metric: Metric::NormalizedRms,
            budget: f64::NAN,
            rate: 48_000.0,
            about: None,
            reference_map: None,
            up_to_scale: false,
        }
    }

    /// Rename the case (several cases on one reference, or one case on
    /// several references).
    pub fn named(self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }

    pub fn stimulus(self, stimulus: Stimulus) -> Self {
        Self { stimulus, ..self }
    }

    /// The metric and the largest error it may report.
    pub fn budget(self, metric: Metric, budget: f64) -> Self {
        Self {
            metric,
            budget,
            ..self
        }
    }

    /// The reference's sample rate (48 kHz unless set).
    pub fn rate(self, rate: f64) -> Self {
        Self { rate, ..self }
    }

    /// Compare swings about `point` (both sides minus it): the error is
    /// then relative to the signal, not to the operating point.
    pub fn about(self, point: f64) -> Self {
        Self {
            about: Some(point),
            ..self
        }
    }

    /// [`Case::about`] the reference's first value: its operating point.
    pub fn about_operating_point(self) -> Self {
        let point = self.reference.values()[0];
        self.about(point)
    }

    /// Map the reference's values before comparing (such as an
    /// inverting stage's swing, `q − v`), described for the failure.
    pub fn map_reference(self, describe: &str, map: impl Fn(f64) -> f64 + 'static) -> Self {
        Self {
            reference_map: Some((describe.to_string(), Box::new(map))),
            ..self
        }
    }

    /// Compare up to one fitted, positive scale on the reference (the
    /// least-squares level calibration): a model that is right but for
    /// its level passes; one inverted relative to the reference fails.
    pub fn up_to_scale(self) -> Self {
        Self {
            up_to_scale: true,
            ..self
        }
    }

    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    /// Run a stage against a time-domain reference: `tick` takes the
    /// stimulus at each oversampled step and returns the output.
    pub fn stage(&self, drive: Drive, mut tick: impl FnMut(f64) -> f64) -> Outcome {
        let os = drive.oversample;
        let rate = self.rate * os as f64;
        let offset = usize::from(drive.at_step_end);
        let run: Vec<f64> = (0..self.reference.len() * os)
            .map(|i| tick(self.stimulus.at((i + offset) as f64 / rate)))
            .skip(offset * (os - 1))
            .step_by(os)
            .collect();
        let context = format!(
            "{os}× the reference's {} Hz{}",
            self.rate,
            if drive.at_step_end {
                ", read at each step's end"
            } else {
                ""
            }
        );
        self.judge(&run, &context)
    }

    /// Evaluate a model at every reference point: `model(stimulus(x))`
    /// for each abscissa `x` (a static transfer, a frequency response, a
    /// closed form).
    pub fn pointwise(&self, mut model: impl FnMut(f64) -> f64) -> Outcome {
        let run: Vec<f64> = self
            .reference
            .abscissa()
            .iter()
            .map(|&x| model(self.stimulus.at(x)))
            .collect();
        self.judge(&run, "pointwise")
    }

    /// Compare a run the test produced itself, one value per reference
    /// point.
    pub fn compare(&self, run: &[f64]) -> Outcome {
        self.judge(run, "a run from the test")
    }

    fn judge(&self, run: &[f64], context: &str) -> Outcome {
        assert!(
            !self.budget.is_nan(),
            "case `{}`: no budget (Case::budget)",
            self.name
        );
        assert_eq!(
            run.len(),
            self.reference.len(),
            "case `{}`: the model gave {} points for the reference's {}",
            self.name,
            run.len(),
            self.reference.len()
        );
        let about = self.about.unwrap_or(0.0);
        let mut reference: Vec<f64> = self
            .reference
            .values()
            .iter()
            .map(|&v| match &self.reference_map {
                Some((_, map)) => map(v - about),
                None => v - about,
            })
            .collect();
        let model: Vec<f64> = run.iter().map(|v| v - about).collect();
        let mut scale = None;
        if self.up_to_scale {
            let s = model
                .iter()
                .zip(&reference)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                / reference.iter().map(|b| b * b).sum::<f64>();
            reference.iter_mut().for_each(|v| *v *= s);
            scale = Some(s);
        }
        let (mut error, worst) = self
            .metric
            .measure(self.reference.abscissa(), &reference, &model);
        let mut problem = None;
        if scale.is_some_and(|s| s.is_nan() || s <= 0.0) {
            problem = Some(format!(
                "the fitted scale is {}: the model is inverted (or silent) relative to the reference",
                scale.unwrap_or(f64::NAN)
            ));
            error = f64::INFINITY;
        }
        let worst = match worst {
            Worst::Sample(i) => format!(
                "at {} = {:.9e}{} (point {i} of {}): reference {:.9e}, model {:.9e}, error {:.3e}",
                self.reference.axis().symbol(),
                self.reference.abscissa()[i],
                self.reference.axis().unit(),
                reference.len(),
                reference[i],
                model[i],
                model[i] - reference[i]
            ),
            Worst::Harmonic(k) => format!("at harmonic {k}"),
            Worst::Whole => "over the whole waveform".to_string(),
        };
        let outcome = Outcome {
            case: self.name.clone(),
            metric: self.metric,
            budget: self.budget,
            error,
            worst,
            scale,
            problem,
            setting: format!(
                "reference {}\n  stimulus  {:?}{}{}\n  model     {context}",
                self.reference.describe(),
                self.stimulus,
                self.about
                    .map(|p| format!(", swings about {p}"))
                    .unwrap_or_default(),
                self.reference_map
                    .as_ref()
                    .map(|(d, _)| format!(", reference mapped by {d}"))
                    .unwrap_or_default(),
            ),
            dump: None,
        };
        let wanted = std::env::var_os("STAVE_REFERENCE_DUMP").is_some();
        if wanted || !outcome.passed() {
            let path = dump(
                &self.name,
                self.reference.axis(),
                self.reference.abscissa(),
                &reference,
                &model,
            );
            return Outcome {
                dump: Some(path),
                ..outcome
            };
        }
        outcome
    }
}

// -------------------------------------------------------------- Outcome

/// A case's result: the error against the budget, and where it is worst.
#[derive(Clone, Debug)]
#[must_use = "an outcome fails nothing until asserted"]
pub struct Outcome {
    pub case: String,
    pub metric: Metric,
    pub budget: f64,
    pub error: f64,
    /// Where the error is largest, with both values there.
    pub worst: String,
    /// The fitted scale, under [`Case::up_to_scale`].
    pub scale: Option<f64>,
    problem: Option<String>,
    setting: String,
    /// The dump: written on a failure, or on every case when
    /// `STAVE_REFERENCE_DUMP` is set.
    pub dump: Option<PathBuf>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.problem.is_none() && self.error <= self.budget
    }

    /// Print the outcome (`cargo test -- --nocapture` lists every case),
    /// panic if the error is over the budget; return the error.
    #[track_caller]
    pub fn assert(self) -> f64 {
        println!("{self}");
        assert!(self.passed(), "{self}");
        self.error
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.passed() { "ok" } else { "FAILED" };
        let relation = if self.error <= self.budget {
            "≤"
        } else {
            ">"
        };
        writeln!(
            f,
            "reference case `{}` {verdict}: {} {:.3e} {relation} budget {:e}",
            self.case,
            self.metric.describe(),
            self.error,
            self.budget
        )?;
        if let Some(problem) = &self.problem {
            writeln!(f, "  problem   {problem}")?;
        }
        writeln!(f, "  worst     {}", self.worst)?;
        if let Some(s) = self.scale {
            writeln!(f, "  scale     {s:.9e} (fitted)")?;
        }
        write!(f, "  {}", self.setting)?;
        if let Some(path) = &self.dump {
            let short = path.strip_prefix(workspace_root()).unwrap_or(path);
            write!(
                f,
                "\n  dump      {} (abscissa reference model error)",
                short.display()
            )?;
        }
        Ok(())
    }
}

/// A slower step must give a larger error by at least `ratio` (4 for a
/// second-order method halving its step): the model converges to the
/// reference rather than to a model of its own.
#[track_caller]
pub fn assert_converges(coarse: &Outcome, fine: &Outcome, ratio: f64) {
    assert!(
        fine.error * ratio < coarse.error,
        "case `{}` is not converging: {:.3e} → {:.3e} as the step halves (wanted at least {ratio}×)",
        coarse.case,
        coarse.error,
        fine.error
    );
}

// ----------------------------------------------------------------- dump

fn workspace_root() -> &'static Path {
    let manifest = Path::new(FIXTURES);
    manifest
        .ancestors()
        .find(|d| d.join("Cargo.lock").exists())
        .unwrap_or(manifest)
}

/// `STAVE_REFERENCE_OUT`, else `target/reference-diff` of the workspace.
fn dump_dir() -> PathBuf {
    std::env::var_os("STAVE_REFERENCE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target/reference-diff"))
}

fn dump(case: &str, axis: Axis, abscissa: &[f64], reference: &[f64], model: &[f64]) -> PathBuf {
    use std::fmt::Write as _;
    let dir = dump_dir();
    let file: String = case
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{file}.tsv"));
    let unit = axis.unit().trim();
    let column = if unit.is_empty() {
        axis.symbol().to_string()
    } else {
        format!("{} ({unit})", axis.symbol())
    };
    let mut text = format!("# {column}\treference\tmodel\terror\n");
    for i in 0..reference.len() {
        let _ = writeln!(
            text,
            "{:.12e}\t{:.12e}\t{:.12e}\t{:.6e}",
            abscissa[i],
            reference[i],
            model[i],
            model[i] - reference[i]
        );
    }
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
        eprintln!("could not write {}: {e}", path.display());
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampled(n: usize, f: impl Fn(f64) -> f64) -> (Vec<f64>, Vec<f64>) {
        let t: Vec<f64> = (0..n).map(|i| i as f64 / 48_000.0).collect();
        let v = t.iter().map(|&t| f(t)).collect();
        (t, v)
    }

    #[test]
    fn sample_metrics_find_the_worst_point() {
        let (t, r) = sampled(100, |t| (std::f64::consts::TAU * 480.0 * t).sin());
        let mut m = r.clone();
        m[37] += 0.01;
        let (e, w) = Metric::MaxAbs.measure(&t, &r, &m);
        assert!((e - 0.01).abs() < 1e-12);
        assert_eq!(w, Worst::Sample(37));
        let (rms, _) = Metric::Rms.measure(&t, &r, &m);
        assert!((rms - 0.001).abs() < 1e-9);
        let (nrms, _) = Metric::NormalizedRms.measure(&t, &r, &m);
        assert!((nrms - 0.001 / 0.5f64.sqrt()).abs() < 1e-6);
        let (db, _) = Metric::MaxDb.measure(&t, &[1.0, 2.0], &[1.0, 2.0 * 10f64.powf(0.05)]);
        assert!((db - 1.0).abs() < 1e-12);
    }

    #[test]
    fn spectral_metrics_read_harmonics() {
        let wave = |h3: f64| {
            move |t: f64| {
                let w = std::f64::consts::TAU * 1_000.0 * t;
                w.sin() + h3 * (3.0 * w).sin()
            }
        };
        let (t, r) = sampled(480, wave(0.1));
        let (_, m) = sampled(480, wave(0.11));
        let h = Metric::Harmonics {
            fundamental_hz: 1_000.0,
            harmonics: 5,
        };
        let (e, w) = h.measure(&t, &r, &m);
        assert!((e - 0.01).abs() < 1e-9, "{e}");
        assert_eq!(w, Worst::Harmonic(3));
        let thd = Metric::ThdDelta {
            fundamental_hz: 1_000.0,
            harmonics: 5,
        };
        let (e, _) = thd.measure(&t, &r, &m);
        assert!((e - 20.0 * 1.1f64.log10()).abs() < 1e-9, "{e}");
    }

    #[test]
    fn a_case_fails_with_the_worst_point_and_a_dump() {
        let dir = std::env::temp_dir().join(format!("stave-reference-{}", std::process::id()));
        std::env::set_var("STAVE_REFERENCE_OUT", &dir);
        let case = Case::ngspice("rc_step").budget(Metric::MaxAbs, 1e-3);
        let mut run = case.reference().values().to_vec();
        run[100] += 0.01;
        let outcome = case.compare(&run);
        assert!(!outcome.passed());
        let text = outcome.to_string();
        assert!(text.contains("point 100 of 241"), "{text}");
        assert!(text.contains("budget 1e-3"), "{text}");
        let dump = outcome.dump.expect("a failure writes a dump");
        assert_eq!(std::fs::read_to_string(&dump).unwrap().lines().count(), 242);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_inverted_model_fails_a_scaled_comparison() {
        let case = Case::ngspice("rc_step")
            .budget(Metric::NormalizedRms, 1.0)
            .up_to_scale();
        let run: Vec<f64> = case.reference().values().iter().map(|v| -2.0 * v).collect();
        assert!(!case.compare(&run).passed());
        let run: Vec<f64> = case.reference().values().iter().map(|v| 2.0 * v).collect();
        let outcome = case.compare(&run);
        assert!(outcome.passed() && (outcome.scale.unwrap() - 2.0).abs() < 1e-12);
    }
}
