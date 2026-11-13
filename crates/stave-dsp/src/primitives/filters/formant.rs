//! Formant (vowel) filters: [`Formants`], a cascade of Klatt's digital
//! [`Resonator`]s set from a [`Formant`] table — Peterson & Barney's
//! vowels ([`Vowel::formants`]) or your own.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::units::Hz;

mod tables;

pub use tables::{bandwidths, Speaker, Vowel};

/// One formant: a resonance's centre and its −3 dB bandwidth.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Formant {
    /// The resonance's centre (its pole's frequency).
    pub centre: Hz,
    /// Its bandwidth (its pole's: `r = e^(−π·B/sample_rate)`).
    pub bandwidth: Hz,
}

impl Formant {
    /// The formant at `t` of the way from `self` to `to` (0 = `self`):
    /// centres and bandwidths geometrically, as pitch moves.
    pub fn toward(self, to: Formant, t: f32) -> Formant {
        let geo = |a: f32, b: f32| a * (b / a).powf(t);
        Formant {
            centre: Hz::new(geo(self.centre.get(), to.centre.get())),
            bandwidth: Hz::new(geo(self.bandwidth.get(), to.bandwidth.get())),
        }
    }
}

/// Klatt's digital resonator: one formant, all-pole, unity gain at DC.
///
/// # Models
///
/// The second-order resonator of Klatt's cascade/parallel formant
/// synthesiser (Klatt 1980, "Software for a cascade/parallel formant
/// synthesizer", JASA 67(3), eq. 1): `y = A·x + B·y₁ + C·y₂` with
/// `C = −e^(−2πBw/fs)`, `B = 2e^(−πBw/fs)·cos(2πF/fs)`, `A = 1 − B − C`.
///
/// # Derivation
///
/// The pole pair `r·e^(±jθ)` with `r = e^(−π·Bw/fs)` (impulse-invariant
/// bandwidth) and `θ = 2πF/fs`; `A` normalises DC to unity, so a cascade
/// of resonators is the vocal tract's all-pole response. f64.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/formant/tests.rs::resonators_peak_at_their_formant`:
/// over every formant of the Peterson & Barney table at 44.1–96 kHz the
/// measured peak lies within 1 % of the centre (0.79 % worst: a damped
/// resonance peaks a little below its pole) and the −3 dB width within
/// 2 % of the bandwidth (1.6 % worst).
///
/// # Cost
///
/// About a third of `"Formants (3, Peterson & Barney)"` on the datasheet.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// Centre inside (0, 0.5·fs), bandwidth > 0; a narrow formant far below
/// the rate keeps its precision (f64).
#[derive(Copy, Clone, Debug)]
pub struct Resonator {
    formant: Formant,
    sample_rate: f32,
    a: f64,
    b: f64,
    c: f64,
    y: [f64; 2],
}

impl Resonator {
    /// A resonator at `formant`.
    pub fn new(formant: Formant) -> Self {
        let mut r = Self {
            formant,
            sample_rate: 48_000.0,
            a: 1.0,
            b: 0.0,
            c: 0.0,
            y: [0.0; 2],
        };
        r.set(formant);
        r
    }

    /// Retune (state kept): cheap enough per sample.
    pub fn set(&mut self, formant: Formant) {
        self.formant = formant;
        let t = core::f64::consts::PI / f64::from(self.sample_rate);
        let f = f64::from(formant.centre.get()).clamp(0.0, 0.5 * f64::from(self.sample_rate));
        let bw = f64::from(formant.bandwidth.get()).max(0.0);
        let r = (-t * bw).exp();
        self.c = -r * r;
        self.b = 2.0 * r * (2.0 * t * f).cos();
        self.a = 1.0 - self.b - self.c;
    }

    /// The formant it is set to.
    pub fn formant(&self) -> Formant {
        self.formant
    }

    /// `|H|` at `f` (Hz), from the coefficients.
    pub fn magnitude_at(&self, f: f32) -> f64 {
        let w = core::f64::consts::TAU * f64::from(f) / f64::from(self.sample_rate);
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let re = 1.0 - self.b * c1 - self.c * c2;
        let im = self.b * s1 + self.c * s2;
        self.a.abs() / (re * re + im * im).sqrt()
    }

    #[inline]
    fn tick64(&mut self, x: f64) -> f64 {
        let y = self.a * x + self.b * self.y[0] + self.c * self.y[1];
        self.y[1] = self.y[0];
        self.y[0] = denormal::flush64(y);
        y
    }
}

impl Stage for Resonator {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.set(self.formant);
        self.reset();
    }
    fn reset(&mut self) {
        self.y = [0.0; 2];
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick64(f64::from(x)) as f32
    }
    fn tail(&self) -> Tail {
        Tail::from_biquad(-self.b, -self.c)
    }
}

/// A vowel filter: three formant [`Resonator`]s in cascade, the output
/// scaled so its strongest formant peaks at 0 dB.
///
/// # Models
///
/// The cascade branch of Klatt's formant synthesiser (Klatt 1980): the
/// vocal tract as an all-pole filter, one resonator per formant. Fed a
/// glottal-like source (a sawtooth, a pulse train, a guitar) it imposes
/// the vowel. The vowels are Peterson & Barney's (1952) measured
/// averages ([`Vowel::formants`]); any other table works through
/// [`set`](Self::set).
///
/// # Derivation
///
/// A cascade of unity-DC resonators, so the formants' relative levels
/// fall out of the poles as in the tract (no per-formant amplitudes to
/// guess). That level is high at the first formant (+20 to +30 dB), so
/// the output is scaled by the inverse of the cascade's largest
/// magnitude at its three centres, computed from the coefficients at
/// each [`set`](Self::set).
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/formant/tests.rs::vowels_peak_at_the_table`:
/// for all 30 vowels of the table (men, women, children) every formant,
/// with Fant's bandwidths ([`bandwidths`]), shows as a peak of the
/// cascade's measured response within 2 % of its centre (1.26 %
/// worst), and the maximum is 0 dB within 0.3 dB (0.016 dB worst).
/// `crates/stave-dsp/src/primitives/filters/formant/tests.rs::the_table_is_peterson_and_barney`
/// holds all 90 centres to Peterson & Barney's printed Table II.
///
/// # Cost
///
/// `"Formants (3, Peterson & Barney)"` on the datasheet.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// Centres inside (0, 0.5·fs); retune per sample for a moving vowel
/// ([`Formant::toward`] morphs one into another).
///
/// ```
/// use stave_dsp::primitives::filters::formant::{Formants, Speaker, Vowel};
/// use stave_dsp::Stage;
///
/// let mut ah = Formants::new(Vowel::Aa.formants(Speaker::Man));
/// ah.prepare(48_000.0);
/// let y = ah.tick(0.5);
/// assert!(y.is_finite());
/// // Halfway to "ee".
/// ah.morph(Vowel::Aa.formants(Speaker::Man), Vowel::Iy.formants(Speaker::Man), 0.5);
/// ```
#[derive(Copy, Clone, Debug)]
pub struct Formants {
    sections: [Resonator; 3],
    gain: f64,
}

impl Formants {
    /// A filter at `formants`.
    pub fn new(formants: [Formant; 3]) -> Self {
        let mut f = Self {
            sections: formants.map(Resonator::new),
            gain: 1.0,
        };
        f.set(formants);
        f
    }

    /// Retune to `formants` (state kept).
    pub fn set(&mut self, formants: [Formant; 3]) {
        for (s, f) in self.sections.iter_mut().zip(formants) {
            s.set(f);
        }
        self.normalise();
    }

    /// Retune to `t` of the way from `from` to `to`.
    pub fn morph(&mut self, from: [Formant; 3], to: [Formant; 3], t: f32) {
        self.set(core::array::from_fn(|i| from[i].toward(to[i], t)));
    }

    /// The formants it is set to.
    pub fn formants(&self) -> [Formant; 3] {
        self.sections.map(|s| s.formant())
    }

    /// The cascade's `|H|` at `f` (Hz), before the output scaling.
    pub fn magnitude_at(&self, f: f32) -> f64 {
        self.sections.iter().map(|s| s.magnitude_at(f)).product()
    }

    fn normalise(&mut self) {
        let peak = self
            .sections
            .iter()
            .map(|s| self.magnitude_at(s.formant.centre.get()))
            .fold(0.0f64, f64::max);
        self.gain = if peak > 0.0 { 1.0 / peak } else { 1.0 };
    }
}

impl Stage for Formants {
    fn prepare(&mut self, sample_rate: f32) {
        for s in &mut self.sections {
            s.prepare(sample_rate);
        }
        self.normalise();
    }
    fn reset(&mut self) {
        for s in &mut self.sections {
            s.reset();
        }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = f64::from(x) * self.gain;
        for s in &mut self.sections {
            y = s.tick64(y);
        }
        y as f32
    }
    fn tail(&self) -> Tail {
        self.sections.iter().fold(Tail::None, |t, s| t.then(s.tail()))
    }
}

#[cfg(test)]
mod tests;
