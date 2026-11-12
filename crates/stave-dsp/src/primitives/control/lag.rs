//! `Lag` — a SIGNED asymmetric one-pole: rises toward the input at
//! one rate, falls at another. This is the analog-character
//! primitive for control signals: a triangle through a symmetric
//! lag is the Juno chorus LFO (RC-rounded corners); a sine through
//! a fast-rise/slow-fall lag is the Univibe's lamp throb (the bulb
//! brightens fast, dims slow). It is also portamento, when a synth
//! earns one. Unlike an envelope follower it does NOT rectify —
//! bipolar signals pass with their sign.

#[derive(Clone, Copy, Debug, Default)]
pub struct Lag {
    rise: f32,
    fall: f32,
    rise_ms: f32,
    fall_ms: f32,
    y: f32,
}

#[inline]
fn coeff(ms: f32, sr: f32) -> f32 {
    if ms <= 0.0 {
        return 1.0; // instantaneous
    }
    debug_assert!(sr > 0.0, "sample rate must be positive");
    1.0 - (-1.0 / (ms * 0.001 * sr)).exp()
}

impl Lag {
    pub fn new(rise_ms: f32, fall_ms: f32) -> Self {
        Self {
            rise: 1.0,
            fall: 1.0,
            rise_ms,
            fall_ms,
            y: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.rise = coeff(self.rise_ms, sample_rate);
        self.fall = coeff(self.fall_ms, sample_rate);
        self.y = 0.0;
    }

    pub fn set_times(&mut self, rise_ms: f32, fall_ms: f32, sample_rate: f32) {
        self.rise_ms = rise_ms;
        self.fall_ms = fall_ms;
        self.rise = coeff(rise_ms, sample_rate);
        self.fall = coeff(fall_ms, sample_rate);
    }

    pub fn reset(&mut self) {
        self.y = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let c = if x > self.y { self.rise } else { self.fall };
        self.y = crate::primitives::denormal::flush(self.y + (x - self.y) * c);
        self.y
    }

    pub fn state(&self) -> f32 {
        self.y
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// τ measured both directions matches the labels — the asymmetry
    /// IS the primitive (the Univibe lamp: bright fast, dim slow).
    #[test]
    fn rise_and_fall_are_independent_taus() {
        let sr = 48_000.0;
        let mut lag = Lag::new(1.0, 100.0);
        lag.prepare(sr);
        // Rise: step 0 → 1; 63% within ~1 ms.
        let mut n = 0;
        while lag.state() < 0.632 {
            lag.tick(1.0);
            n += 1;
            assert!(n < 1000, "rise never settled");
        }
        let rise_ms = n as f32 / sr * 1000.0;
        assert!((rise_ms - 1.0).abs() < 0.3, "rise τ {rise_ms} ms (label 1)");
        // Saturate, then fall 1 → 0; 63% down within ~100 ms.
        for _ in 0..(sr as usize / 10) {
            lag.tick(1.0);
        }
        let top = lag.state();
        let mut n = 0;
        while lag.state() > top * (1.0 - 0.632) {
            lag.tick(0.0);
            n += 1;
            assert!(n < 100_000, "fall never settled");
        }
        let fall_ms = n as f32 / sr * 1000.0;
        assert!(
            (fall_ms - 100.0).abs() < 10.0,
            "fall τ {fall_ms} ms (label 100)"
        );
    }

    /// Bipolar signals keep their sign — this is NOT a follower.
    #[test]
    fn signed_signals_pass_signed() {
        let mut lag = Lag::new(5.0, 5.0);
        lag.prepare(48_000.0);
        for _ in 0..48_000 {
            lag.tick(-1.0);
        }
        assert!(
            lag.state() < -0.99,
            "settled at the NEGATIVE input: {}",
            lag.state()
        );
    }
}
