//! Transposed-direct-form-II biquad with Robert Bristow-Johnson cookbook
//! coefficients (plus stave's own shelf), denormal-safe. The response is
//! declared as a [`FilterSpec`] in hertz and decibels and re-derived at
//! whatever rate the biquad is prepared for.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};

/// A second-order section: one [`FilterSpec`] at the prepared rate.
/// Before `prepare` it runs at 48 kHz; `Biquad::default()` is a
/// passthrough.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
    spec: Option<FilterSpec>,
    sample_rate: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::passthrough()
    }
}

impl Biquad {
    /// A biquad realizing `spec`.
    pub fn new(spec: FilterSpec) -> Self {
        let mut b = Self::passthrough();
        b.set(spec);
        b
    }

    /// Unity gain, no state: the identity section.
    pub const fn passthrough() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
            spec: None,
            sample_rate: 48_000.0,
        }
    }

    /// The response currently realized (`None` for a passthrough).
    pub const fn spec(&self) -> Option<FilterSpec> {
        self.spec
    }

    /// Retune (block rate): coefficients re-derived at the prepared
    /// rate, state kept. Corners are held inside (1 Hz, 0.499·fs): the
    /// bilinear map is singular at Nyquist.
    pub fn set(&mut self, spec: FilterSpec) {
        self.spec = Some(spec);
        self.apply();
    }

    fn apply(&mut self) {
        let Some(spec) = self.spec else {
            return;
        };
        let sample_rate = self.sample_rate;
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

    fn lowpass(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let pi = core::f32::consts::PI;
        let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.499);
        let w0 = 2.0 * pi * cutoff / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let b0 = (1.0 - cw) * 0.5;
        let b1 = 1.0 - cw;
        let b2 = (1.0 - cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn highpass(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let pi = core::f32::consts::PI;
        let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.499);
        let w0 = 2.0 * pi * cutoff / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let b0 = (1.0 + cw) * 0.5;
        let b1 = -(1.0 + cw);
        let b2 = (1.0 + cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn allpass(&mut self, freq_hz: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let w0 = core::f32::consts::TAU * (freq_hz / sample_rate).clamp(1e-6, 0.49);
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - alpha) / a0;
        self.b1 = (-2.0 * cw) / a0;
        self.b2 = (1.0 + alpha) / a0;
        self.a1 = (-2.0 * cw) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn peak(&mut self, freq_hz: f32, gain_db: f32, q: f32, sample_rate: f32) {
        debug_assert!(q > 0.0, "Q must be positive");
        let pi = core::f32::consts::PI;
        let freq = freq_hz.clamp(1.0, sample_rate * 0.499);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * pi * freq / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cw;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha / a;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
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
        let pi = core::f32::consts::PI;
        let freq = freq_hz.clamp(1.0, sample_rate * 0.499);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * pi * freq / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        // Stave-shelf alpha (see doc comment above; NOT cookbook).
        let alpha = sw * 0.5 * (((a + 1.0 / a) * (1.0 / 0.9) - 1.0) + 2.0).sqrt() * 0.5;
        let two_sqa = 2.0 * a.sqrt();
        let b0 = a * ((a + 1.0) - (a - 1.0) * cw + two_sqa * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cw);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cw - two_sqa * alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cw + two_sqa * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cw);
        let a2 = (a + 1.0) + (a - 1.0) * cw - two_sqa * alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// The STAVE high shelf — same deliberate non-cookbook corner
    /// character as `FilterSpec::LowShelf`; see there.
    fn high_shelf(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let pi = core::f32::consts::PI;
        let freq = freq_hz.clamp(1.0, sample_rate * 0.499);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * pi * freq / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        // Stave-shelf alpha (see set_low_shelf; NOT cookbook).
        let alpha = sw * 0.5 * (((a + 1.0 / a) * (1.0 / 0.9) - 1.0) + 2.0).sqrt() * 0.5;
        let two_sqa = 2.0 * a.sqrt();
        let b0 = a * ((a + 1.0) + (a - 1.0) * cw + two_sqa * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cw);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cw - two_sqa * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cw + two_sqa * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cw);
        let a2 = (a + 1.0) - (a - 1.0) * cw - two_sqa * alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    /// Cookbook-exact RBJ low shelf, S = 1 (monotonic — no corner lip).
    fn low_shelf_rbj(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let pi = core::f32::consts::PI;
        let freq = freq_hz.clamp(1.0, sample_rate * 0.499);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * pi * freq / sample_rate;
        let (cw, sw) = (w0.cos(), w0.sin());
        // S = 1 ⇒ (A + 1/A)(1/S − 1) + 2 = 2.
        let alpha = sw * 0.5 * 2.0f32.sqrt();
        let two_sqa = 2.0 * a.sqrt();
        let b0 = a * ((a + 1.0) - (a - 1.0) * cw + two_sqa * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cw);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cw - two_sqa * alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cw + two_sqa * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cw);
        let a2 = (a + 1.0) + (a - 1.0) * cw - two_sqa * alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Cookbook-exact RBJ high shelf, S = 1.
    fn high_shelf_rbj(&mut self, freq_hz: f32, gain_db: f32, sample_rate: f32) {
        let pi = core::f32::consts::PI;
        let freq = freq_hz.clamp(1.0, sample_rate * 0.499);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * pi * freq / sample_rate;
        let (cw, sw) = (w0.cos(), w0.sin());
        let alpha = sw * 0.5 * 2.0f32.sqrt();
        let two_sqa = 2.0 * a.sqrt();
        let b0 = a * ((a + 1.0) + (a - 1.0) * cw + two_sqa * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cw);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cw - two_sqa * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cw + two_sqa * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cw);
        let a2 = (a + 1.0) - (a - 1.0) * cw - two_sqa * alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Complex frequency response H(e^{jω}) of the current
    /// coefficients at the prepared rate — the display and analysis
    /// read. Touches no state.
    pub fn response_at(&self, hz: f32) -> (f32, f32) {
        let w = core::f32::consts::TAU * (hz / self.sample_rate.max(1.0)).clamp(0.0, 0.5);
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        let den = (dr * dr + di * di).max(1e-30);
        ((nr * dr + ni * di) / den, (ni * dr - nr * di) / den)
    }

    /// |H(e^{jω})| — see [`response_at`](Self::response_at).
    pub fn magnitude_at(&self, hz: f32) -> f32 {
        let (re, im) = self.response_at(hz);
        (re * re + im * im).sqrt()
    }
}

impl Stage for Biquad {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.apply();
        self.reset();
    }
    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = denormal::flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = denormal::flush(self.b2 * x - self.a2 * y);
        y
    }
    /// Samples for the slower pole to fall 120 dB.
    fn tail(&self) -> Tail {
        match self.spec {
            None => Tail::None,
            Some(_) => Tail::from_biquad(f64::from(self.a1), f64::from(self.a2)),
        }
    }
}

/// A second-order response in musical units, re-derived at whatever rate
/// the [`Biquad`] is prepared for. `q` is the quality factor (Butterworth
/// = 1/√2) everywhere in stave-dsp.
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
