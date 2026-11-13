//! Quantisation to a word length, with dither and noise shaping (DSP.md
//! H8): what a converter or a word-length reduction does to a signal.
//!
//! A [`Quantiser`] maps full scale ±1 onto `bits`-bit two's-complement
//! codes, step `Δ = 2^(1 − bits)`, mid-tread (zero is a code). Each
//! sample:
//!
//! ```text
//!     v = x − Σ h_k·e[n−k]          the shaping filter's feedback
//!     y = Q(v + d)                  rounded or truncated, then saturated
//!     e = y − v                     the total error, dither included
//! ```
//!
//! so the output is `y = x + (1 − H(z))·e`: the error `e` is white when
//! the dither makes it so, and the noise transfer function `1 − H(z)`
//! shapes its spectrum. With triangular (TPDF) dither of ±Δ peak the
//! error's first and second moments are independent of the signal
//! (Wannamaker, Lipshitz & Vanderkooy, "A Theory of Nonsubtractive
//! Dither", IEEE Trans. Signal Processing 48(2), 2000): no distortion and
//! no noise modulation, at a total error power `Δ²/4` — three times the
//! `Δ²/12` of plain rounding on a busy signal.
//!
//! ```
//! use stave_dsp::primitives::quantise::{Dither, Quantiser, Shaping};
//!
//! let mut q = Quantiser::new(16).with_dither(Dither::Triangular, 1);
//! let y = q.quantise(0.25);
//! // On the 16-bit grid, within the dither's ±Δ plus half a step.
//! let step = q.step();
//! assert!(((y / step).round() - y / step).abs() < 1e-9);
//! assert!((y - 0.25).abs() <= 1.5 * step);
//! let shaped = Quantiser::new(16).with_shaping(Shaping::SECOND_ORDER).unwrap();
//! assert_eq!(shaped.bits(), 16);
//! ```

use crate::kernel::node::{Stage, Tail};
use crate::primitives::random::SplitMix64;

/// How a value between two codes becomes one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// To the nearest code, halves upward: `floor(v/Δ + ½)`.
    Nearest,
    /// Toward −∞: the low bits dropped from a two's-complement word, as
    /// a converter or an arithmetic shift does.
    Truncate,
}

/// The noise added before the quantiser.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dither {
    None,
    /// Uniform over ±Δ/2 (RPDF): removes distortion, not noise
    /// modulation.
    Rectangular,
    /// The sum of two independent ±Δ/2 uniforms (TPDF, ±Δ peak): the
    /// least power that makes the error's mean and variance independent
    /// of the signal.
    Triangular,
    /// The difference of successive ±Δ/2 uniforms: triangular in
    /// distribution, its power rising at 6 dB/octave (a `1 − z⁻¹`
    /// spectrum), so less of it lands where the ear is sensitive.
    HighpassTriangular,
}

/// The most taps a shaping filter may have.
pub const MAX_TAPS: usize = 16;

/// The error-feedback filter `H(z) = Σ h_k z^−k` (`k` from 1); the noise
/// transfer function is `1 − H(z)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Shaping(pub &'static [f64]);

impl Shaping {
    /// White error: `1 − H = 1`.
    pub const NONE: Self = Self(&[]);
    /// `1 − z⁻¹`: zero at DC, +6 dB at Nyquist, rising 6 dB/octave.
    pub const FIRST_ORDER: Self = Self(&[1.0]);
    /// `(1 − z⁻¹)²`: +12 dB at Nyquist, rising 12 dB/octave.
    pub const SECOND_ORDER: Self = Self(&[2.0, -1.0]);

    /// The noise transfer function's power gain `|1 − H(e^{jω})|²` at
    /// `omega` radians per sample.
    pub fn power_gain(&self, omega: f64) -> f64 {
        let (mut re, mut im) = (1.0, 0.0);
        for (k, h) in self.0.iter().enumerate() {
            let p = omega * (k + 1) as f64;
            re -= h * p.cos();
            im += h * p.sin();
        }
        re * re + im * im
    }
}

/// A shaping filter longer than [`MAX_TAPS`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TooManyTaps(pub usize);

/// A word-length reduction: dither, error-feedback noise shaping,
/// rounding or truncation, saturation at the word's ends.
///
/// # Models
/// A `bits`-bit two's-complement quantiser on full scale ±1 (step
/// `Δ = 2^(1 − bits)`), with optional RPDF, TPDF or highpass TPDF dither
/// and an error-feedback noise shaper.
///
/// # Derivation
/// Textbook: the quantisation error model (Widrow; error uniform over
/// ±Δ/2, power `Δ²/12`, white for a busy signal) and nonsubtractive
/// dither theory (Wannamaker, Lipshitz & Vanderkooy 2000: TPDF of ±Δ
/// makes the error's first two moments signal-independent, total power
/// `Δ²/4`); error feedback places the total error `e` behind the noise
/// transfer function `1 − H(z)` (module docs).
///
/// # Verified
/// `crates/stave-dsp/src/primitives/quantise/tests.rs::error_powers_match_theory`
/// (`Δ²/12` rounding a busy signal, `Δ²/6` RPDF, `Δ²/4` TPDF and
/// highpass TPDF, within 1 %; 16-bit TPDF at −96.33 dBFS),
/// `crates/stave-dsp/src/primitives/quantise/tests.rs::full_scale_sines_reach_the_textbook_snr`
/// (`6.02·N + 1.76` dB within 0.3 dB at 8, 12 and 16 bits),
/// `crates/stave-dsp/src/primitives/quantise/tests.rs::tpdf_error_moments_do_not_depend_on_the_signal`
/// (mean within 0.006 Δ and power within 1.5 % of `Δ²/4` at every offset
/// across one step; RPDF's swings from 0 to `Δ²/4`, as theory says),
/// `crates/stave-dsp/src/primitives/quantise/tests.rs::shaped_error_spectra_follow_the_noise_transfer_function`
/// (the error's power spectrum against `(Δ²/4)·|1 − H|²`, within 0.3 dB
/// per band) and
/// `crates/stave-dsp/src/primitives/quantise/tests.rs::undithered_low_level_tones_distort_and_dithered_ones_do_not`.
///
/// # Cost
/// Datasheet `"Quantiser 16-bit TPDF"` and `"Quantiser 16-bit TPDF 2nd-order shaped"`.
///
/// # Aliasing
/// None of its own: it runs at the rate it is given, and its error is
/// noise (dithered) or harmonics of the input folded at that rate
/// (undithered), which is the device's sound.
///
/// # Valid ranges
/// `bits` 2–32; any input (beyond full scale it saturates, and the
/// shaper's error is clamped to ±2Δ so an overload never makes it
/// unstable); shaping filters of at most [`MAX_TAPS`] taps.
#[derive(Clone, Debug)]
pub struct Quantiser {
    bits: u32,
    step: f64,
    min_code: i64,
    max_code: i64,
    rounding: Rounding,
    dither: Dither,
    shaping: Shaping,
    seed: u64,
    rng: SplitMix64,
    last_uniform: f64,
    /// The last errors, newest first.
    errors: [f64; MAX_TAPS],
}

impl Quantiser {
    /// Rounding to the nearest code, no dither, no shaping. `bits` is
    /// clamped to 2–32.
    pub fn new(bits: u32) -> Self {
        let bits = bits.clamp(2, 32);
        let half = 1i64 << (bits - 1);
        let seed = 1;
        Self {
            bits,
            step: 2f64.powi(1 - bits as i32),
            min_code: -half,
            max_code: half - 1,
            rounding: Rounding::Nearest,
            dither: Dither::None,
            shaping: Shaping::NONE,
            seed,
            rng: SplitMix64::new(seed),
            last_uniform: 0.0,
            errors: [0.0; MAX_TAPS],
        }
    }

    pub fn with_rounding(mut self, rounding: Rounding) -> Self {
        self.rounding = rounding;
        self
    }

    /// Dither drawn from a stream seeded with `seed` (the same seed, the
    /// same noise: runs reproduce).
    pub fn with_dither(mut self, dither: Dither, seed: u64) -> Self {
        self.dither = dither;
        self.seed = seed;
        self.reset();
        self
    }

    pub fn with_shaping(mut self, shaping: Shaping) -> Result<Self, TooManyTaps> {
        if shaping.0.len() > MAX_TAPS {
            return Err(TooManyTaps(shaping.0.len()));
        }
        self.shaping = shaping;
        Ok(self)
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// One code's width, `Δ`.
    pub fn step(&self) -> f64 {
        self.step
    }

    /// The noise transfer function in use.
    pub fn shaping(&self) -> Shaping {
        self.shaping
    }

    /// The code for `v` (already dithered and shaped), saturated.
    #[inline]
    pub fn code(&self, v: f64) -> i64 {
        let scaled = v / self.step;
        let code = match self.rounding {
            Rounding::Nearest => (scaled + 0.5).floor(),
            Rounding::Truncate => scaled.floor(),
        };
        // NaN quantises to zero rather than to an end.
        if code.is_nan() {
            return 0;
        }
        (code.clamp(self.min_code as f64, self.max_code as f64)) as i64
    }

    fn dither_sample(&mut self) -> f64 {
        let step = self.step;
        match self.dither {
            Dither::None => 0.0,
            Dither::Rectangular => self.rng.next_centred() * step,
            Dither::Triangular => (self.rng.next_centred() + self.rng.next_centred()) * step,
            Dither::HighpassTriangular => {
                let u = self.rng.next_centred();
                let d = (u - self.last_uniform) * step;
                self.last_uniform = u;
                d
            }
        }
    }

    /// One sample to its code, as a two's-complement integer.
    #[inline]
    pub fn quantise_code(&mut self, x: f64) -> i64 {
        let mut v = x;
        for (h, e) in self.shaping.0.iter().zip(&self.errors) {
            v -= h * e;
        }
        let d = self.dither_sample();
        let code = self.code(v + d);
        if !self.shaping.0.is_empty() {
            let limit = 2.0 * self.step;
            let e = (code as f64 * self.step - v).clamp(-limit, limit);
            self.errors.copy_within(0..MAX_TAPS - 1, 1);
            self.errors[0] = e;
        }
        code
    }

    /// One sample, quantised: its code times `Δ`.
    #[inline]
    pub fn quantise(&mut self, x: f64) -> f64 {
        self.quantise_code(x) as f64 * self.step
    }
}

impl Stage for Quantiser {
    /// The dither stream restarts and the shaper's memory clears.
    fn reset(&mut self) {
        self.rng = SplitMix64::new(self.seed);
        self.last_uniform = 0.0;
        self.errors = [0.0; MAX_TAPS];
    }

    fn tick(&mut self, x: f32) -> f32 {
        self.quantise(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        Tail::None
    }
}

#[cfg(test)]
mod tests;
