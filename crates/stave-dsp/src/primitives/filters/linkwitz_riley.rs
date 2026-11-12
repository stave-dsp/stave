//! Linkwitz-Riley 4th-order crossover filter.
//!
//! Implemented as two cascaded Butterworth 2nd-order biquads (Q=1/√2
//! each). 24 dB/oct rolloff at the crossover frequency. The defining
//! property: summing a matched LR4 lowpass and highpass at the same
//! cutoff produces a magnitude-flat result (with allpass phase) — so a
//! multi-band split + recombine is unity-gain everywhere except for the
//! phase rotation at the crossover.

use crate::primitives::filters::biquad::Biquad;
use crate::FilterSpec;

#[derive(Clone, Copy, Debug)]
pub struct LinkwitzRiley4 {
    a: Biquad,
    b: Biquad,
}

impl Default for LinkwitzRiley4 {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkwitzRiley4 {
    pub const fn new() -> Self {
        Self {
            a: Biquad::new(),
            b: Biquad::new(),
        }
    }

    pub fn set_lowpass(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let q = core::f32::consts::FRAC_1_SQRT_2;
        self.a
            .set(FilterSpec::Lowpass { hz: cutoff_hz, q }, sample_rate);
        self.b
            .set(FilterSpec::Lowpass { hz: cutoff_hz, q }, sample_rate);
    }

    pub fn set_highpass(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let q = core::f32::consts::FRAC_1_SQRT_2;
        self.a
            .set(FilterSpec::Highpass { hz: cutoff_hz, q }, sample_rate);
        self.b
            .set(FilterSpec::Highpass { hz: cutoff_hz, q }, sample_rate);
    }

    pub fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.b.process(self.a.process(x))
    }
}
