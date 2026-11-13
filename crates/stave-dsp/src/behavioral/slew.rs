//! Slew — rate-domain nonlinearity (pushed-speaker/OTA grit).
use crate::kernel::node::{Stage, Tail};

/// Slew limiter: output can rise/fall at most so many full-scale units per
/// millisecond. Distortion that depends on FREQUENCY at equal amplitude —
/// the pushed-speaker/OTA character no memoryless curve can produce.
///
/// # Models
///
/// A rate limit, `|dy/dt| ≤ rise` (or `fall`), with no hardware claim:
/// the shape of an amplifier's slew-rate limit, not any particular one.
///
/// # Derivation
///
/// The definition, per sample: the step toward the input is clamped to
/// `rate · 1000 / sample_rate` full-scale units.
///
/// # Verified
///
/// `crates/stave-dsp/src/behavioral/slew/tests.rs::steps_ramp_at_the_set_rates_and_slow_signals_pass`:
/// steps ramp at exactly the set rise and fall rates at 44.1, 48 and
/// 96 kHz, and a sine under both limits passes, within 1e-4.
///
/// # Cost
///
/// Datasheet `"Slew"`.
///
/// # Aliasing
///
/// None added below the limit; a limited edge has slope discontinuities
/// whose partials fall at 12 dB/octave. Run hard limiting oversampled.
///
/// # Valid ranges
///
/// Rates > 0 full-scale units per millisecond (debug-asserted); any
/// sample rate. Rates above `sample_rate / 1000` per ms never engage.
#[derive(Clone)]
pub struct Slew {
    rise_per_ms: f32,
    fall_per_ms: f32,
    sample_rate: f32,
    max_rise: f32,
    max_fall: f32,
    y: f32,
}

impl Slew {
    pub fn new(rise_per_ms: f32, fall_per_ms: f32) -> Self {
        debug_assert!(
            rise_per_ms > 0.0 && fall_per_ms > 0.0,
            "slew rates must be positive"
        );
        let mut s = Self {
            rise_per_ms,
            fall_per_ms,
            sample_rate: 48_000.0,
            max_rise: 1.0,
            max_fall: 1.0,
            y: 0.0,
        };
        s.set_rates(rise_per_ms, fall_per_ms);
        s
    }
    /// Full-scale units per millisecond, each direction (state kept).
    pub fn set_rates(&mut self, rise_per_ms: f32, fall_per_ms: f32) {
        debug_assert!(
            rise_per_ms > 0.0 && fall_per_ms > 0.0,
            "slew rates must be positive"
        );
        self.rise_per_ms = rise_per_ms;
        self.fall_per_ms = fall_per_ms;
        self.max_rise = rise_per_ms * 1000.0 / self.sample_rate;
        self.max_fall = fall_per_ms * 1000.0 / self.sample_rate;
    }
}

impl Stage for Slew {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_rates(self.rise_per_ms, self.fall_per_ms);
        self.y = 0.0;
    }
    fn reset(&mut self) {
        self.y = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let d = (x - self.y).clamp(-self.max_fall, self.max_rise);
        self.y += d;
        self.y
    }
    /// The slower direction's walk back from full scale.
    fn tail(&self) -> Tail {
        Tail::samples((1.0 / self.max_rise.min(self.max_fall)).ceil() as u32)
    }
}

#[cfg(test)]
mod tests;
