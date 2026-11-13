//! One parametric band, and the response evaluation the linear-phase
//! designs share.

use crate::kernel::node::Stage;
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::units::{Db, Hz};

/// One band of a parametric EQ, in musical units. Its response is the
/// cookbook biquad's ([`FilterSpec`]) at the prepared rate: the
/// minimum-phase EQ runs that biquad, the linear-phase one designs a
/// FIR with its magnitude.
///
/// ```
/// use stave_dsp::effects::eq::Band;
/// use stave_dsp::units::{db, hz};
///
/// let bell = Band::Bell { frequency: hz(1_000.0), gain: db(6.0), q: 1.0 };
/// // The bell's centre gain, at 48 kHz.
/// assert!((bell.gain_db(1_000.0, 48_000.0) - 6.0).abs() < 1e-3);
/// assert_eq!(bell.with_gain(db(-3.0)).gain(), Some(db(-3.0)));
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Band {
    /// A peaking bell (RBJ `peakingEQ`): `gain` at `frequency`, width by
    /// `q`.
    Bell {
        /// The centre.
        frequency: Hz,
        /// The gain at the centre.
        gain: Db,
        /// The quality factor (higher is narrower).
        q: f32,
    },
    /// A low shelf (RBJ, slope 1): `gain` below `frequency`, half of it
    /// (in dB) at `frequency`.
    LowShelf {
        /// The midpoint.
        frequency: Hz,
        /// The gain below it.
        gain: Db,
    },
    /// A high shelf (RBJ, slope 1): `gain` above `frequency`.
    HighShelf {
        /// The midpoint.
        frequency: Hz,
        /// The gain above it.
        gain: Db,
    },
    /// A 12 dB/octave high-pass at `frequency` (`q` = 1/√2 is
    /// Butterworth).
    LowCut {
        /// The corner.
        frequency: Hz,
        /// The quality factor.
        q: f32,
    },
    /// A 12 dB/octave low-pass at `frequency`.
    HighCut {
        /// The corner.
        frequency: Hz,
        /// The quality factor.
        q: f32,
    },
}

/// The shelves' quality factor: RBJ slope `S = 1`.
pub(super) const SHELF_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

impl Band {
    /// The biquad this band is.
    pub fn spec(self) -> FilterSpec {
        match self {
            Band::Bell { frequency, gain, q } => FilterSpec::Peak {
                hz: frequency.get(),
                db: gain.get(),
                q,
            },
            Band::LowShelf { frequency, gain } => FilterSpec::LowShelfRbj {
                hz: frequency.get(),
                db: gain.get(),
            },
            Band::HighShelf { frequency, gain } => FilterSpec::HighShelfRbj {
                hz: frequency.get(),
                db: gain.get(),
            },
            Band::LowCut { frequency, q } => FilterSpec::Highpass {
                hz: frequency.get(),
                q,
            },
            Band::HighCut { frequency, q } => FilterSpec::Lowpass {
                hz: frequency.get(),
                q,
            },
        }
    }

    /// The band's gain, for the bands that have one (bells and shelves).
    pub fn gain(self) -> Option<Db> {
        match self {
            Band::Bell { gain, .. } | Band::LowShelf { gain, .. } | Band::HighShelf { gain, .. } => {
                Some(gain)
            }
            Band::LowCut { .. } | Band::HighCut { .. } => None,
        }
    }

    /// The same band at another gain (a cut is returned unchanged).
    pub fn with_gain(self, gain: Db) -> Self {
        match self {
            Band::Bell { frequency, q, .. } => Band::Bell { frequency, gain, q },
            Band::LowShelf { frequency, .. } => Band::LowShelf { frequency, gain },
            Band::HighShelf { frequency, .. } => Band::HighShelf { frequency, gain },
            cut => cut,
        }
    }

    /// The band's corner or centre frequency.
    pub fn frequency(self) -> Hz {
        match self {
            Band::Bell { frequency, .. }
            | Band::LowShelf { frequency, .. }
            | Band::HighShelf { frequency, .. }
            | Band::LowCut { frequency, .. }
            | Band::HighCut { frequency, .. } => frequency,
        }
    }

    /// The biquad's gain at `hz`, at `sample_rate`, in dB: the response
    /// both EQs are held to.
    pub fn gain_db(self, hz: f64, sample_rate: f32) -> f64 {
        let (b, a) = coefficients(self, sample_rate);
        let w = core::f64::consts::TAU * hz / f64::from(sample_rate);
        10.0 * power(b, a, w.cos(), (2.0 * w).cos()).log10()
    }
}

/// A band's normalised biquad coefficients at `sample_rate`.
pub(super) fn coefficients(band: Band, sample_rate: f32) -> ([f64; 3], [f64; 2]) {
    let mut section = Biquad::new(band.spec());
    section.prepare(sample_rate);
    section.coefficients()
}

/// `|H(e^{jω})|²` of a normalised biquad from `cos ω` and `cos 2ω`:
/// `|b0 + b1·z⁻¹ + b2·z⁻²|²` over `|1 + a1·z⁻¹ + a2·z⁻²|²`, each
/// `c0 + 2·c1·cos ω + 2·c2·cos 2ω` in its autocorrelation.
#[inline]
pub(super) fn power(b: [f64; 3], a: [f64; 2], c1: f64, c2: f64) -> f64 {
    let num = b[0] * b[0] + b[1] * b[1] + b[2] * b[2]
        + 2.0 * (b[0] * b[1] + b[1] * b[2]) * c1
        + 2.0 * b[0] * b[2] * c2;
    let den = 1.0 + a[0] * a[0] + a[1] * a[1] + 2.0 * (a[0] + a[0] * a[1]) * c1 + 2.0 * a[1] * c2;
    (num / den.max(1e-300)).max(0.0)
}

/// A frequency grid with `cos ω` and `cos 2ω` tabulated, and the
/// sections of a set of bands at one rate: evaluates products of band
/// magnitudes without allocating or a transcendental per point.
#[derive(Clone, Debug)]
pub(super) struct Grid {
    cos1: Vec<f64>,
    cos2: Vec<f64>,
    /// `([b], [a])` per band at the prepared rate.
    sections: Vec<([f64; 3], [f64; 2])>,
}

impl Grid {
    /// A grid of `points` from DC to Nyquist inclusive, room for
    /// `bands` sections.
    pub(super) fn new(points: usize, bands: usize) -> Self {
        let step = core::f64::consts::PI / (points - 1).max(1) as f64;
        Self {
            cos1: (0..points).map(|k| (k as f64 * step).cos()).collect(),
            cos2: (0..points).map(|k| (2.0 * k as f64 * step).cos()).collect(),
            sections: Vec::with_capacity(bands),
        }
    }

    /// The number of grid points.
    pub(super) fn points(&self) -> usize {
        self.cos1.len()
    }

    /// Sets the sections to `bands` at `sample_rate`. Never allocates
    /// within the capacity given to [`new`](Self::new).
    pub(super) fn set(&mut self, bands: impl Iterator<Item = Band>, sample_rate: f32) {
        self.sections.clear();
        for band in bands {
            self.sections.push(coefficients(band, sample_rate));
        }
    }

    /// The product of the sections' magnitudes at grid point `k`.
    #[inline]
    pub(super) fn magnitude(&self, k: usize) -> f64 {
        let (c1, c2) = (self.cos1[k], self.cos2[k]);
        self.sections
            .iter()
            .map(|&(b, a)| power(b, a, c1, c2))
            .product::<f64>()
            .sqrt()
    }

    /// The magnitude of the biquad `(b, a)` at grid point `k`.
    #[inline]
    pub(super) fn magnitude_of(&self, (b, a): ([f64; 3], [f64; 2]), k: usize) -> f64 {
        power(b, a, self.cos1[k], self.cos2[k]).sqrt()
    }

    /// One section's magnitude at grid point `k`.
    #[inline]
    pub(super) fn section(&self, index: usize, k: usize) -> f64 {
        let (b, a) = self.sections[index];
        power(b, a, self.cos1[k], self.cos2[k]).sqrt()
    }
}
