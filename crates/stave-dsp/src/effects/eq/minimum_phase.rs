//! The minimum-phase parametric EQ: the bands' biquads in series.

use super::band::Band;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::Biquad;

/// A minimum-phase parametric EQ: one cookbook [`Biquad`] per [`Band`],
/// in series — the reference the linear-phase and dynamic EQs are held
/// to.
///
/// # Models
///
/// The product of the bands' bilinear-transformed analog prototypes
/// (RBJ *Audio EQ Cookbook*): each band's magnitude with the least
/// delay any filter of that magnitude has, so no pre-ringing; the
/// phase turns around each band. No hardware claim.
///
/// # Derivation
///
/// [`Band::spec`] per band, as [`Biquad`]s in series (f64 state).
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/eq/tests.rs::minimum_phase_is_the_biquads`:
/// its measured response is the product of [`Band::gain_db`] within
/// 1e-3 dB from 20 Hz to 20 kHz at 44.1, 48 and 96 kHz (worst 6e-4, the
/// `f32` output's floor), and zero latency.
///
/// # Cost
///
/// Datasheet `"MinimumPhase EQ (5 bands)"`: one biquad per band.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// The [`Band`]s' (corners inside (1 Hz, 0.499·fs)). A
/// [`set_band`](Self::set_band) changes the coefficients at once; for a
/// gain that moves while audio plays, use [`super::Dynamic`], whose
/// minimum-phase bands modulate without clicks.
///
/// ```
/// use stave_dsp::effects::eq::{Band, MinimumPhase};
/// use stave_dsp::units::{db, hz};
/// use stave_dsp::Stage;
///
/// let mut eq = MinimumPhase::new(&[Band::HighShelf { frequency: hz(8_000.0), gain: db(3.0) }]);
/// eq.prepare(48_000.0);
/// assert_eq!(eq.latency(), 0);
/// assert!((eq.gain_db(20_000.0) - 3.0).abs() < 0.2);
/// ```
#[derive(Clone, Debug)]
pub struct MinimumPhase {
    bands: Vec<Band>,
    sections: Vec<Biquad>,
    sample_rate: f32,
}

impl MinimumPhase {
    /// An EQ of `bands`.
    pub fn new(bands: &[Band]) -> Self {
        Self {
            bands: bands.to_vec(),
            sections: bands.iter().map(|b| Biquad::new(b.spec())).collect(),
            sample_rate: 48_000.0,
        }
    }

    /// The bands.
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// Replaces band `index` (state kept). Realtime: never allocates.
    ///
    /// # Panics
    ///
    /// If `index` is out of range.
    pub fn set_band(&mut self, index: usize, band: Band) {
        self.bands[index] = band;
        self.sections[index].set(band.spec());
    }

    /// The EQ's gain at `hz` at the prepared rate, in dB.
    pub fn gain_db(&self, hz: f64) -> f64 {
        self.bands
            .iter()
            .map(|b| b.gain_db(hz, self.sample_rate))
            .sum()
    }
}

impl Stage for MinimumPhase {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for s in &mut self.sections {
            s.prepare(sample_rate);
        }
    }

    fn reset(&mut self) {
        for s in &mut self.sections {
            s.reset();
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.sections.iter_mut().fold(x, |y, s| s.tick(y))
    }

    /// The longest section's ring-out, summed over the series.
    fn tail(&self) -> Tail {
        self.sections
            .iter()
            .fold(Tail::None, |acc, s| acc.then(s.tail()))
    }
}
