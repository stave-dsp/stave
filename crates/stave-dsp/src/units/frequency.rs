//! Frequency: [`Hz`] and the prepared [`SampleRate`].

use core::fmt;

use super::pitch::Ratio;
use super::text;
use super::time::Seconds;

unit! {
    /// A frequency in hertz, `f32`: the precision of every stave-dsp
    /// setter and of [`SampleRate`], so passing one through is lossless.
    /// Work that accumulates (oscillator phase) asks for its step in
    /// `f64` with [`Hz::cycles_per_sample`].
    ///
    /// Negative frequencies are allowed (through-zero FM); NaN is a
    /// debug-build panic.
    ///
    /// ```
    /// use stave_dsp::units::{hz, khz, Ratio};
    ///
    /// assert_eq!(khz(2.4), hz(2400.0));
    /// assert_eq!(hz(220.0) * Ratio::new(2.0), hz(440.0));
    /// assert_eq!(hz(500.0).period().to_ms(), 2.0);
    /// assert_eq!(hz(2400.0).to_string(), "2.40 kHz");
    /// ```
    Hz, f32, |v| !v.is_nan(), "Hz is NaN"
}
linear!(Hz, f32, quotient_elsewhere);

/// `v` hertz.
#[inline]
#[track_caller]
pub const fn hz(v: f32) -> Hz {
    Hz::new(v)
}

/// `v` kilohertz.
#[inline]
#[track_caller]
pub const fn khz(v: f32) -> Hz {
    Hz::new(v * 1e3)
}

impl Hz {
    /// The duration of one cycle, `1 / f`, in `f64`. Zero hertz has an
    /// infinite period (a debug-build panic: it is never meant).
    #[inline]
    #[track_caller]
    pub fn period(self) -> Seconds {
        debug_assert!(self.0 != 0.0, "the period of 0 Hz");
        Seconds::new(1.0 / self.0 as f64)
    }

    /// Cycles per sample at `sample_rate`, `f / sample_rate` in `f64`: an
    /// oscillator's phase step (phase is in cycles).
    #[inline]
    pub fn cycles_per_sample(self, sample_rate: SampleRate) -> f64 {
        self.0 as f64 / sample_rate.0 as f64
    }
}

/// The interval from one frequency to another: `b / a` is how far `b`
/// lies above `a` (`(hz(440.0) / hz(220.0)).semitones() == 12.0`).
impl core::ops::Div<Hz> for Hz {
    type Output = Ratio;
    #[inline]
    fn div(self, rhs: Hz) -> Ratio {
        Ratio(self.0 / rhs.0)
    }
}

/// A frequency moved by an interval.
impl core::ops::Mul<Ratio> for Hz {
    type Output = Hz;
    #[inline]
    fn mul(self, r: Ratio) -> Hz {
        Hz(self.0 * r.0)
    }
}

/// A frequency moved down by an interval.
impl core::ops::Div<Ratio> for Hz {
    type Output = Hz;
    #[inline]
    fn div(self, r: Ratio) -> Hz {
        Hz(self.0 / r.0)
    }
}

/// Reads as a stave-core `Unit::Hz` param does: `440.0 Hz`, `2.40 kHz`.
impl fmt::Display for Hz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::switching(f, self.0 as f64, "Hz", "kHz")
    }
}

unit! {
    /// The sample rate a stage was prepared at, in hertz, `f32` (the
    /// crate's sample-rate type everywhere: `Stage::prepare` and
    /// stave-core's `Environment` carry it as `f32`, which is exact for
    /// every rate a host uses). The one prepared-rate type: rate-derived
    /// conversions ([`Seconds::samples`], [`Hz::cycles_per_sample`]) take
    /// it, never a loose `f32`. stave-core converts an `Environment` into
    /// one (`SampleRate::from(env)`).
    ///
    /// Finite and positive; anything else is a debug-build panic.
    ///
    /// ```
    /// use stave_dsp::units::{ms, SampleRate};
    ///
    /// let sample_rate = SampleRate::new(48_000.0);
    /// assert_eq!(ms(10.0).samples(sample_rate).get(), 480.0);
    /// assert_eq!(sample_rate.nyquist().get(), 24_000.0);
    /// assert_eq!(sample_rate.to_string(), "48.00 kHz");
    /// ```
    SampleRate, f32, |v| v > 0.0 && v.is_finite(), "SampleRate is not finite and positive"
}

impl SampleRate {
    /// The rate as a frequency.
    #[inline]
    pub const fn hz(self) -> Hz {
        Hz(self.0)
    }

    /// Half the rate: the highest frequency it can carry.
    #[inline]
    pub const fn nyquist(self) -> Hz {
        Hz(self.0 * 0.5)
    }

    /// The duration of one sample, in `f64`.
    #[inline]
    pub fn period(self) -> Seconds {
        Seconds::new(1.0 / self.0 as f64)
    }

    /// The rate inside a region running `factor`× faster (an
    /// oversampled subtree), as stave-core's `Environment::region`.
    #[inline]
    pub const fn region(self, factor: usize) -> SampleRate {
        SampleRate(self.0 * factor as f32)
    }
}

/// Reads as the frequency it is: `48.00 kHz`.
impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.hz().fmt(f)
    }
}
