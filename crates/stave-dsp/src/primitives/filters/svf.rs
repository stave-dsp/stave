//! State-variable filter (Cytomic / Andy Simper ZDF topology), mono.
//!
//! All four responses (lowpass, bandpass, highpass, notch) come out of
//! one step, so a multi-output filter pays the math once: [`Svf::taps`]
//! returns them together; the [`Stage`] view ticks the lowpass. One
//! instance is one channel — lift it with
//! [`PerChannel`](crate::PerChannel) for stereo.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::units::Hz;

/// The four simultaneous responses of one [`Svf`] step.
#[derive(Clone, Copy, Debug)]
pub struct SvfTaps {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
    pub notch: f32,
}

/// A zero-delay-feedback state-variable filter: lowpass, bandpass,
/// highpass and notch from one step, tunable per sample.
///
/// # Models
///
/// The analog two-integrator-loop SVF, `1/(s² + s/Q + 1)` and its band,
/// high and notch outputs, discretised by the trapezoidal rule.
///
/// # Derivation
///
/// Zavalishin's topology-preserving transform (*The Art of VA Filter
/// Design*) in Andy Simper's (Cytomic) linear trapezoidal form, f32.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/svf.rs::responses_match_the_bilinear_prototype`:
/// lowpass, band and highpass taps within 0.01 dB of the prewarped
/// bilinear prototype, 40 Hz–15 kHz.
///
/// # Cost
///
/// No datasheet line yet (DSP.md P1): about a biquad's arithmetic, plus
/// a `tan` on each retune.
///
/// # Aliasing
///
/// None: linear. Retuning per sample does not alias; it modulates.
///
/// # Valid ranges
///
/// Corner held inside (1 Hz, 0.49·fs), `q` inside 0.05–500. f32 state:
/// corners far below the rate lose precision sooner than [`Biquad`](crate::primitives::filters::biquad::Biquad)'s
/// f64.
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
        Self::new(Hz::new(1_000.0), core::f32::consts::FRAC_1_SQRT_2)
    }
}

impl Svf {
    /// A filter with its corner at `cutoff` and quality factor `q`
    /// (1/√2 = Butterworth; higher rings).
    pub fn new(cutoff: Hz, q: f32) -> Self {
        let mut f = Self {
            sample_rate: 48_000.0,
            hz: cutoff.get(),
            q,
            g: 0.0,
            k: 0.0,
            a1: 1.0,
            a2: 0.0,
            a3: 0.0,
            ic1eq: 0.0,
            ic2eq: 0.0,
        };
        f.set(cutoff, q);
        f
    }

    /// Retune (state kept). The corner is held inside (1 Hz, 0.49·fs)
    /// and `q` inside [0.05, 500]. Cheap enough to call per sample.
    #[inline]
    pub fn set(&mut self, cutoff: Hz, q: f32) {
        let hz = cutoff.get();
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
        self.set(Hz::new(self.hz), self.q);
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
    use crate::units::hz;

    #[test]
    fn taps_are_one_step_and_tick_is_the_lowpass() {
        let (mut a, mut b) = (Svf::new(hz(800.0), 2.0), Svf::new(hz(800.0), 2.0));
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
    fn responses_match_the_bilinear_prototype() {
        // The analog prototype 1/(s² + s/Q + 1) (lowpass), s/(…)
        // (band) and s²/(…) (highpass) under the bilinear transform
        // prewarped at the corner, s = j·tan(πf/fs)/tan(πf₀/fs), against
        // tones measured through `taps`: within 0.01 dB.
        let (fs, f0, q) = (48_000.0f64, 900.0f64, 2.0f64);
        let g = (core::f64::consts::PI * f0 / fs).tan();
        for hz in [40.0f64, 300.0, 900.0, 2_000.0, 6_000.0, 15_000.0] {
            let w = (core::f64::consts::PI * hz / fs).tan() / g;
            // |s² + s/Q + 1| at s = jw.
            let den = ((1.0 - w * w).powi(2) + (w / q).powi(2)).sqrt();
            let want = [1.0 / den, w / den, w * w / den].map(|m| 20.0 * m.log10());
            for (tap, want) in want.iter().enumerate() {
                let mut f = Svf::new(Hz::new(f0 as f32), q as f32);
                f.prepare(fs as f32);
                let n = fs as usize / 2;
                let (mut acc_in, mut acc_out) = (0.0f64, 0.0f64);
                for i in 0..2 * n {
                    let x = (core::f64::consts::TAU * hz * i as f64 / fs).sin() as f32;
                    let t = f.taps(x);
                    let y = [t.lp, t.bp, t.hp][tap];
                    if i >= n {
                        acc_in += f64::from(x).powi(2);
                        acc_out += f64::from(y).powi(2);
                    }
                }
                let measured = 10.0 * (acc_out / acc_in).log10();
                assert!(
                    (measured - want).abs() < 0.01,
                    "tap {tap} at {hz} Hz: {measured:.4} dB, prototype {want:.4}"
                );
            }
        }
    }

    #[test]
    fn tail_tracks_the_measured_ring() {
        let mut f = Svf::new(hz(500.0), 8.0);
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
