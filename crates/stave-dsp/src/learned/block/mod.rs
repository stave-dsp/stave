//! Block-oriented models (DSP.md M3): linear filters and static curves
//! in a fixed arrangement, fitted to a device's input and output.
//!
//! * [`WienerHammerstein`] — `G(f(H(x)))`, and its two halves, the
//!   Wiener (`f(H(x))`) and Hammerstein (`G(f(x))`) models;
//! * [`ParallelHammerstein`] — `Σ Gₘ(xᵐ)`, one filter per power
//!   (Novak's generalised polynomial Hammerstein model);
//! * cascades are [`Serial`](crate::Serial)s of these.
//!
//! The linear blocks are [`Linear`]: cascaded second-order sections or
//! FIR taps; the curve is a [`Nonlinearity`]: a polynomial, or a cubic
//! spline through knots placed where the fitting signal spent its time,
//! held at its edge values outside them.
//!
//! A block model is small (tens of numbers), cheap (a few biquads and a
//! curve: tens of ns a sample against the circuits' hundreds to
//! thousands) and readable: its filters are responses you can plot and
//! its curve a transfer you can see. What it cannot hold is memory inside
//! the nonlinearity — a corner that moves with level, bias that shifts
//! after a burst — which is where a circuit model (or a network) is
//! needed. Fitted to stave's own circuits
//! (`crates/stave-test/tests/block_models.rs`): the common-cathode triode
//! is a Wiener–Hammerstein system to within 0.03 dB of BLA shape and the
//! fit matches the circuit level for level; the diode clipper and the
//! Tube Screamer's gain stage move their corners with drive, and the fit
//! falls from about −30 dB of error below the knee to −11 dB at +12 dB
//! re knee.
//!
//! Fitting lives in `stave_measure::identify`, the capture workflow in
//! `tools/stave-fit` (DSP.md M4); [`parse`] reads the file it writes.
//!
//! ```
//! use stave_dsp::learned::block::{self, Linear, Nonlinearity, Section, WienerHammerstein};
//! use stave_dsp::Stage;
//!
//! // A one-pole low-pass into a cubic soft clipper.
//! let lowpass = Linear::Rational(vec![Section { b: [0.5, 0.0, 0.0], a: [-0.5, 0.0] }]);
//! let cubic = Nonlinearity::Polynomial { coefficients: vec![0.0, 1.5, 0.0, -0.5], low: -1.0, high: 1.0 };
//! let mut model = WienerHammerstein::wiener(48_000.0, lowpass, cubic);
//! model.prepare(48_000.0);
//! assert!((model.tick(1.0) - 0.6875).abs() < 1e-6); // u = 0.5: 0.75 − 0.0625
//!
//! let text = block::Block::from(model.clone()).to_json("a cubic after a one-pole");
//! let back = block::parse(&text)?;
//! assert_eq!(back.note, "a cubic after a one-pole");
//! # Ok::<(), stave_dsp::learned::LoadError>(())
//! ```

mod linear;
mod nonlinearity;
mod parallel;
mod wiener_hammerstein;

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

pub use linear::{Linear, Section};
pub use nonlinearity::{spline_weights, Nonlinearity};
pub use parallel::ParallelHammerstein;
pub use wiener_hammerstein::WienerHammerstein;

use super::json::Json;
use super::{LoadError, RateMismatch, RatePolicy};
use crate::kernel::node::{Stage, Tail};

/// The file format's version this reader writes and reads.
pub const VERSION: u32 = 1;

/// A block model's fitted rate against its prepared one. Rational
/// filters follow any rate; a model with an FIR block is silent at a
/// foreign rate unless the policy is [`RatePolicy::Any`]
/// ([`RatePolicy::Interpolated`] is strict here: there is no state to
/// interpolate).
#[derive(Copy, Clone, Debug)]
pub(crate) struct Rates {
    pub(crate) fitted: f32,
    prepared: Option<f32>,
    fir: bool,
    policy: RatePolicy,
    pub(crate) muted: bool,
}

impl Rates {
    pub(crate) fn new(fitted: f32, fir: bool) -> Self {
        Self {
            fitted,
            prepared: None,
            fir,
            policy: RatePolicy::Strict,
            muted: false,
        }
    }

    pub(crate) fn prepare(&mut self, rate: f32) {
        self.prepared = Some(rate);
        self.update();
    }

    pub(crate) fn set_policy(&mut self, policy: RatePolicy) {
        self.policy = policy;
        self.update();
    }

    fn update(&mut self) {
        self.muted = self.fir && self.mismatch().is_some() && self.policy != RatePolicy::Any;
    }

    pub(crate) fn mismatch(&self) -> Option<RateMismatch> {
        self.prepared.filter(|&p| p != self.fitted).map(|prepared| RateMismatch {
            trained: self.fitted,
            prepared,
        })
    }
}

/// A block model read from a file: either structure, as one stage.
///
/// # Models
///
/// What the file's model was fitted to: a [`WienerHammerstein`] or a
/// [`ParallelHammerstein`] (each catalogued with its own claims).
///
/// # Derivation
///
/// `tools/stave-fit` (or `stave_measure::identify`) fits it and writes the
/// file, [`Block::to_json`]; [`parse`] reads it, refusing an unstable
/// section, a non-finite number or a malformed field by name.
///
/// # Verified
///
/// Every number reads back exactly and the read model runs bit for bit
/// as the written one (`crates/stave-dsp/src/learned/block/tests.rs`);
/// the models themselves in `crates/stave-test/tests/block_models.rs`.
///
/// # Cost
///
/// The model it holds plus one match a sample: datasheet
/// `"WienerHammerstein (2 + 2 sections, 24-knot spline)"`,
/// `"ParallelHammerstein (order 5, 2 sections a branch)"`.
///
/// # Aliasing
///
/// The model's own: see [`WienerHammerstein`], [`ParallelHammerstein`].
///
/// # Valid ranges
///
/// The model's own; the file format is version 1 ([`VERSION`]).
#[derive(Clone, Debug)]
pub enum Block {
    WienerHammerstein(Box<WienerHammerstein>),
    ParallelHammerstein(ParallelHammerstein),
}

impl From<WienerHammerstein> for Block {
    fn from(m: WienerHammerstein) -> Self {
        Block::WienerHammerstein(Box::new(m))
    }
}

impl From<ParallelHammerstein> for Block {
    fn from(m: ParallelHammerstein) -> Self {
        Block::ParallelHammerstein(m)
    }
}

/// A block model file read: the model and its note (what it was fitted
/// to, and how well).
#[derive(Clone, Debug)]
pub struct File {
    pub block: Block,
    pub note: String,
}

impl Block {
    /// The rate the model was fitted at.
    pub fn sample_rate(&self) -> f32 {
        match self {
            Block::WienerHammerstein(m) => m.sample_rate(),
            Block::ParallelHammerstein(m) => m.sample_rate(),
        }
    }

    /// One sample in f64.
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        match self {
            Block::WienerHammerstein(m) => m.tick_f64(x),
            Block::ParallelHammerstein(m) => m.tick_f64(x),
        }
    }

    /// The model as a file [`parse`] reads back exactly (every number
    /// written in its shortest round-trip form), with `note` beside it.
    pub fn to_json(&self, note: &str) -> String {
        let mut s = String::new();
        let _ = write!(s, "{{\"format\": \"stave-block\", \"version\": {VERSION}, ");
        let _ = write!(s, "\"sample_rate\": {}, \"note\": {}, ", number(f64::from(self.sample_rate())), string(note));
        match self {
            Block::WienerHammerstein(m) => {
                let _ = write!(
                    s,
                    "\"architecture\": \"WienerHammerstein\",\n \"input\": {},\n \"nonlinearity\": {},\n \"output\": {},\n \"offset\": {}}}\n",
                    linear(m.input()),
                    curve(m.nonlinearity()),
                    linear(m.output()),
                    number(m.offset())
                );
            }
            Block::ParallelHammerstein(m) => {
                let branches: Vec<String> = m.branches().iter().map(linear).collect();
                let _ = write!(
                    s,
                    "\"architecture\": \"ParallelHammerstein\",\n \"branches\": [\n  {}],\n \"offset\": {}{}}}\n",
                    branches.join(",\n  "),
                    number(m.offset()),
                    if m.limit().is_finite() { format!(", \"limit\": {}", number(m.limit())) } else { String::new() }
                );
            }
        }
        s
    }
}

impl Stage for Block {
    fn prepare(&mut self, sample_rate: f32) {
        match self {
            Block::WienerHammerstein(m) => m.prepare(sample_rate),
            Block::ParallelHammerstein(m) => m.prepare(sample_rate),
        }
    }

    fn reset(&mut self) {
        match self {
            Block::WienerHammerstein(m) => m.reset(),
            Block::ParallelHammerstein(m) => m.reset(),
        }
    }

    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        match self {
            Block::WienerHammerstein(m) => m.tail(),
            Block::ParallelHammerstein(m) => m.tail(),
        }
    }
}

fn number(v: f64) -> String {
    // `{:?}` is the shortest representation that reads back exactly; it
    // is valid JSON for every finite value.
    format!("{v:?}")
}

fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn numbers(v: &[f64]) -> String {
    let items: Vec<String> = v.iter().map(|&x| number(x)).collect();
    format!("[{}]", items.join(", "))
}

fn linear(l: &Linear) -> String {
    match l {
        Linear::Rational(sections) => {
            let items: Vec<String> = sections
                .iter()
                .map(|s| numbers(&[s.b[0], s.b[1], s.b[2], s.a[0], s.a[1]]))
                .collect();
            format!("{{\"sections\": [{}]}}", items.join(", "))
        }
        Linear::Fir(taps) => format!("{{\"fir\": {}}}", numbers(taps)),
    }
}

fn curve(n: &Nonlinearity) -> String {
    match n {
        Nonlinearity::Polynomial { coefficients, low, high } => format!(
            "{{\"polynomial\": {}, \"domain\": {}}}",
            numbers(coefficients),
            numbers(&[*low, *high])
        ),
        Nonlinearity::Spline { knots, values } => {
            format!("{{\"knots\": {},\n  \"spline\": {}}}", numbers(knots), numbers(values))
        }
    }
}

/// Read a block model file (`tools/stave-fit`'s output, or
/// [`Block::to_json`]'s). Every error names what was wrong.
pub fn parse(text: &str) -> Result<File, LoadError> {
    let json = Json::parse(text).map_err(|e| LoadError::Json {
        offset: e.offset,
        reason: e.reason,
    })?;
    let config = |what: &str| LoadError::Config(what.to_string());
    if json.get("format").and_then(Json::as_str) != Some("stave-block") {
        return Err(config("not a stave block model (\"format\": \"stave-block\")"));
    }
    let version = json.get("version").and_then(Json::as_f64).ok_or_else(|| config("no version"))?;
    if version != f64::from(VERSION) {
        return Err(config(&format!("block file version {version}; this reader reads {VERSION}")));
    }
    let rate = json
        .get("sample_rate")
        .and_then(Json::as_f64)
        .filter(|r| *r > 0.0 && r.is_finite())
        .ok_or_else(|| config("no sample_rate"))? as f32;
    let note = json.field("note").and_then(Json::as_str).unwrap_or("").to_string();
    let offset = json.field("offset").map_or(Some(0.0), Json::as_f64).ok_or_else(|| config("offset is not a number"))?;
    let block = match json.get("architecture").and_then(Json::as_str) {
        Some("WienerHammerstein") => {
            let input = read_linear(json.get("input"), "input")?;
            let output = read_linear(json.get("output"), "output")?;
            let nonlinearity = read_curve(json.get("nonlinearity"))?;
            let m = WienerHammerstein::checked(rate, input, nonlinearity, output).map_err(LoadError::Config)?;
            Block::WienerHammerstein(Box::new(m.with_offset(offset)))
        }
        Some("ParallelHammerstein") => {
            let items = json
                .get("branches")
                .and_then(Json::as_array)
                .ok_or_else(|| config("branches is not an array"))?;
            let branches = items
                .iter()
                .enumerate()
                .map(|(i, b)| read_linear(Some(b), &format!("branches[{i}]")))
                .collect::<Result<Vec<_>, _>>()?;
            let limit = json
                .field("limit")
                .map_or(Some(f64::INFINITY), Json::as_f64)
                .filter(|l| *l > 0.0)
                .ok_or_else(|| config("limit is not a positive number"))?;
            let m = ParallelHammerstein::checked(rate, branches).map_err(LoadError::Config)?;
            Block::ParallelHammerstein(m.with_offset(offset).with_limit(limit))
        }
        Some(other) => return Err(LoadError::Architecture(other.to_string())),
        None => return Err(config("no architecture")),
    };
    Ok(File { block, note })
}

fn floats(v: Option<&Json>, what: &str) -> Result<Vec<f64>, LoadError> {
    let items = v
        .and_then(Json::as_array)
        .ok_or_else(|| LoadError::Config(format!("{what} is not an array")))?;
    items
        .iter()
        .map(|x| x.as_f64().ok_or_else(|| LoadError::Config(format!("{what} holds a non-number"))))
        .collect()
}

fn read_linear(v: Option<&Json>, what: &str) -> Result<Linear, LoadError> {
    let v = v.ok_or_else(|| LoadError::Config(format!("no {what}")))?;
    if let Some(sections) = v.get("sections") {
        let items = sections
            .as_array()
            .ok_or_else(|| LoadError::Config(format!("{what}.sections is not an array")))?;
        let sections = items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let c = floats(Some(s), &format!("{what}.sections[{i}]"))?;
                match c[..] {
                    [b0, b1, b2, a1, a2] => Ok(Section { b: [b0, b1, b2], a: [a1, a2] }),
                    _ => Err(LoadError::Config(format!("{what}.sections[{i}] is not [b0, b1, b2, a1, a2]"))),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Linear::Rational(sections))
    } else if let Some(taps) = v.get("fir") {
        Ok(Linear::Fir(floats(Some(taps), &format!("{what}.fir"))?))
    } else {
        Err(LoadError::Config(format!("{what} has neither sections nor fir")))
    }
}

fn read_curve(v: Option<&Json>) -> Result<Nonlinearity, LoadError> {
    let v = v.ok_or_else(|| LoadError::Config("no nonlinearity".into()))?;
    if let Some(c) = v.get("polynomial") {
        let domain = floats(v.get("domain"), "nonlinearity.domain")?;
        let [low, high] = domain[..] else {
            return Err(LoadError::Config("nonlinearity.domain is not [low, high]".into()));
        };
        Ok(Nonlinearity::Polynomial {
            coefficients: floats(Some(c), "nonlinearity.polynomial")?,
            low,
            high,
        })
    } else if let Some(c) = v.get("spline") {
        Ok(Nonlinearity::Spline {
            knots: floats(v.get("knots"), "nonlinearity.knots")?,
            values: floats(Some(c), "nonlinearity.spline")?,
        })
    } else {
        Err(LoadError::Config("nonlinearity is neither polynomial nor spline".into()))
    }
}
