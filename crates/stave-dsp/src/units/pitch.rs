//! Intervals: [`Ratio`], [`Semitones`] and [`Cents`].

use core::fmt;

unit! {
    /// A dimensionless `x:1` ratio, `f32`: a compressor's ratio (`4:1`),
    /// or the interval between two frequencies (`Hz / Hz`, and
    /// `Hz * Ratio` moves a frequency by it). `f32` resolves an interval
    /// to about 1e-4 cents, far below hearing, and matches [`Hz`].
    ///
    /// Positive; zero, negative and NaN are debug-build panics (`inf`
    /// is allowed: a limiter's ratio).
    ///
    /// ```
    /// use stave_dsp::units::{cents, hz, semitones, Ratio};
    ///
    /// assert_eq!(semitones(12.0).ratio(), Ratio::new(2.0));
    /// assert_eq!((hz(440.0) / hz(220.0)).semitones(), 12.0);
    /// assert!((cents(1200.0).ratio().get() - 2.0).abs() < 1e-6);
    /// assert_eq!(Ratio::new(4.0).to_string(), "4.0:1");
    /// ```
    ///
    /// [`Hz`]: super::Hz
    Ratio, f32, |v| v > 0.0, "Ratio is not positive"
}

impl Ratio {
    /// 1:1 — no compression, the unison interval.
    pub const UNITY: Ratio = Ratio(1.0);

    /// The interval in semitones, `12·log2(r)`.
    #[inline]
    pub fn semitones(self) -> f32 {
        12.0 * self.0.log2()
    }

    /// The interval in cents, `1200·log2(r)`.
    #[inline]
    pub fn cents(self) -> f32 {
        1200.0 * self.0.log2()
    }
}

/// Intervals stack by multiplying.
impl core::ops::Mul for Ratio {
    type Output = Ratio;
    #[inline]
    fn mul(self, rhs: Ratio) -> Ratio {
        Ratio(self.0 * rhs.0)
    }
}

/// The interval between two intervals.
impl core::ops::Div for Ratio {
    type Output = Ratio;
    #[inline]
    fn div(self, rhs: Ratio) -> Ratio {
        Ratio(self.0 / rhs.0)
    }
}

/// Reads as a stave-core `Unit::Ratio` param does: `4.0:1`.
impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}:1", self.0)
    }
}

unit! {
    /// An equal-tempered interval in semitones, `f32` (fractional:
    /// detune and glide), and its frequency ratio `2^(st/12)`. Intervals
    /// add.
    ///
    /// Finite; anything else is a debug-build panic.
    ///
    /// ```
    /// use stave_dsp::units::{cents, hz, semitones};
    ///
    /// assert_eq!(hz(440.0) * semitones(-12.0).ratio(), hz(220.0));
    /// assert_eq!(semitones(7.0) + cents(50.0).into(), semitones(7.5));
    /// assert_eq!(semitones(7.0).to_string(), "+7.00 st");
    /// ```
    Semitones, f32, |v| v.is_finite(), "Semitones is not finite"
}
linear!(Semitones, f32);

/// `v` semitones.
#[inline]
#[track_caller]
pub const fn semitones(v: f32) -> Semitones {
    Semitones::new(v)
}

impl Semitones {
    /// The frequency ratio, `2^(st/12)`.
    #[inline]
    pub fn ratio(self) -> Ratio {
        Ratio((self.0 / 12.0).exp2())
    }
}

impl From<Cents> for Semitones {
    #[inline]
    fn from(c: Cents) -> Semitones {
        Semitones(c.0 / 100.0)
    }
}

/// `+7.00 st`, signed.
impl fmt::Display for Semitones {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+.2} st", self.0)
    }
}

unit! {
    /// A fine interval in cents (1/100 semitone), `f32`, and its
    /// frequency ratio `2^(c/1200)`. Intervals add.
    ///
    /// Finite; anything else is a debug-build panic.
    ///
    /// ```
    /// use stave_dsp::units::{cents, semitones, Cents};
    ///
    /// assert_eq!(Cents::from(semitones(1.0)), cents(100.0));
    /// assert_eq!(cents(-5.0).to_string(), "-5.0 cents");
    /// ```
    Cents, f32, |v| v.is_finite(), "Cents is not finite"
}
linear!(Cents, f32);

/// `v` cents.
#[inline]
#[track_caller]
pub const fn cents(v: f32) -> Cents {
    Cents::new(v)
}

impl Cents {
    /// The frequency ratio, `2^(c/1200)`.
    #[inline]
    pub fn ratio(self) -> Ratio {
        Ratio((self.0 / 1200.0).exp2())
    }
}

impl From<Semitones> for Cents {
    #[inline]
    fn from(s: Semitones) -> Cents {
        Cents(s.0 * 100.0)
    }
}

/// `+5.0 cents`, signed.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+.1} cents", self.0)
    }
}
