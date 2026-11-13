//! A reverberation time per octave band: the decay profile the graphic-EQ
//! attenuation filters hold.

use crate::units::Seconds;

/// Octave bands in a [`T60`] profile.
pub const BANDS: usize = 10;

/// A reverberation time per octave band, 31.5 Hz to 16 kHz (ISO 266's
/// nominal octave centres; the exact centres are `1000·2ᵏ` Hz,
/// [`T60::CENTRES_HZ`]).
///
/// An infinite band never decays there; a reverb holding a profile with
/// every band infinite is frozen.
///
/// ```
/// use stave_dsp::effects::reverb::T60;
/// use stave_dsp::units::seconds;
///
/// // A hall: long lows, short highs.
/// let hall = T60::octaves([3.0, 2.8, 2.7, 2.55, 2.5, 2.5, 2.3, 1.9, 1.4, 1.2].map(seconds));
/// assert_eq!(hall.band(5), seconds(2.5)); // 1 kHz
/// assert_eq!(hall.longest(), seconds(3.0));
/// assert_eq!(T60::uniform(seconds(2.0)).band(9), seconds(2.0));
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct T60 {
    bands: [f32; BANDS],
}

impl T60 {
    /// Octave bands in a profile.
    pub const BANDS: usize = BANDS;

    /// The exact band centres, `1000·2ᵏ` Hz for `k = −5 … 4`.
    pub const CENTRES_HZ: [f32; BANDS] = [
        31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ];

    /// The same reverberation time in every band.
    pub fn uniform(t60: Seconds) -> Self {
        Self::octaves([t60; BANDS])
    }

    /// One reverberation time per octave band, lowest first.
    ///
    /// # Panics
    ///
    /// In debug builds, if a band is not positive.
    pub fn octaves(bands: [Seconds; BANDS]) -> Self {
        Self {
            bands: bands.map(|t| {
                debug_assert!(t.get() > 0.0, "T60 {} s", t.get());
                t.get() as f32
            }),
        }
    }

    /// Band `k`'s reverberation time (`k` from 0, 31.5 Hz, to 9, 16 kHz).
    pub fn band(&self, k: usize) -> Seconds {
        Seconds::new(f64::from(self.bands[k]))
    }

    /// The longest band: what sets the tail.
    pub fn longest(&self) -> Seconds {
        Seconds::new(f64::from(self.bands.iter().copied().fold(0.0, f32::max)))
    }

    /// The bands in seconds, as `f32`.
    pub(crate) fn seconds(&self) -> [f32; BANDS] {
        self.bands
    }
}
