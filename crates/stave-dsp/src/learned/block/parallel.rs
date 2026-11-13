//! **Parallel Hammerstein** — powers of the input, each through its own
//! filter, summed (Novak's generalised polynomial Hammerstein model).

use super::linear::{Linear, Running};
use super::Rates;
use crate::kernel::node::{Stage, Tail};
use crate::learned::{RateMismatch, RatePolicy};

/// `y = offset + Σₘ Gₘ(xᵐ)`, `m = 1 … M`: one Hammerstein branch per
/// power of the input, summed (the generalised polynomial Hammerstein
/// model; Novak, Simon, Kadlec & Lotton 2010).
///
/// # Models
///
/// Whatever it was fitted to, as a weak nonlinearity whose every order
/// has its own frequency response: H2 through H_M in magnitude and phase
/// at every frequency, at the level it was measured at. It is exact for
/// a polynomial Hammerstein system and close for a mildly driven
/// Wiener–Hammerstein one; a power series fitted at one level does not
/// extrapolate to another (Novak et al. 2010 classify a device by how
/// much its kernels move with level), and past the fitted level the
/// highest power runs away.
///
/// # Derivation
///
/// Fitted by `stave_measure::identify::hammerstein` from one
/// synchronized exponential sweep (Novak, Lotton & Simon 2015):
/// deconvolution separates one response per harmonic, and the
/// triangular relation between harmonic responses and branch filters
/// (from the expansion of `sinᵐ`) gives each branch; each is then a
/// rational fit (sections) or its taps (FIR).
///
/// # Verified
///
/// Round trip: a polynomial Hammerstein system with a different filter
/// on each of three powers, measured through an anti-aliased interface
/// (run at 4×), comes back from one 2 s sweep: each kernel within 0.3 %
/// of its peak from 100 Hz to 18 kHz (budget 0.3 %), its rational fit
/// within 1.2 % to 16 kHz (budget 4 %), held-out noise −48 dB
/// (`crates/stave-test/tests/block_models.rs::a_known_polynomial_hammerstein_system_comes_back`).
/// Fitted at the knee of stave's circuits and characterised against
/// ngspice (`crates/stave-test/tests/block_models.rs::fitted_to_the_diode_clipper`,
/// `crates/stave-test/tests/block_models.rs::fitted_to_the_tube_screamer`,
/// `crates/stave-test/tests/block_models.rs::fitted_to_the_common_cathode`):
/// held-out error at −20 / +12 dB re knee −23.8 / −5.2 dB, −18.5 / −3.4
/// and −41.2 / −2.7 — a power series fitted at one level holds near it
/// and not at others.
///
/// # Cost
///
/// `M` powers and every branch's filter: datasheet
/// `"ParallelHammerstein (order 5, 2 sections a branch)"`,
/// `"ParallelHammerstein (order 5, 256 taps a branch)"`.
///
/// # Aliasing
///
/// None added: power `m` puts a tone's harmonics up to `m·f`, which fold
/// above Nyquist unless the stage runs oversampled (an `Oversampled`
/// region at `M`× keeps every product below the original Nyquist).
/// Measured at 48 kHz on the circuits' fits (C10, 1 kHz at +12 dB re
/// knee): −48.1 dBc, ANMR −4.2 dB (diode clipper), −49.0 / −6.0 (Tube
/// Screamer), −38.6 / +12.7 (common cathode): audible at the host rate,
/// past the level it was fitted at.
///
/// # Valid ranges
///
/// Inputs near the level of the sweep it was fitted from; beyond the
/// sweep's peak the input is clamped to it ([`with_limit`](Self::with_limit)).
/// Rates: rational branches keep their analog prototype at any rate; FIR
/// branches run only at the fitted rate ([`RatePolicy::Any`] runs them
/// anyway).
#[derive(Clone, Debug)]
pub struct ParallelHammerstein {
    branches: Vec<Linear>,
    offset: f64,
    limit: f64,
    running: Vec<Running>,
    rates: Rates,
}

impl ParallelHammerstein {
    /// Branch `m − 1` filters `xᵐ`, fitted at `sample_rate`.
    ///
    /// # Panics
    ///
    /// With no branches, an unstable section or a non-finite coefficient.
    pub fn new(sample_rate: f32, branches: Vec<Linear>) -> Self {
        Self::checked(sample_rate, branches).unwrap_or_else(|e| panic!("{e}"))
    }

    pub(crate) fn checked(sample_rate: f32, branches: Vec<Linear>) -> Result<Self, String> {
        if branches.is_empty() {
            return Err("no branches".into());
        }
        for (m, b) in branches.iter().enumerate() {
            b.check().map_err(|e| format!("branch {}: {e}", m + 1))?;
        }
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(format!("sample rate {sample_rate}"));
        }
        Ok(Self {
            running: branches.iter().map(Running::new).collect(),
            rates: Rates::new(sample_rate, branches.iter().any(Linear::is_fir)),
            branches,
            offset: 0.0,
            limit: f64::INFINITY,
        })
    }

    /// A constant added to the output.
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    /// Inputs beyond `±limit` are clamped to it (a fit sets the sweep's
    /// peak: a power series says nothing past the level it was fitted
    /// at, and its highest power runs away there).
    pub fn with_limit(mut self, limit: f64) -> Self {
        self.limit = if limit.is_nan() { f64::INFINITY } else { limit.abs() };
        self
    }

    /// The input limit (infinite unless set).
    pub fn limit(&self) -> f64 {
        self.limit
    }

    /// The branches, `x¹` first.
    pub fn branches(&self) -> &[Linear] {
        &self.branches
    }

    /// The highest power.
    pub fn order(&self) -> usize {
        self.branches.len()
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// The rate the model was fitted at.
    pub fn sample_rate(&self) -> f32 {
        self.rates.fitted
    }

    /// What to do at a rate other than the fitted one (FIR branches
    /// only; rational ones follow any rate).
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        self.rates.set_policy(policy);
    }

    /// The prepared rate, when it differs from the fitted one.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        self.rates.mismatch()
    }

    /// One sample in f64.
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        if self.rates.muted {
            return 0.0;
        }
        let x = x.clamp(-self.limit, self.limit);
        let mut power = 1.0;
        let mut y = self.offset;
        for branch in &mut self.running {
            power *= x;
            y += branch.tick(power);
        }
        y
    }
}

impl Stage for ParallelHammerstein {
    fn prepare(&mut self, sample_rate: f32) {
        self.rates.prepare(sample_rate);
        let (fitted, rate) = (f64::from(self.rates.fitted), f64::from(sample_rate));
        self.running.iter_mut().for_each(|r| r.prepare(fitted, rate));
    }

    fn reset(&mut self) {
        self.running.iter_mut().for_each(Running::reset);
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        crate::kernel::denormal::normal64(self.tick_f64(f64::from(x)))
    }

    fn tail(&self) -> Tail {
        self.running.iter().fold(Tail::None, |t, r| t.max(r.tail()))
    }
}
