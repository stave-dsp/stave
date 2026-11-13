use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::hilbert::Hilbert;
use crate::units::Hz;

/// A frequency shifter: every partial moved by the same number of hertz,
/// up or down, so harmonic sounds turn inharmonic.
///
/// # Models
/// Single-sideband modulation by a sinusoid: `Re{(re + j·im)·e^{jΩt}}`
/// of the input's analytic signal, which keeps only the shifted
/// sideband. No hardware claim (the Bode shifter did this with analog
/// 90° networks).
///
/// # Derivation
/// The [`Hilbert`] pair (90° ± 0.5° from 20 Hz by default) and a
/// quadrature oscillator, a unit phasor rotated each sample in f64 and
/// renormalised; the shift may change per sample without a click.
///
/// # Verified
/// `crates/stave-dsp/src/effects/modulation/tests.rs::shifter_rejects_the_image`:
/// tones from 50 Hz to 15 kHz shifted ±100 Hz and ±1 kHz at 44.1, 48 and
/// 96 kHz: the wanted sideband at unity within 1e-3 dB, the image at
/// least 40 dB below it (measured 50.1 dB; the Hilbert design's 0.36°
/// ripple bounds it near 50 dB).
///
/// # Cost
/// Datasheet `"FrequencyShifter"` at 48 kHz.
///
/// # Aliasing
/// None for shifts that keep partials inside (0, fs/2); a partial shifted
/// below 0 Hz folds back (through-zero), above Nyquist it wraps.
///
/// # Valid ranges
/// Shift within ±fs/2; input partials within the Hilbert band
/// (20 Hz to fs/2 − 20 Hz by default).
#[derive(Clone, Debug)]
pub struct FrequencyShifter {
    hilbert: Hilbert,
    shift: Hz,
    sample_rate: f32,
    phasor: (f64, f64),
    rotation: (f64, f64),
}

impl FrequencyShifter {
    /// A shifter moving every partial by `shift` (negative: down), on
    /// the default [`Hilbert`] design.
    pub fn new(shift: Hz) -> Self {
        Self::with_hilbert(shift, Hilbert::default())
    }

    /// The same on a [`Hilbert`] of the caller's design (a tighter phase
    /// budget rejects the image further).
    pub fn with_hilbert(shift: Hz, hilbert: Hilbert) -> Self {
        let mut s = Self {
            hilbert,
            shift,
            sample_rate: 48_000.0,
            phasor: (1.0, 0.0),
            rotation: (1.0, 0.0),
        };
        s.set_shift(shift);
        s
    }

    /// Change the shift; the oscillator's phase continues.
    pub fn set_shift(&mut self, shift: Hz) {
        self.shift = shift;
        let step = shift.cycles_per_sample(crate::units::SampleRate::new(self.sample_rate))
            * core::f64::consts::TAU;
        self.rotation = (step.cos(), step.sin());
    }

    /// One sample: `(shifted by +shift, shifted by −shift)`, both
    /// sidebands from one analytic signal.
    #[inline]
    pub fn sidebands(&mut self, x: f32) -> (f32, f32) {
        let (re, im) = self.hilbert.analytic_f64(f64::from(x));
        let (c, s) = self.phasor;
        let (rc, rs) = self.rotation;
        let next = (c * rc - s * rs, c * rs + s * rc);
        // Renormalise: one Newton step towards |phasor| = 1.
        let g = 1.5 - 0.5 * (next.0 * next.0 + next.1 * next.1);
        self.phasor = (next.0 * g, next.1 * g);
        ((re * c - im * s) as f32, (re * c + im * s) as f32)
    }
}

impl Stage for FrequencyShifter {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.hilbert.prepare(sample_rate);
        self.set_shift(self.shift);
        self.reset();
    }
    fn reset(&mut self) {
        self.hilbert.reset();
        self.phasor = (1.0, 0.0);
    }
    /// The input shifted by `shift`.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.sidebands(x).0
    }
    fn tail(&self) -> Tail {
        self.hilbert.tail()
    }
}
