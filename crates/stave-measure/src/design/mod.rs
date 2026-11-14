//! Design tools (DSP.md G2–G4): a model run over its parts' values —
//! a grid of values ([`sweep`]), each part's sensitivity
//! ([`sensitivity`]: which resistor moves the tone most), built
//! units drawn from the parts' tolerances ([`monte_carlo`]: the spread
//! of response across a production run, and which part's grade
//! dominates it), and the values that best meet a target ([`fit()`]: a
//! response, a harmonic-vs-level curve or a capture, the error in
//! budgets, snapped to preferred values on request).
//!
//! Any model takes part: a [`Design`] says what its parameters are and
//! builds a [`Subject`] from their values. [`Parts`] is a netlist's
//! design (its resistors, capacitors, inductors and pots, graded by
//! [`Tolerances`]); [`Parameters`] wraps any closure that builds a
//! [`Circuit`], a hand-derived topology or any [`Stage`] ([`Staged`]).
//! What is measured are [`Quantity`]s: small-signal response and phase
//! (exact from a circuit's linearisation, else measured with a small
//! coherent sine), a sine's gain, its THD, and one harmonic's level.
//! A pot's rotation joins a netlist's parameters with
//! [`Parts::setting`].
//!
//! Evaluations run on [`Bench::threads`] threads (one by default; a
//! shell sets it from its caps — stave-test's `threads()` reads
//! `RAYON_NUM_THREADS`, else `STAVE_OFFLOAD_JOBS`, else the machine's
//! parallelism); each result lands by its index, so the numbers never
//! depend on the thread count.
//!
//! ```
//! use stave_dsp::circuit::netlist::tolerance::{Tolerance, Tolerances};
//! use stave_dsp::circuit::netlist::Netlist;
//! use stave_measure::design::{sensitivity, Bench, Parts, Quantity};
//!
//! let rc = Netlist::parse_spice("* rc\nV1 in 0 AC 1\nR1 in out 1k\nC1 out 0 100n\n.control\nwrdata o v(out)\n.endc").unwrap();
//! let design = Parts::new(&rc, &Tolerances::new().resistors(Tolerance::uniform(0.01)));
//! let corner = 1.0 / (std::f64::consts::TAU * 1e3 * 100e-9);
//! let s = sensitivity(&design, &[Quantity::ResponseDb { hz: corner }], &Bench::default()).unwrap();
//! // At the corner, a 1 % larger R lowers the response by 0.0434 dB.
//! assert!((s.per_percent("r1", 0).unwrap() + 0.0434).abs() < 1e-3);
//! ```
//!
//! [`Circuit`]: stave_dsp::circuit::Circuit
//! [`Stage`]: stave_dsp::Stage

pub mod fit;
mod series;
mod spread;
mod study;

use std::fmt;

use stave_dsp::circuit::methods::dk::{Dk, Phasor};
use stave_dsp::circuit::netlist::tolerance::{Part, Tolerance, Tolerances};
use stave_dsp::circuit::netlist::{Netlist, Variable};
use stave_dsp::circuit::Circuit;
use stave_dsp::Stage;

pub use fit::{fit, Budget, Fit, Fitting, Free, Point, Target};
pub use series::Series;
pub use spread::{monte_carlo, Advice, Spread};
pub use study::{sensitivity, sweep, Sensitivity, Sweep};

/// A value a design varies: a part, or any parameter of a model.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub nominal: f64,
    /// Its grade, for [`monte_carlo`] (exact by default).
    pub tolerance: Tolerance,
}

impl Parameter {
    pub fn new(name: &str, nominal: f64) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
            nominal,
            tolerance: Tolerance::EXACT,
        }
    }

    pub fn graded(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// What a study measures on each built model.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Quantity {
    /// The small-signal magnitude at `hz`, in dB: a circuit's
    /// linearisation at its operating point, else a sine at the bench's
    /// small amplitude.
    ResponseDb { hz: f64 },
    /// The small-signal phase at `hz`, in degrees.
    PhaseDegrees { hz: f64 },
    /// A sine's gain at `hz` and `amplitude` (volts, or full scale), in dB.
    GainDb { hz: f64, amplitude: f64 },
    /// A sine's THD at `hz` and `amplitude`: harmonics 2… up to the
    /// bench's limit against the fundamental, in dB.
    ThdDb { hz: f64, amplitude: f64 },
    /// Harmonic `harmonic` (2, 3, …) of a sine at `hz` and `amplitude`,
    /// against the output's fundamental, in dB (dBc): one point of a
    /// harmonic-vs-level curve. Harmonics past the bench's limit read NaN.
    HarmonicDb { hz: f64, amplitude: f64, harmonic: u32 },
}

impl Quantity {
    /// A short label: `response 1 kHz`, `THD 1 kHz @ 0.5`.
    pub fn label(&self) -> String {
        // Four significant figures, trailing zeros dropped.
        let figures = |v: f64| {
            let decimals = if v.is_finite() && v != 0.0 {
                (3 - v.abs().log10().floor() as i32).clamp(0, 12) as usize
            } else {
                0
            };
            let s = format!("{v:.decimals$}");
            if s.contains('.') {
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                s
            }
        };
        let f = |hz: f64| {
            if hz >= 1e3 {
                format!("{} kHz", figures(hz / 1e3))
            } else {
                format!("{} Hz", figures(hz))
            }
        };
        match *self {
            Self::ResponseDb { hz } => format!("response {}", f(hz)),
            Self::PhaseDegrees { hz } => format!("phase {}", f(hz)),
            Self::GainDb { hz, amplitude } => format!("gain {} @ {amplitude}", f(hz)),
            Self::ThdDb { hz, amplitude } => format!("THD {} @ {amplitude}", f(hz)),
            Self::HarmonicDb { hz, amplitude, harmonic } => format!("H{harmonic} {} @ {amplitude}", f(hz)),
        }
    }

    /// Its unit: `dB` or `°`.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::PhaseDegrees { .. } => "°",
            _ => "dB",
        }
    }

    fn hz(&self) -> f64 {
        match *self {
            Self::ResponseDb { hz }
            | Self::PhaseDegrees { hz }
            | Self::GainDb { hz, .. }
            | Self::ThdDb { hz, .. }
            | Self::HarmonicDb { hz, .. } => hz,
        }
    }
}

/// The conditions every model is measured under.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bench {
    /// The rate the models run at.
    pub sample_rate: f64,
    /// Samples the sine runs before it is measured (the slowest time
    /// constant's several multiples).
    pub settle_seconds: f64,
    /// The analysis window, in samples: a tone sits on one of its bins
    /// (`sample_rate / window` apart; 10 Hz by default), moved there if it
    /// is not.
    pub window: usize,
    /// The sine a model without a linearisation is measured with for the
    /// small-signal quantities.
    pub small_amplitude: f64,
    /// Harmonics counted up to here.
    pub harmonics_to_hz: f64,
    /// The relative step of a sensitivity's central difference.
    pub step: f64,
    /// The threads a study's evaluations share: the caller's cap (this
    /// crate reads no environment).
    pub threads: usize,
}

impl Bench {
    pub const DEFAULT: Self = Self {
        sample_rate: 96_000.0,
        settle_seconds: 0.2,
        window: 9_600,
        small_amplitude: 1e-4,
        harmonics_to_hz: 20_000.0,
        step: 1e-4,
        threads: 1,
    };

    /// The frequency a tone at `hz` is measured at: the nearest bin.
    pub fn snapped(&self, hz: f64) -> f64 {
        let bin = self.sample_rate / self.window as f64;
        (hz / bin).round().max(1.0) * bin
    }
}

impl Default for Bench {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A model as a study runs it: samples in and out, and its exact
/// small-signal response when it has one.
pub trait Subject {
    fn prepare(&mut self, sample_rate: f64);
    fn reset(&mut self);
    fn tick(&mut self, x: f64) -> f64;
    /// The small-signal response at `hz` (after [`prepare`](Self::prepare)),
    /// if the model can say it exactly; otherwise it is measured.
    fn small_signal(&self, _hz: f64) -> Option<Phasor> {
        None
    }
}

/// A DK circuit: its driven source in, its probe out, and its
/// linearisation at the operating point for the small-signal quantities
/// ([`Dk::analog_response`]).
impl Subject for Circuit<Dk> {
    fn prepare(&mut self, sample_rate: f64) {
        Dk::prepare(self, sample_rate as f32);
    }
    fn reset(&mut self) {
        Dk::reset(self);
    }
    fn tick(&mut self, x: f64) -> f64 {
        self.step(x)
    }
    fn small_signal(&self, hz: f64) -> Option<Phasor> {
        self.analog_response(hz)
    }
}

/// Any [`Stage`] as a subject, measured at f32 like a host runs it.
pub struct Staged<S>(pub S);

impl<S: Stage> Subject for Staged<S> {
    fn prepare(&mut self, sample_rate: f64) {
        self.0.prepare(sample_rate as f32);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn tick(&mut self, x: f64) -> f64 {
        f64::from(self.0.tick(x as f32))
    }
}

/// A closure as a subject: `f64` in, `f64` out, no state to prepare.
pub struct Ticked<F>(pub F);

impl<F: FnMut(f64) -> f64> Subject for Ticked<F> {
    fn prepare(&mut self, _sample_rate: f64) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f64) -> f64 {
        (self.0)(x)
    }
}

/// A model with parameters: what a study varies and how it builds the
/// model from their values.
pub trait Design: Sync {
    type Subject: Subject;
    fn parameters(&self) -> &[Parameter];
    /// The model at `values` (one per parameter, in order).
    fn build(&self, values: &[f64]) -> Result<Self::Subject, Error>;
}

/// Why a study could not run.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A study named a parameter the design does not have.
    UnknownParameter(String),
    /// The model could not be built at some values.
    Build(String),
    /// A study needs something it was not given (units, a grid axis).
    Empty(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownParameter(name) => write!(f, "no parameter {name}"),
            Self::Build(why) => write!(f, "the model could not be built: {why}"),
            Self::Empty(what) => write!(f, "nothing to run: no {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A design from a closure: named parameters and a builder.
pub struct Parameters<F> {
    parameters: Vec<Parameter>,
    build: F,
}

impl<S: Subject, F: Fn(&[f64]) -> S + Sync> Parameters<F> {
    pub fn new(parameters: Vec<Parameter>, build: F) -> Self {
        Self { parameters, build }
    }
}

impl<S: Subject, F: Fn(&[f64]) -> S + Sync> Design for Parameters<F> {
    type Subject = S;
    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
    fn build(&self, values: &[f64]) -> Result<S, Error> {
        Ok((self.build)(values))
    }
}

/// A netlist's design: its parts ([`Tolerances::parts`] — resistors,
/// capacitors, inductors, pots) at their graded tolerances, built as a
/// [`Circuit<Dk>`](Circuit). The small-signal quantities are the
/// netlist's linearisation from its `AC` source (or the driven one) to
/// its probe; the sine quantities need a driven source ([`drive`](Self::drive)).
pub struct Parts {
    netlist: Netlist,
    parts: Vec<Part>,
    /// Pots and rheostats whose rotation is a parameter, after the parts.
    settings: Vec<Variable>,
    parameters: Vec<Parameter>,
    drive: Option<String>,
}

/// A pot's or rheostat's rotation in `netlist`, from its elements'
/// values and its taper.
fn rotation(netlist: &Netlist, variable: &Variable) -> Option<f64> {
    match variable {
        Variable::Pot { legs, taper, end_ohms, .. } => {
            let (a, b) = (netlist.value(&legs[0])?, netlist.value(&legs[1])?);
            let track = a + b - 2.0 * end_ohms;
            Some(taper.position((a - end_ohms) / track))
        }
        Variable::Rheostat { element, track_ohms, taper, end_ohms, .. } => {
            Some(taper.position((netlist.value(element)? - end_ohms) / track_ohms))
        }
        _ => None,
    }
}

/// Turn a pot or rheostat in `netlist` to `position`, its track as the
/// parts left it (`base` is the netlist as read: a rheostat's track
/// scales with its element).
fn turn(netlist: &mut Netlist, base: &Netlist, variable: &Variable, position: f64) -> Result<(), Error> {
    let fail = |e: stave_dsp::circuit::netlist::NetlistError| Error::Build(e.to_string());
    let missing = |name: &str| Error::Build(format!("no element {name}"));
    // A leg never reaches zero ohms (the solver wants a resistance).
    let floor = |ohms: f64, track: f64| ohms.max(track * 1e-9);
    match variable {
        Variable::Pot { legs, taper, end_ohms, .. } => {
            let a = netlist.value(&legs[0]).ok_or_else(|| missing(&legs[0]))?;
            let b = netlist.value(&legs[1]).ok_or_else(|| missing(&legs[1]))?;
            let track = a + b - 2.0 * end_ohms;
            let f = taper.fraction(position);
            netlist.set_value(&legs[0], floor(end_ohms + track * f, track)).map_err(fail)?;
            netlist.set_value(&legs[1], floor(end_ohms + track * (1.0 - f), track)).map_err(fail)
        }
        Variable::Rheostat { element, track_ohms, taper, end_ohms, .. } => {
            let now = netlist.value(element).ok_or_else(|| missing(element))?;
            let was = base.value(element).ok_or_else(|| missing(element))?;
            let track = track_ohms * now / was;
            netlist.set_value(element, floor(end_ohms + track * taper.fraction(position), track)).map_err(fail)
        }
        _ => Err(Error::Build(format!("{} is not a pot or rheostat", variable.name()))),
    }
}

impl Parts {
    pub fn new(netlist: &Netlist, tolerances: &Tolerances) -> Self {
        let parts = tolerances.parts(netlist);
        let parameters = parts
            .iter()
            .map(|p| Parameter::new(&p.name, p.nominal).graded(p.tolerance))
            .collect();
        Self {
            netlist: netlist.clone(),
            parts,
            settings: Vec::new(),
            parameters,
            drive: None,
        }
    }

    /// The pot or rheostat `pot` (a declared [`Variable`]) joins the
    /// parameters as its rotation, 0 to 1 along its taper, named
    /// `<pot>.position`, its nominal the rotation the netlist's values
    /// give. Its track stays a part of its own.
    pub fn setting(mut self, pot: &str) -> Result<Self, Error> {
        let name = pot.to_ascii_lowercase();
        let variable = self
            .netlist
            .variables()
            .iter()
            .find(|v| v.name() == name && matches!(v, Variable::Pot { .. } | Variable::Rheostat { .. }))
            .cloned()
            .ok_or_else(|| Error::UnknownParameter(name.clone()))?;
        let position = rotation(&self.netlist, &variable).ok_or_else(|| Error::Build(format!("{name}: no value")))?;
        self.parameters.push(Parameter::new(&format!("{name}.position"), position));
        self.settings.push(variable);
        Ok(self)
    }

    /// The independent source the sine quantities drive.
    pub fn drive(mut self, source: &str) -> Self {
        self.drive = Some(source.to_string());
        self
    }

    /// Only the named parts vary; the rest stay at their values.
    pub fn only(mut self, names: &[&str]) -> Result<Self, Error> {
        let names: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        if let Some(n) = names.iter().find(|n| !self.parameters.iter().any(|p| p.name == **n)) {
            return Err(Error::UnknownParameter(n.clone()));
        }
        let keep = |name: &str| names.iter().any(|n| n == name);
        self.parameters.retain(|p| keep(&p.name));
        self.parts.retain(|p| keep(&p.name));
        self.settings.retain(|v| keep(&format!("{}.position", v.name())));
        Ok(self)
    }
}

impl Design for Parts {
    type Subject = Circuit<Dk>;
    fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
    fn build(&self, values: &[f64]) -> Result<Circuit<Dk>, Error> {
        let mut netlist = self.netlist.clone();
        for (part, &v) in self.parts.iter().zip(values) {
            part.set(&mut netlist, &self.netlist, v).map_err(|e| Error::Build(e.to_string()))?;
        }
        for (pot, &position) in self.settings.iter().zip(&values[self.parts.len()..]) {
            turn(&mut netlist, &self.netlist, pot, position)?;
        }
        let mut circuit = Circuit::<Dk>::new(&netlist).map_err(|e| Error::Build(e.to_string()))?;
        if let Some(source) = &self.drive {
            circuit.drive(source).map_err(|e| Error::Build(e.to_string()))?;
        }
        Ok(circuit)
    }
}

/// One coherent sine through a prepared subject: the complex amplitude
/// of the fundamental (re the input's) and the harmonics' amplitudes.
fn sine(subject: &mut dyn Subject, hz: f64, amplitude: f64, bench: &Bench) -> (Phasor, Vec<f64>) {
    let rate = bench.sample_rate;
    let hz = bench.snapped(hz);
    let w = std::f64::consts::TAU * hz / rate;
    subject.reset();
    let settle = (bench.settle_seconds * rate).round() as usize;
    let n = bench.window;
    let mut out = Vec::with_capacity(n);
    for i in 0..settle + n {
        let y = subject.tick(amplitude * (w * i as f64).sin());
        if i >= settle {
            out.push(y);
        }
    }
    // The input is sin(w·i): its phasor at bin k is −j·A/2 · n, so the
    // response is the output's bin over the input's, phase from settle.
    let bin = |k: f64| -> Phasor {
        let (mut re, mut im) = (0.0, 0.0);
        for (j, &y) in out.iter().enumerate() {
            let a = k * w * (settle + j) as f64;
            re += y * a.cos();
            im -= y * a.sin();
        }
        Phasor::new(2.0 * re / n as f64, 2.0 * im / n as f64)
    };
    let f = bin(1.0);
    // Divide by the input's phasor, −j·A: (re + j·im)/(−j·A) = (−im + j·re)/A.
    let response = Phasor::new(-f.im / amplitude, f.re / amplitude);
    let harmonics = (2..)
        .take_while(|&k| f64::from(k) * hz <= bench.harmonics_to_hz.min(rate / 2.0))
        .map(|k| bin(f64::from(k)).magnitude())
        .collect();
    (response, harmonics)
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// The quantities measured on one built subject.
pub fn measure(subject: &mut dyn Subject, quantities: &[Quantity], bench: &Bench) -> Vec<f64> {
    subject.prepare(bench.sample_rate);
    quantities
        .iter()
        .map(|q| match *q {
            Quantity::ResponseDb { .. } | Quantity::PhaseDegrees { .. } => {
                let h = subject
                    .small_signal(q.hz())
                    .unwrap_or_else(|| sine(subject, q.hz(), bench.small_amplitude, bench).0);
                match q {
                    Quantity::ResponseDb { .. } => db(h.magnitude()),
                    _ => h.phase().to_degrees(),
                }
            }
            Quantity::GainDb { hz, amplitude } => db(sine(subject, hz, amplitude, bench).0.magnitude()),
            Quantity::ThdDb { hz, amplitude } => {
                let (h, harmonics) = sine(subject, hz, amplitude, bench);
                let distortion = harmonics.iter().map(|a| a * a).sum::<f64>().sqrt();
                db(distortion / (h.magnitude() * amplitude))
            }
            Quantity::HarmonicDb { hz, amplitude, harmonic } => {
                let (h, harmonics) = sine(subject, hz, amplitude, bench);
                let k = harmonic as usize;
                match harmonics.get(k.wrapping_sub(2)) {
                    Some(a) if k >= 2 => db(a / (h.magnitude() * amplitude)),
                    _ => f64::NAN,
                }
            }
        })
        .collect()
}

/// Each value-set measured, in parallel, results in input order.
fn evaluate<D: Design>(design: &D, points: &[Vec<f64>], quantities: &[Quantity], bench: &Bench) -> Result<Vec<Vec<f64>>, Error> {
    let workers = bench.threads.min(points.len()).max(1);
    let run = |i: usize| -> Result<Vec<f64>, Error> {
        let mut subject = design.build(&points[i])?;
        Ok(measure(&mut subject, quantities, bench))
    };
    let mut results: Vec<(usize, Result<Vec<f64>, Error>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                let run = &run;
                scope.spawn(move || (w..points.len()).step_by(workers).map(|i| (i, run(i))).collect::<Vec<_>>())
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("a study thread panicked")).collect()
    });
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, r)| r).collect()
}

/// The index of the parameter named `name`.
fn index_of<D: Design>(design: &D, name: &str) -> Result<usize, Error> {
    let name = name.to_ascii_lowercase();
    design
        .parameters()
        .iter()
        .position(|p| p.name == name)
        .ok_or(Error::UnknownParameter(name))
}

fn nominal<D: Design>(design: &D) -> Vec<f64> {
    design.parameters().iter().map(|p| p.nominal).collect()
}

#[cfg(test)]
mod tests;
