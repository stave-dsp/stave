//! Slow random processes for drift and ageing (DSP.md H9): a part's
//! temperature wandering about its ambient ([`OrnsteinUhlenbeck`]), a
//! value creeping away from where it was built ([`Wiener`]). Both are
//! exact in distribution at any sample rate (no discretisation error),
//! seeded and reproducible, and allocation-free.
//!
//! ```
//! use stave_dsp::primitives::control::drift::OrnsteinUhlenbeck;
//! use stave_dsp::circuit::devices::thermal::Expo;
//! use stave_dsp::units::seconds;
//! use stave_dsp::Stage;
//!
//! // A VCO core wandering ±0.5 °C about 30 °C over tens of seconds,
//! // uncompensated, two octaves above its reference.
//! let mut celsius = OrnsteinUhlenbeck::new(30.0, 0.5, seconds(20.0), 7);
//! celsius.prepare(1_000.0); // a control rate
//! let expo = Expo::uncompensated(30.0);
//! let ratio = expo.ratio(2.0, celsius.tick_f64());
//! assert!((ratio / 4.0 - 1.0).abs() < 0.02);
//! ```

use crate::kernel::node::{Generator, Stage, Tail};
use crate::primitives::random::SplitMix64;
use crate::units::Seconds;

/// A mean-reverting random process: `dx = −(x − μ)/τ·dt + σ·√(2/τ)·dW`,
/// stationary at `N(μ, σ²)` with autocorrelation `exp(−|Δt|/τ)`.
///
/// # Models
/// Ornstein–Uhlenbeck's process (Uhlenbeck & Ornstein, Phys. Rev. 36,
/// 1930): the temperature of a part about its ambient, a bias that
/// wanders and returns — a Lorentzian spectrum, flat below `1/(2πτ)` and
/// falling 6 dB/octave above.
///
/// # Derivation
/// The exact update over a step `T`: `x' = μ + a·(x − μ) + σ·√(1 − a²)·g`,
/// `a = exp(−T/τ)`, `g` standard normal; the first value is drawn from
/// the stationary law, so the process is stationary from its first
/// sample.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/control/drift/tests.rs::ornstein_uhlenbeck_has_its_variance_and_correlation`
/// (variance within 4 % of `σ²`, autocorrelation within 0.03 of
/// `exp(−Δt/τ)` at `τ/2`, `τ`, `2τ`, at two rates).
///
/// # Cost
/// Datasheet `"OrnsteinUhlenbeck"`.
///
/// # Aliasing
/// None: a control signal, band-limited by its own time constant well
/// below any rate it runs at.
///
/// # Valid ranges
/// `τ` from one sample up; any `σ ≥ 0`.
#[derive(Clone, Debug)]
pub struct OrnsteinUhlenbeck {
    mean: f64,
    sigma: f64,
    tau_s: f64,
    seed: u64,
    rng: SplitMix64,
    a: f64,
    b: f64,
    x: f64,
}

impl OrnsteinUhlenbeck {
    /// About `mean`, standard deviation `sigma`, correlation time
    /// `tau_s` seconds; prepared at 48 kHz.
    pub fn new(mean: f64, sigma: f64, tau: Seconds, seed: u64) -> Self {
        let tau_s = tau.get();
        let mut p = Self {
            mean,
            sigma: sigma.max(0.0),
            tau_s: tau_s.max(1e-9),
            seed,
            rng: SplitMix64::new(seed),
            a: 0.0,
            b: 0.0,
            x: mean,
        };
        p.prepare(48_000.0);
        p
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// The next value.
    #[inline]
    pub fn tick_f64(&mut self) -> f64 {
        let x = self.x;
        self.x = self.mean + self.a * (self.x - self.mean) + self.b * self.rng.next_normal();
        x
    }
}

impl Stage for OrnsteinUhlenbeck {
    fn prepare(&mut self, sample_rate: f32) {
        self.a = (-1.0 / (f64::from(sample_rate) * self.tau_s)).exp();
        self.b = self.sigma * (1.0 - self.a * self.a).sqrt();
        self.reset();
    }

    /// Back to the seed, the first value a stationary draw.
    fn reset(&mut self) {
        self.rng = SplitMix64::new(self.seed);
        self.x = self.mean + self.sigma * self.rng.next_normal();
    }

    fn tick(&mut self, _: f32) -> f32 {
        self.tick_f64() as f32
    }

    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for OrnsteinUhlenbeck {}

/// Brownian motion with drift: `x(t) = x₀ + μ·t + σ·W(t)`, its mean
/// moving at `μ` per second and its spread growing as `σ·√t`.
///
/// # Models
/// The Wiener process with drift: a value ageing away from where it
/// started — a component's slow, random creep (a resistor's drift, an
/// electrolytic's loss), each unit its own path from its seed.
///
/// # Derivation
/// Independent Gaussian increments, exact at any step `T`:
/// `x' = x + μ·T + σ·√T·g`.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/control/drift/tests.rs::wiener_spreads_as_the_root_of_time`
/// (over 4000 seeded paths the mean within 3 standard errors of `μ·t`
/// and the variance within 6 % of `σ²·t`, at 1 s and 4 s).
///
/// # Cost
/// Datasheet `"Wiener"`.
///
/// # Aliasing
/// None: a control signal.
///
/// # Valid ranges
/// Any `μ`, `σ ≥ 0`; it does not return, so bound what it drives.
#[derive(Clone, Debug)]
pub struct Wiener {
    start: f64,
    drift: f64,
    diffusion: f64,
    seed: u64,
    rng: SplitMix64,
    step_drift: f64,
    step_spread: f64,
    x: f64,
}

impl Wiener {
    /// From `start`, `drift` per second, `diffusion` per √second;
    /// prepared at 48 kHz.
    pub fn new(start: f64, drift: f64, diffusion: f64, seed: u64) -> Self {
        let mut w = Self {
            start,
            drift,
            diffusion: diffusion.max(0.0),
            seed,
            rng: SplitMix64::new(seed),
            step_drift: 0.0,
            step_spread: 0.0,
            x: start,
        };
        w.prepare(48_000.0);
        w
    }

    /// The next value.
    #[inline]
    pub fn tick_f64(&mut self) -> f64 {
        let x = self.x;
        self.x += self.step_drift + self.step_spread * self.rng.next_normal();
        x
    }
}

impl Stage for Wiener {
    fn prepare(&mut self, sample_rate: f32) {
        let t = 1.0 / f64::from(sample_rate);
        self.step_drift = self.drift * t;
        self.step_spread = self.diffusion * t.sqrt();
        self.reset();
    }

    fn reset(&mut self) {
        self.rng = SplitMix64::new(self.seed);
        self.x = self.start;
    }

    fn tick(&mut self, _: f32) -> f32 {
        self.tick_f64() as f32
    }

    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for Wiener {}

#[cfg(test)]
mod tests;
