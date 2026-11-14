//! First-order frequency-tunable allpass.
//!
//! `H(z) = (g + z⁻¹) / (1 + g·z⁻¹)`, with `g` set so the phase passes
//! through −90° at the centre frequency. Phasers cascade these and
//! modulate the centre to sweep their notches.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::units::Hz;

#[derive(Clone, Copy, Debug)]
pub struct Allpass1 {
    hz: f32,
    sample_rate: f32,
    g: f32,
    z: f32,
}

impl Default for Allpass1 {
    /// Centred at 1 kHz.
    fn default() -> Self {
        Self::new(Hz::new(1_000.0))
    }
}

impl Allpass1 {
    pub fn new(centre: Hz) -> Self {
        let mut ap = Self {
            hz: centre.get(),
            sample_rate: 48_000.0,
            g: 0.0,
            z: 0.0,
        };
        ap.set_frequency(centre);
        ap
    }

    /// Move the phase-shift centre (state kept). Held inside
    /// (1 Hz, 0.499·fs).
    #[inline]
    pub fn set_frequency(&mut self, centre: Hz) {
        let hz = centre.get();
        self.hz = hz;
        let hz = hz.clamp(1.0, self.sample_rate * 0.499);
        let t = (core::f32::consts::PI * hz / self.sample_rate).tan();
        self.g = (t - 1.0) / (t + 1.0);
    }
}

impl Stage for Allpass1 {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_frequency(Hz::new(self.hz));
        self.reset();
    }
    fn reset(&mut self) {
        self.z = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.g * x + self.z;
        self.z = denormal::flush(x - self.g * y);
        y
    }
    fn tail(&self) -> Tail {
        Tail::from_pole(f64::from(self.g)).max(Tail::Samples(1))
    }
}
