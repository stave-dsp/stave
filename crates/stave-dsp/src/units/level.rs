//! Level: [`Db`] (logarithmic) and [`Gain`] (linear amplitude), two
//! types for two scales, so a dB value is never multiplied into a
//! sample.

use core::fmt;

unit! {
    /// A level in decibels, `f32`: the precision of the samples it ends
    /// up scaling and of every stave-dsp gain setter. Levels add
    /// (`db(-6.0) + db(3.0)`); a sample is scaled by its linear
    /// [`Gain`], [`Db::to_gain`].
    ///
    /// `-inf` dB is silence and allowed; NaN is a debug-build panic.
    ///
    /// ```
    /// use stave_dsp::units::{db, Gain};
    ///
    /// assert!((db(-6.0).to_gain().get() - 0.501).abs() < 1e-3);
    /// assert_eq!(Gain::new(1.0).to_db(), db(0.0));
    /// assert_eq!(db(-6.0).to_string(), "-6.0 dB");
    /// ```
    Db, f32, |v| !v.is_nan(), "Db is NaN"
}
linear!(Db, f32);

/// `v` decibels.
#[inline]
#[track_caller]
pub const fn db(v: f32) -> Db {
    Db::new(v)
}

impl Db {
    /// Unity: 0 dB.
    pub const UNITY: Db = Db(0.0);
    /// The level [`Gain::to_db`] reads silence as, −180 dB: the floor of
    /// `f32` audio (an amplitude of 1e-9), as [`amp_to_db`] has it.
    ///
    /// [`amp_to_db`]: crate::primitives::gain::amp_to_db
    pub const FLOOR: Db = Db(-180.0);

    /// The linear amplitude, `10^(dB/20)` (−inf dB is 0). Bit-identical
    /// to [`db_to_amp`].
    ///
    /// [`db_to_amp`]: crate::primitives::gain::db_to_amp
    #[inline]
    pub fn to_gain(self) -> Gain {
        Gain(10f32.powf(self.0 / 20.0))
    }
}

/// Reads as a stave-core `Unit::Db` param does: `-6.0 dB`; silence reads
/// `-inf dB`.
impl fmt::Display for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == f32::NEG_INFINITY {
            f.write_str("-inf dB")
        } else {
            write!(f, "{:.1} dB", self.0)
        }
    }
}

unit! {
    /// A linear amplitude multiplier, `f32` like the samples it scales:
    /// `gain * x` is a scaled sample, and gains in series multiply.
    /// Negative is a polarity flip and allowed; NaN is a debug-build
    /// panic.
    ///
    /// ```
    /// use stave_dsp::units::{db, Gain};
    ///
    /// let g = Gain::new(0.5);
    /// assert_eq!(g * 0.8, 0.4);
    /// assert_eq!(g * g, Gain::new(0.25));
    /// assert_eq!(Gain::new(0.0).to_db(), stave_dsp::units::Db::FLOOR);
    /// assert_eq!(g.to_string(), "×0.500");
    /// ```
    Gain, f32, |v| !v.is_nan(), "Gain is NaN"
}

impl Gain {
    /// Unity: ×1.
    pub const UNITY: Gain = Gain(1.0);
    /// Silence: ×0.
    pub const ZERO: Gain = Gain(0.0);

    /// The level of this amplitude's magnitude in dB, `20·log10|g|`; an
    /// amplitude at or below 1e-9 reads [`Db::FLOOR`]. Bit-identical to
    /// [`amp_to_db`] for positive gains.
    ///
    /// [`amp_to_db`]: crate::primitives::gain::amp_to_db
    #[inline]
    pub fn to_db(self) -> Db {
        let a = self.0.abs();
        if a <= 1e-9 {
            Db::FLOOR
        } else {
            Db(20.0 * a.log10())
        }
    }
}

/// Gains in series multiply.
impl core::ops::Mul for Gain {
    type Output = Gain;
    #[inline]
    fn mul(self, rhs: Gain) -> Gain {
        Gain(self.0 * rhs.0)
    }
}

impl core::ops::MulAssign for Gain {
    #[inline]
    fn mul_assign(&mut self, rhs: Gain) {
        self.0 *= rhs.0;
    }
}

/// A sample scaled by a gain.
impl core::ops::Mul<f32> for Gain {
    type Output = f32;
    #[inline]
    fn mul(self, x: f32) -> f32 {
        self.0 * x
    }
}

/// A sample scaled by a gain.
impl core::ops::Mul<Gain> for f32 {
    type Output = f32;
    #[inline]
    fn mul(self, g: Gain) -> f32 {
        self * g.0
    }
}

/// `×0.500`.
impl fmt::Display for Gain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "×{:.3}", self.0)
    }
}
