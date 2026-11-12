//! The interstage coupling cap into its grid leak: a first-order RC
//! high-pass, discretized as the circuit layer discretizes every
//! capacitor (trapezoidal), so it matches ngspice to 6e-7 normalized RMS
//! (stave-test `matches_ngspice`).
use crate::kernel::node::Stage;

/// `y = (x − x₁ + (1 − g)·y₁) / (1 + g)`, g = ωc·T/2: unity gain at
/// Nyquist, zero at DC. Coefficient and state are f64: low corners at 4×
/// rates put the pole within ~2e-4 of unity, where f32 quantizes the
/// corner and leaves a measurable low-frequency error floor (pinned in
/// `stave-test/tests/coupler_conformance.rs`).
pub struct Coupler {
    hz: f32,
    /// ωc·T/2, the trapezoidal companion's half-step conductance ratio.
    g: f64,
    x1: f64,
    y1: f64,
}

impl Coupler {
    pub fn new(hz: f32) -> Self {
        debug_assert!(hz > 0.0, "corner must be positive");
        Self {
            hz,
            g: 0.0,
            x1: 0.0,
            y1: 0.0,
        }
    }
    /// Corner change keeps state (a knob move must not re-charge the
    /// cap); takes effect via the next `prepare` OR immediately if
    /// already prepared at `rate`.
    pub fn set_corner(&mut self, hz: f32, rate: f32) {
        debug_assert!(hz > 0.0, "corner must be positive");
        self.hz = hz;
        if rate > 0.0 {
            self.g = half_step(self.hz, rate);
        }
    }
}

fn half_step(hz: f32, rate: f32) -> f64 {
    core::f64::consts::PI * f64::from(hz) / f64::from(rate)
}

impl Stage for Coupler {
    fn prepare(&mut self, rate: f32) {
        debug_assert!(rate > 0.0, "sample rate must be positive");
        self.g = half_step(self.hz, rate);
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
        self.y1 = crate::primitives::denormal::flush64(out);
        out as f32
    }
}
