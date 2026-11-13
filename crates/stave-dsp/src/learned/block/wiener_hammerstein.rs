//! **Wiener–Hammerstein** — a linear filter, a static curve, a linear
//! filter.

use super::linear::{Linear, Running};
use super::nonlinearity::Nonlinearity;
use super::Rates;
use crate::behavioral::adaa::{Adaa, AdaaOrder};
use crate::behavioral::curve::Curve;
use crate::kernel::node::{Stage, Tail};
use crate::learned::{RateMismatch, RatePolicy};

/// A Wiener–Hammerstein model: `y = G(f(H(x))) + offset`, two linear
/// filters around a static curve. A Wiener model is one with `G = 1`
/// ([`wiener`](Self::wiener)), a Hammerstein model one with `H = 1`
/// ([`hammerstein`](Self::hammerstein)). Cascades are
/// [`Serial`](crate::Serial)s of these.
///
/// # Models
///
/// Whatever it was fitted to: a device whose memory is linear and sits
/// before and after one memoryless nonlinearity — a filter into a
/// clipper into a filter. It cannot hold what is not of that form: a
/// filter whose corner moves with level (a clipper's capacitor across
/// its diodes, an op-amp's feedback diodes shunting its gain), bias that
/// shifts after a burst, or hysteresis. The best linear approximation of
/// a Wiener–Hammerstein system changes with level only in gain
/// (Schoukens & Ljung 2019); measure that before trusting one.
///
/// # Derivation
///
/// Fitted by `stave_measure::identify::wiener_hammerstein`: the best
/// linear approximation from random-phase multisines, a rational fit of
/// it, the poles and zeros split between the two filters by the best of
/// every split, the curve by linear least squares on knots placed where
/// the signal spends its time (Sjöberg, Lauwers & Schoukens 2012's best
/// split of the linear model). Read from a file by
/// [`parse`](super::parse); `tools/stave-fit` writes one.
///
/// # Verified
///
/// Round trip: a known Wiener–Hammerstein system fitted from its
/// measurements recovers both filters to 0.02 dB / 0.8° (budget 0.05 dB
/// / 2°) and the curve to 0.4 % of its range (budget 1 %), output error
/// under −56 dB at every level
/// (`crates/stave-test/tests/block_models.rs::a_known_wiener_hammerstein_system_comes_back`),
/// and from one recording aligned on its loopback
/// (`crates/stave-test/tests/block_models.rs::a_recording_is_aligned_and_fitted`).
/// Fitted to stave's circuits through `Circuit<Dk>` at 4× and
/// characterised against ngspice on their R17 plans at 48 kHz
/// (`crates/stave-test/tests/block_models.rs::fitted_to_the_diode_clipper`,
/// `crates/stave-test/tests/block_models.rs::fitted_to_the_tube_screamer`,
/// `crates/stave-test/tests/block_models.rs::fitted_to_the_common_cathode`):
/// held-out error at −20 / +12 dB re knee −28.6 / −10.9 dB (diode
/// clipper), −31.4 / −11.5 (Tube Screamer), −57.3 / −30.0 (common
/// cathode, the circuit's own at 48 kHz −57.2 / −29.4). The rate change
/// keeps the analog prototype
/// (`crates/stave-dsp/src/learned/block/tests.rs`).
///
/// # Cost
///
/// One multiply-add per coefficient and a spline's four knots (a binary
/// search finds them) or a polynomial's Horner steps: datasheet
/// `"WienerHammerstein (2 + 2 sections, 24-knot spline)"`; with ADAA
/// `"WienerHammerstein (2 + 2 sections, 24-knot spline, ADAA)"`.
///
/// # Aliasing
///
/// The curve's harmonics fold at the host rate unless the stage runs
/// oversampled. Measured on the fitted circuits at 48 kHz (C10, a 1 kHz
/// tone at +12 dB re knee): −70.0 dBc, ANMR −25.7 dB (diode clipper);
/// −80.5 dBc, −25.4 dB (Tube Screamer); at the floor, −26.7 dB (common
/// cathode) — inaudible by Kahles et al.'s −10 dB. First-order ADAA
/// ([`set_antialiasing`](Self::set_antialiasing), off by default) on
/// the curve's table adds half a sample of delay and the kernel's
/// `cos(ω/2)` droop (−6 dB at 16 kHz at 48 kHz) the fit did not see.
///
/// # Valid ranges
///
/// Inputs that keep `H(x)` inside the curve's fitted domain (outside, the
/// curve holds its edge value), at the levels and in the band it was
/// fitted at. Rates: rational filters keep their analog prototype at any
/// rate; FIR filters run only at the fitted rate ([`RatePolicy::Any`]
/// runs them anyway, time scaled). [`reset`](Stage::reset) puts every
/// filter at its steady state under silence, as a circuit starts at its
/// operating point.
#[derive(Clone)]
pub struct WienerHammerstein {
    input: Linear,
    nonlinearity: Nonlinearity,
    output: Linear,
    offset: f64,
    pre: Running,
    post: Running,
    curve: Curve,
    adaa: Adaa,
    adaa_on: bool,
    rates: Rates,
}

impl WienerHammerstein {
    /// `G(f(H(x)))`, fitted at `sample_rate`.
    ///
    /// # Panics
    ///
    /// On an unstable section, a non-finite coefficient or an empty
    /// domain (a file's errors come back from [`parse`](super::parse)
    /// instead).
    pub fn new(sample_rate: f32, input: Linear, nonlinearity: Nonlinearity, output: Linear) -> Self {
        Self::checked(sample_rate, input, nonlinearity, output).unwrap_or_else(|e| panic!("{e}"))
    }

    pub(crate) fn checked(
        sample_rate: f32,
        input: Linear,
        nonlinearity: Nonlinearity,
        output: Linear,
    ) -> Result<Self, String> {
        input.check().map_err(|e| format!("input filter: {e}"))?;
        output.check().map_err(|e| format!("output filter: {e}"))?;
        nonlinearity.check().map_err(|e| format!("nonlinearity: {e}"))?;
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(format!("sample rate {sample_rate}"));
        }
        let curve = nonlinearity.tabulate(0.0);
        let mut model = Self {
            pre: Running::new(&input),
            post: Running::new(&output),
            rates: Rates::new(sample_rate, input.is_fir() || output.is_fir()),
            input,
            nonlinearity,
            output,
            offset: 0.0,
            curve,
            adaa: Adaa::new(AdaaOrder::First),
            adaa_on: false,
        };
        model.reset();
        Ok(model)
    }

    /// A Wiener model: `f(H(x))`.
    pub fn wiener(sample_rate: f32, input: Linear, nonlinearity: Nonlinearity) -> Self {
        Self::new(sample_rate, input, nonlinearity, Linear::identity())
    }

    /// A Hammerstein model: `G(f(x))`.
    pub fn hammerstein(sample_rate: f32, nonlinearity: Nonlinearity, output: Linear) -> Self {
        Self::new(sample_rate, Linear::identity(), nonlinearity, output)
    }

    /// A constant added to the output (a circuit's operating point).
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    pub fn input(&self) -> &Linear {
        &self.input
    }

    pub fn nonlinearity(&self) -> &Nonlinearity {
        &self.nonlinearity
    }

    pub fn output(&self) -> &Linear {
        &self.output
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// The rate the model was fitted at.
    pub fn sample_rate(&self) -> f32 {
        self.rates.fitted
    }

    /// First-order ADAA on the curve (off by default; see *Aliasing*).
    pub fn set_antialiasing(&mut self, on: bool) {
        self.adaa_on = on;
    }

    /// What to do at a rate other than the fitted one (FIR filters only;
    /// rational ones follow any rate).
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        self.rates.set_policy(policy);
    }

    /// The prepared rate, when it differs from the fitted one.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        self.rates.mismatch()
    }

    /// One sample in f64 (a circuit's volts keep their precision).
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        if self.rates.muted {
            return 0.0;
        }
        let u = self.pre.tick(x);
        let v = if self.adaa_on {
            self.adaa.tick(&self.curve.framed(0.0, 1.0, 1.0), u)
        } else {
            self.nonlinearity.at(u)
        };
        self.post.tick(v) + self.offset
    }
}

impl std::fmt::Debug for WienerHammerstein {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WienerHammerstein")
            .field("input", &self.input)
            .field("nonlinearity", &self.nonlinearity)
            .field("output", &self.output)
            .field("offset", &self.offset)
            .field("fitted_rate", &self.rates.fitted)
            .field("adaa", &self.adaa_on)
            .finish()
    }
}

impl Stage for WienerHammerstein {
    fn prepare(&mut self, sample_rate: f32) {
        self.rates.prepare(sample_rate);
        let (fitted, rate) = (f64::from(self.rates.fitted), f64::from(sample_rate));
        self.pre.prepare(fitted, rate);
        self.post.prepare(fitted, rate);
        self.reset();
    }

    /// Back to rest: every filter at its steady state under silence, as
    /// a circuit starts at its operating point (a model whose curve is
    /// not zero at rest would otherwise climb to its DC level through
    /// the output filter's slowest pole).
    fn reset(&mut self) {
        let u = self.pre.settle(0.0);
        self.adaa.reset();
        self.adaa.push(u);
        self.adaa.push(u);
        self.post.settle(self.nonlinearity.at(u));
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        crate::kernel::denormal::normal64(self.tick_f64(f64::from(x)))
    }

    fn tail(&self) -> Tail {
        self.pre.tail().then(self.post.tail())
    }
}
