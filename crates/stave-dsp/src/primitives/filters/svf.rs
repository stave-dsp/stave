//! State-variable filter (Cytomic / Andy Simper ZDF topology), mono.
//!
//! All four responses (lowpass, bandpass, highpass, notch) come out of
//! one step, so a multi-output filter pays the math once: [`Svf::taps`]
//! returns them together; the [`Stage`] view ticks the lowpass. One
//! instance is one channel — lift it with
//! [`PerChannel`](crate::PerChannel) for stereo.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};

/// The four simultaneous responses of one [`Svf`] step.
#[derive(Clone, Copy, Debug)]
pub struct SvfTaps {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
    pub notch: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Svf {
    sample_rate: f32,
    hz: f32,
    q: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1eq: f32,
    ic2eq: f32,
}

impl Default for Svf {
    /// 1 kHz, Butterworth Q.
    fn default() -> Self {
        Self::new(1_000.0, core::f32::consts::FRAC_1_SQRT_2)
    }
}

impl Svf {
    /// A filter with its corner at `hz` and quality factor `q`
    /// (1/√2 = Butterworth; higher rings).
    pub fn new(hz: f32, q: f32) -> Self {
        let mut f = Self {
            sample_rate: 48_000.0,
            hz,
            q,
            g: 0.0,
            k: 0.0,
            a1: 1.0,
            a2: 0.0,
            a3: 0.0,
            ic1eq: 0.0,
            ic2eq: 0.0,
        };
        f.set(hz, q);
        f
    }

    /// Retune (state kept). The corner is held inside (1 Hz, 0.49·fs)
    /// and `q` inside [0.05, 500]. Cheap enough to call per sample.
    #[inline]
    pub fn set(&mut self, hz: f32, q: f32) {
        self.hz = hz;
        self.q = q;
        let cutoff = hz.clamp(1.0, self.sample_rate * 0.49);
        self.g = (core::f32::consts::PI * cutoff / self.sample_rate).tan();
        self.k = 1.0 / q.clamp(0.05, 500.0);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    /// One step, all four responses.
    #[inline]
    pub fn taps(&mut self, x: f32) -> SvfTaps {
        let (v1, v2) = self.step(x);
        let hp = x - self.k * v1 - v2;
        SvfTaps {
            lp: v2,
            bp: v1,
            hp,
            notch: hp + v2,
        }
    }

    #[inline]
    fn step(&mut self, x: f32) -> (f32, f32) {
        let (ic1, ic2) = (self.ic1eq, self.ic2eq);
        let v3 = x - ic2;
        let v1 = self.a1 * ic1 + self.a2 * v3;
        let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
        self.ic1eq = denormal::flush(2.0 * v1 - ic1);
        self.ic2eq = denormal::flush(2.0 * v2 - ic2);
        (v1, v2)
    }
}

impl Stage for Svf {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set(self.hz, self.q);
        self.reset();
    }
    fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }
    /// The lowpass response (the others: [`Svf::taps`]).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.step(x).1
    }
    /// The bilinear SVF's poles are those of
    /// `(1 + gk + g²) + 2(g² − 1)z⁻¹ + (1 − gk + g²)z⁻²`.
    fn tail(&self) -> Tail {
        let (g, k) = (f64::from(self.g), f64::from(self.k));
        let a0 = 1.0 + g * k + g * g;
        Tail::from_biquad(2.0 * (g * g - 1.0) / a0, (1.0 - g * k + g * g) / a0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_are_one_step_and_tick_is_the_lowpass() {
        let (mut a, mut b) = (Svf::new(800.0, 2.0), Svf::new(800.0, 2.0));
        a.prepare(48_000.0);
        b.prepare(48_000.0);
        for i in 0..512 {
            let x = ((i * 31) % 17) as f32 / 8.0 - 1.0;
            let t = a.taps(x);
            assert_eq!(t.lp.to_bits(), b.tick(x).to_bits());
            assert!((t.notch - (t.hp + t.lp)).abs() < 1e-6);
        }
    }

    #[test]
    fn tail_tracks_the_measured_ring() {
        let mut f = Svf::new(500.0, 8.0);
        f.prepare(48_000.0);
        let Tail::Samples(n) = f.tail() else { panic!() };
        f.tick(1.0);
        let mut last_loud = 0u32;
        for i in 1..(4 * n) {
            if f.tick(0.0).abs() > 1e-6 {
                last_loud = i;
            }
        }
        assert!(last_loud <= n, "rang {last_loud} past the reported {n}");
        assert!(
            last_loud > n / 2,
            "reported {n} for a {last_loud}-sample ring"
        );
    }
}
