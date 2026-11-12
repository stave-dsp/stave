//! First-order frequency-tunable allpass.
//!
//! Transfer function: H(z) = (g + z^-1) / (1 + g·z^-1), where the
//! coefficient g is set such that the phase response passes through π at
//! the given centre frequency. Phasers cascade these and modulate the
//! centre to create the swept notches.

use crate::primitives::denormal;

#[derive(Clone, Copy, Debug)]
pub struct Allpass1 {
    g: f32,
    z: f32,
    sr: f32,
}

impl Default for Allpass1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Allpass1 {
    pub const fn new() -> Self {
        Self {
            g: 0.0,
            z: 0.0,
            sr: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sr = sample_rate;
        self.z = 0.0;
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }

    /// Set the phase-shift centre frequency (Hz).
    #[inline]
    pub fn set_frequency(&mut self, hz: f32) {
        let pi = core::f32::consts::PI;
        let hz = hz.clamp(1.0, self.sr * 0.499);
        let t = (pi * hz / self.sr).tan();
        self.g = (t - 1.0) / (t + 1.0);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.g * x + self.z;
        self.z = denormal::flush(x - self.g * y);
        y
    }
}
