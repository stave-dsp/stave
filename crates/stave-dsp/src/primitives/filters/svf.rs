//! State-variable filter (Cytomic / Andy Simper ZDF topology).
//!
//! One instance carries N channels of state. Coefficients are shared across
//! channels — call `set(cutoff, resonance)` once per block (or rarer), then
//! `process(sample, channel)` per sample. All four taps (LP / BP / HP /
//! Notch) come out of a single sample step so a multi-output filter plugin
//! pays the math cost only once.

use crate::primitives::denormal;

/// Max channels per SVF instance. Stereo is the common case; raise if a
/// surround filter shows up.
pub const SVF_MAX_CHANNELS: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct SvfTaps {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
    pub notch: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Svf {
    sr: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1eq: [f32; SVF_MAX_CHANNELS],
    ic2eq: [f32; SVF_MAX_CHANNELS],
}

impl Default for Svf {
    fn default() -> Self {
        Self::new()
    }
}

impl Svf {
    pub const fn new() -> Self {
        Self {
            sr: 48_000.0,
            g: 0.0,
            k: 0.0,
            a1: 1.0,
            a2: 0.0,
            a3: 0.0,
            ic1eq: [0.0; SVF_MAX_CHANNELS],
            ic2eq: [0.0; SVF_MAX_CHANNELS],
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sr = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.ic1eq = [0.0; SVF_MAX_CHANNELS];
        self.ic2eq = [0.0; SVF_MAX_CHANNELS];
    }

    /// Compute coefficients for a given cutoff (Hz) + resonance (0..1).
    /// Cheap enough to call per sample if a host insists, but typically you
    /// call once per block.
    #[inline]
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32) {
        let cutoff = clamp(cutoff_hz, 1.0, self.sr * 0.49);
        let res = clamp(resonance, 0.0, 0.999);
        let pi = core::f32::consts::PI;
        self.g = tan32(pi * cutoff / self.sr);
        self.k = 2.0 - 2.0 * res;
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    /// Process one sample on channel `ch`. Returns all four taps.
    #[inline]
    pub fn process(&mut self, x: f32, ch: usize) -> SvfTaps {
        let c = ch.min(SVF_MAX_CHANNELS - 1);
        let ic1 = self.ic1eq[c];
        let ic2 = self.ic2eq[c];
        let v3 = x - ic2;
        let v1 = self.a1 * ic1 + self.a2 * v3;
        let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
        self.ic1eq[c] = denormal::flush(2.0 * v1 - ic1);
        self.ic2eq[c] = denormal::flush(2.0 * v2 - ic2);
        let lp = v2;
        let bp = v1;
        let hp = x - self.k * v1 - v2;
        let notch = hp + lp;
        SvfTaps { lp, bp, hp, notch }
    }

    /// Convenience: just the LP tap. Skips computing BP/HP/Notch in case
    /// the compiler can't fold them away.
    #[inline]
    pub fn process_lp(&mut self, x: f32, ch: usize) -> f32 {
        let c = ch.min(SVF_MAX_CHANNELS - 1);
        let ic1 = self.ic1eq[c];
        let ic2 = self.ic2eq[c];
        let v3 = x - ic2;
        let v1 = self.a1 * ic1 + self.a2 * v3;
        let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
        self.ic1eq[c] = denormal::flush(2.0 * v1 - ic1);
        self.ic2eq[c] = denormal::flush(2.0 * v2 - ic2);
        v2
    }
}

#[inline]
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[inline]
fn tan32(x: f32) -> f32 {
    x.tan()
}
