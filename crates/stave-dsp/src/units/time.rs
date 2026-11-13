//! Time: [`Seconds`] and [`Samples`].

use core::fmt;

use super::frequency::{Hz, SampleRate};
use super::text;

unit! {
    /// A duration in seconds, `f64`: a duration becomes a sample count
    /// by multiplying by the rate, and `f32` stops counting whole samples
    /// past 2²⁴ (22 seconds at 768 kHz), so a long reverb tail or a slow
    /// LFO period would land on the wrong sample. It is also the
    /// precision of stave-core's smoothing times. Milliseconds are a
    /// constructor, [`ms`], not a type.
    ///
    /// Negative durations are allowed (an offset); NaN is a debug-build
    /// panic.
    ///
    /// ```
    /// use stave_dsp::units::{ms, seconds, SampleRate};
    ///
    /// assert_eq!(ms(250.0), seconds(0.25));
    /// assert_eq!(seconds(1.5).samples(SampleRate::new(44_100.0)).get(), 66_150.0);
    /// assert_eq!(ms(30.0).to_string(), "30.0 ms");
    /// assert_eq!(seconds(1.25).to_string(), "1.25 s");
    /// ```
    Seconds, f64, |v| !v.is_nan(), "Seconds is NaN"
}
linear!(Seconds, f64);

/// `v` seconds.
#[inline]
#[track_caller]
pub const fn seconds(v: f64) -> Seconds {
    Seconds::new(v)
}

/// `v` milliseconds, as [`Seconds`].
#[inline]
#[track_caller]
pub const fn ms(v: f64) -> Seconds {
    Seconds::new(v / 1e3)
}

/// `v` microseconds, as [`Seconds`].
#[inline]
#[track_caller]
pub const fn us(v: f64) -> Seconds {
    Seconds::new(v / 1e6)
}

impl Seconds {
    /// The duration in milliseconds.
    #[inline]
    pub const fn to_ms(self) -> f64 {
        self.0 * 1e3
    }

    /// The duration as a sample count at `sample_rate` (fractional: a delay can
    /// sit between samples).
    #[inline]
    pub fn samples(self, sample_rate: SampleRate) -> Samples {
        Samples(self.0 * sample_rate.0 as f64)
    }

    /// The frequency whose period this is, `1 / t` ([`Hz::period`]'s
    /// inverse). A zero duration is a debug-build panic.
    #[inline]
    #[track_caller]
    pub fn frequency(self) -> Hz {
        debug_assert!(self.0 != 0.0, "the frequency of a zero period");
        Hz::new((1.0 / self.0) as f32)
    }
}

/// Reads as a stave-core `Unit::Ms` param does: `30.0 ms`, `1.25 s`.
impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::switching(f, self.to_ms(), "ms", "s")
    }
}

unit! {
    /// A duration in samples, `f64`: fractional (a modulated delay reads
    /// between samples) and exact as a whole count far past what `f32`
    /// holds (2²⁴). [`frames`](Samples::frames) rounds it to the `usize`
    /// count buffers are indexed by.
    ///
    /// ```
    /// use stave_dsp::units::{samples, SampleRate};
    ///
    /// let sample_rate = SampleRate::new(48_000.0);
    /// assert_eq!(samples(480.0).seconds(sample_rate).to_ms(), 10.0);
    /// assert_eq!(samples(2.6).frames(), 3);
    /// assert_eq!(samples(-1.0).frames(), 0);
    /// ```
    Samples, f64, |v| !v.is_nan(), "Samples is NaN"
}
linear!(Samples, f64);

/// `v` samples.
#[inline]
#[track_caller]
pub const fn samples(v: f64) -> Samples {
    Samples::new(v)
}

impl Samples {
    /// A whole count of frames.
    #[inline]
    pub const fn of_frames(n: usize) -> Samples {
        Samples(n as f64)
    }

    /// The count rounded to the nearest whole frame; negative and NaN
    /// counts are 0, and counts past `usize::MAX` saturate. Never panics.
    #[inline]
    pub fn frames(self) -> usize {
        // `as` saturates and maps NaN to 0.
        (self.0 + 0.5).floor().max(0.0) as usize
    }

    /// The duration at `sample_rate`.
    #[inline]
    pub fn seconds(self, sample_rate: SampleRate) -> Seconds {
        Seconds(self.0 / sample_rate.0 as f64)
    }
}

/// `480.00 samples`.
impl fmt::Display for Samples {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2} samples", self.0)
    }
}
