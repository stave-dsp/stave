//! The interstage coupling cap into its grid leak: a first-order RC
//! high-pass, discretized as the circuit layer discretizes every
//! capacitor (trapezoidal), so it matches ngspice to 6e-7 normalized RMS
//! (stave-test `matches_ngspice`).
use crate::kernel::node::{Stage, Tail};

/// `y = (x − x₁ + (1 − g)·y₁) / (1 + g)`, g = ωc·T/2: unity gain at
/// Nyquist, zero at DC. Coefficient and state are f64: low corners at 4×
/// rates put the pole within ~2e-4 of unity, where f32 quantizes the
/// corner and leaves a measurable low-frequency error floor (pinned in
/// `stave-test/tests/coupler_conformance.rs`).
///
/// # Models
/// The interstage coupling capacitor into its grid leak: a first-order
/// RC high-pass.
///
/// # Derivation
/// The RC network, its capacitor discretized as every circuit capacitor
/// is (trapezoidal), in f64.
///
/// # Verified
/// `crates/stave-test/tests/matches_ngspice.rs::coupler_matches_ngspice`:
/// 2e-6 normalized RMS against ngspice at 48 kHz;
/// `crates/stave-test/tests/coupler_conformance.rs::corner_is_accurate_at_4x_rate`.
///
/// # Cost
/// No datasheet line yet (DSP.md P1): one f64 first-order section per
/// sample.
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// Corner and rate positive (debug-asserted), the corner below Nyquist.
#[derive(Clone)]
pub struct Coupler {
    hz: f32,
    sample_rate: f32,
    /// ωc·T/2, the trapezoidal companion's half-step conductance ratio.
    g: f64,
    x1: f64,
    y1: f64,
}

impl Coupler {
    pub fn new(hz: f32) -> Self {
        debug_assert!(hz > 0.0, "corner must be positive");
        let mut c = Self {
            hz,
            sample_rate: 48_000.0,
            g: 0.0,
            x1: 0.0,
            y1: 0.0,
        };
        c.set_hz(hz);
        c
    }
    /// Corner change keeps state (a knob move must not re-charge the
    /// cap).
    pub fn set_hz(&mut self, hz: f32) {
        debug_assert!(hz > 0.0, "corner must be positive");
        self.hz = hz;
        self.g = core::f64::consts::PI * f64::from(hz) / f64::from(self.sample_rate);
    }
}

impl Stage for Coupler {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let out = (f64::from(x) - self.x1 + (1.0 - self.g) * self.y1) / (1.0 + self.g);
        self.x1 = f64::from(x);
        self.y1 = crate::kernel::denormal::flush64(out);
        out as f32
    }
    /// The pole at `(1 − g)/(1 + g)`.
    fn tail(&self) -> Tail {
        Tail::from_pole((1.0 - self.g) / (1.0 + self.g))
    }
}
