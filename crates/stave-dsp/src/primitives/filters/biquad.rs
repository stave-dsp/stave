//! Transposed-direct-form-II biquad with Robert Bristow-Johnson cookbook
//! coefficient setters. Denormal-safe, one allocation per instance.
//!
//! Coefficients and state are f64. A corner far below the sample rate
//! puts the poles a hair inside z = 1: a 7 Hz high-pass at 768 kHz sits
//! 3e-9 inside the stability triangle (|a1| < 1 + a2) while f32's step
//! at 2.0 is 2.4e-7, so f32 coefficients round the pole onto or past
//! the unit circle and the section rings forever or diverges to NaN.
//! The design is also cancellation-free (1 − cos ω written 2·sin²(ω/2)),
//! so a 10 Hz low-pass at 384 kHz keeps its gain instead of rounding
//! to zero. Audio in and out stays f32.

use crate::primitives::denormal;

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::new()
    }
}

/// The bilinear-mapped corner: (cos ω₀, sin ω₀, 1 − cos ω₀) in f64, the
/// corner held inside (1 Hz, 0.499·fs) — the map is singular at Nyquist.
#[inline]
fn corner(freq_hz: f32, sample_rate: f32) -> (f64, f64, f64) {
    let sample_rate = f64::from(sample_rate.max(1.0));
    let freq = f64::from(freq_hz).clamp(1.0_f64.min(sample_rate * 0.499), sample_rate * 0.499);
    let w0 = core::f64::consts::TAU * freq / sample_rate;
    let half = (0.5 * w0).sin();
    (w0.cos(), w0.sin(), 2.0 * half * half)
}

impl Biquad {
    pub const fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Load `spec`'s coefficients at `sample_rate`. Corners are held
    /// inside (1 Hz, 0.499·fs): the bilinear map is singular at Nyquist.
    pub fn set(&mut self, spec: FilterSpec, sample_rate: f32) {
        match spec {
            FilterSpec::Lowpass { hz, q } => self.lowpass(hz, q, sample_rate),
            FilterSpec::Highpass { hz, q } => self.highpass(hz, q, sample_rate),
            FilterSpec::Allpass { hz, q } => self.allpass(hz, q, sample_rate),
            FilterSpec::Peak { hz, db, q } => self.peak(hz, db, q, sample_rate),
            FilterSpec::LowShelf { hz, db } => self.low_shelf(hz, db, sample_rate),
            FilterSpec::HighShelf { hz, db } => self.high_shelf(hz, db, sample_rate),
            FilterSpec::LowShelfRbj { hz, db } => self.low_shelf_rbj(hz, db, sample_rate),
            FilterSpec::HighShelfRbj { hz, db } => self.high_shelf_rbj(hz, db, sample_rate),
        }
    }

    /// Normalize by a0 and store.
    #[inline]
    fn store(&mut self, b: [f64; 3], a: [f64; 3]) {
        let inv = 1.0 / a[0];
        self.b0 = b[0] * inv;
        self.b1 = b[1] * inv;
        self.b2 = b[2] * inv;
        self.a1 = a[1] * inv;
        self.a2 = a[2] * inv;
    }

    fn lowpass(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let (cw, sw, one_minus_cw) = corner(cutoff_hz, sample_rate);
        let alpha = sw / (2.0 * f64::from(q));
        self.store(
            [one_minus_cw * 0.5, one_minus_cw, one_minus_cw * 0.5],
            [1.0 + alpha, -2.0 * cw, 1.0 - alpha],
        );
    }

    fn highpass(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let (cw, sw, _) = corner(cutoff_hz, sample_rate);
        let alpha = sw / (2.0 * f64::from(q));
        let one_plus_cw = 1.0 + cw;
        self.store(
            [one_plus_cw * 0.5, -one_plus_cw, one_plus_cw * 0.5],
            [1.0 + alpha, -2.0 * cw, 1.0 - alpha],
        );
    }

    fn allpass(&mut self, freq_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let ratio = (f64::from(freq_hz) / f64::from(sample_rate.max(1.0))).clamp(1e-6, 0.49);
        let w0 = core::f64::consts::TAU * ratio;
        let alpha = w0.sin() / (2.0 * f64::from(q));
        let cw = w0.cos();
        self.store(
            [1.0 - alpha, -2.0 * cw, 1.0 + alpha],
            [1.0 + alpha, -2.0 * cw, 1.0 - alpha],
        );
    }

    fn peak(&mut self, freq_hz: f32, gain_db: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let (cw, sw, _) = corner(freq_hz, sample_rate);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        let alpha = sw / (2.0 * f64::from(q));
        self.store(
            [1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a],
        );
    }

    /// Shelf coefficients for amplitude `a`, corner `one_minus_cw`
    /// (1 − cos ω₀) and shelf `alpha`. The cookbook's (a±1) ∓ (a∓1)·cos ω₀
    /// terms are rewritten around 1 − cos ω₀ so nothing cancels as
    /// cos ω₀ → 1.
    #[inline]
    fn shelf(&mut self, a: f64, one_minus_cw: f64, alpha: f64, high: bool) {
        let two_sqa = 2.0 * a.sqrt();
        // (a+1) − (a−1)·cw = 2 + (a−1)·(1 − cw); (a+1) + (a−1)·cw = 2a − (a−1)·(1 − cw)
        let minus = 2.0 + (a - 1.0) * one_minus_cw;
        let plus = 2.0 * a - (a - 1.0) * one_minus_cw;
        // (a−1) − (a+1)·cw = (a+1)(1 − cw) − 2; (a−1) + (a+1)·cw = 2a − (a+1)(1 − cw)
        let lo_b1 = (a + 1.0) * one_minus_cw - 2.0;
        let lo_a1 = 2.0 * a - (a + 1.0) * one_minus_cw;
        if high {
            self.store(
                [
                    a * (plus + two_sqa * alpha),
                    -2.0 * a * lo_a1,
                    a * (plus - two_sqa * alpha),
                ],
                [
                    minus + two_sqa * alpha,
                    2.0 * lo_b1,
                    minus - two_sqa * alpha,
                ],
            );
        } else {
            self.store(
                [
                    a * (minus + two_sqa * alpha),
                    2.0 * a * lo_b1,
                    a * (minus - two_sqa * alpha),
                ],
                [plus + two_sqa * alpha, -2.0 * lo_a1, plus - two_sqa * alpha],
            );
        }
    }

    /// The STAVE shelf alpha — deliberately NOT the RBJ cookbook curve.
    #[inline]
    fn stave_shelf_alpha(sw: f64, a: f64) -> f64 {
        sw * 0.5 * (((a + 1.0 / a) * (1.0 / 0.9) - 1.0) + 2.0).sqrt() * 0.5
    }

    /// The STAVE low shelf — deliberately NOT the RBJ cookbook curve
    /// (use `FilterSpec::LowShelfRbj` for that).
    /// Measured at +6 dB / 200 Hz / 48 k (shelf_conformance.rs): a
    /// resonant lip of +0.63 dB just below the corner and a −0.63 dB
    /// dip just above it, settling to flat. That corner character is
    /// part of the voicing everywhere this is used (tape head-bump,
    /// companding emphasis); the alpha is symmetric in A↔1/A, so
    /// reciprocal ±dB pairs still cancel exactly (nyquist_conformance
    /// reciprocity check). Deliberately not the RBJ cookbook formula:
    /// measurement chose keeping the sound.
    fn low_shelf(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let (_, sw, one_minus_cw) = corner(freq_hz, sample_rate);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        self.shelf(a, one_minus_cw, Self::stave_shelf_alpha(sw, a), false);
    }

    /// The STAVE high shelf — same deliberate non-cookbook corner
    /// character as `FilterSpec::LowShelf`; see there.
    fn high_shelf(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let (_, sw, one_minus_cw) = corner(freq_hz, sample_rate);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        self.shelf(a, one_minus_cw, Self::stave_shelf_alpha(sw, a), true);
    }

    /// Cookbook-exact RBJ low shelf, S = 1 (monotonic — no corner lip).
    fn low_shelf_rbj(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let (_, sw, one_minus_cw) = corner(freq_hz, sample_rate);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        // S = 1 ⇒ (A + 1/A)(1/S − 1) + 2 = 2.
        self.shelf(a, one_minus_cw, sw * 0.5 * core::f64::consts::SQRT_2, false);
    }

    /// Cookbook-exact RBJ high shelf, S = 1.
    fn high_shelf_rbj(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let (_, sw, one_minus_cw) = corner(freq_hz, sample_rate);
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        self.shelf(a, one_minus_cw, sw * 0.5 * core::f64::consts::SQRT_2, true);
    }

    /// Complex frequency response H(e^{jω}) of the CURRENT
    /// coefficients — the display/analysis read (first consumer:
    /// tolex's response scene, which is invariant-pinned against the
    /// rendered audio). Touches no state.
    pub fn response_at(&self, hz: f32, rate: f32) -> (f32, f32) {
        let w = core::f64::consts::TAU * (f64::from(hz) / f64::from(rate.max(1.0))).clamp(0.0, 0.5);
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        let den = (dr * dr + di * di).max(1e-300);
        (
            ((nr * dr + ni * di) / den) as f32,
            ((ni * dr - nr * di) / den) as f32,
        )
    }

    /// |H(e^{jω})| — see [`response_at`](Self::response_at).
    pub fn magnitude_at(&self, hz: f32, rate: f32) -> f32 {
        let (re, im) = self.response_at(hz, rate);
        (re * re + im * im).sqrt()
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let x = f64::from(x);
        let y = self.b0 * x + self.z1;
        self.z1 = denormal::flush64(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = denormal::flush64(self.b2 * x - self.a2 * y);
        denormal::flush(y as f32)
    }
}

// FilterStage — Biquad fitted with the node conventions: the
// spec stays in Hz/dB and reapplies at whatever rate the stage
// is prepared for (the rate contract).
use crate::kernel::node::Stage;

/// Filter spec, reapplied at whatever rate the stage is prepared for —
/// the knob stays in Hz/dB while the coefficients follow the rate
/// contract automatically.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterSpec {
    Lowpass {
        hz: f32,
        q: f32,
    },
    Highpass {
        hz: f32,
        q: f32,
    },
    /// RBJ allpass: unity magnitude, −180° at `hz` (Q shapes the
    /// transition) — phase-aligns parallel paths whose crossovers
    /// rotate differently.
    Allpass {
        hz: f32,
        q: f32,
    },
    /// Bell — the placed mid-scoop/mid-bump voicing filter.
    Peak {
        hz: f32,
        db: f32,
        q: f32,
    },
    /// The stave shelf: a +0.63 dB lip below the corner at +6 dB (see
    /// `Biquad::low_shelf`).
    LowShelf {
        hz: f32,
        db: f32,
    },
    HighShelf {
        hz: f32,
        db: f32,
    },
    /// Cookbook RBJ shelf, S = 1 (monotonic).
    LowShelfRbj {
        hz: f32,
        db: f32,
    },
    HighShelfRbj {
        hz: f32,
        db: f32,
    },
}

pub struct FilterStage {
    spec: FilterSpec,
    biquad: Biquad,
    rate: f32,
}

impl FilterStage {
    pub fn new(spec: FilterSpec) -> Self {
        Self {
            spec,
            biquad: Biquad::new(),
            rate: 0.0,
        }
    }
    /// Retune (block rate) — coefficients recomputed at the prepared rate.
    pub fn set(&mut self, spec: FilterSpec) {
        self.spec = spec;
        if self.rate > 0.0 {
            self.apply();
        }
    }
    fn apply(&mut self) {
        self.biquad.set(self.spec, self.rate);
    }
}

impl Stage for FilterStage {
    fn prepare(&mut self, rate: f32) {
        debug_assert!(rate > 0.0, "sample rate must be positive");
        self.rate = rate;
        self.apply();
        self.biquad.reset();
    }
    fn reset(&mut self) {
        self.biquad.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.biquad.process(x)
    }
}
