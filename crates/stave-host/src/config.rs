//! How the host runs what it loads. Every field is a type that cannot
//! hold an invalid value, so a `HostConfig` that exists is usable.

use std::num::NonZeroUsize;

use crate::plugin::TransportConfig;

/// Samples per second: positive and finite.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct SampleRate(f64);

#[derive(Copy, Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0} is not a sample rate: it must be positive and finite")]
pub struct InvalidSampleRate(pub f64);

impl SampleRate {
    pub const fn new(hz: f64) -> Result<Self, InvalidSampleRate> {
        if hz > 0.0 && hz < f64::INFINITY {
            Ok(Self(hz))
        } else {
            Err(InvalidSampleRate(hz))
        }
    }

    /// A rate known when compiling; an invalid one fails the build.
    pub const fn constant(hz: f64) -> Self {
        match Self::new(hz) {
            Ok(rate) => rate,
            Err(_) => panic!("not a sample rate"),
        }
    }

    pub const fn hz(self) -> f64 {
        self.0
    }
}

impl From<SampleRate> for f64 {
    fn from(rate: SampleRate) -> Self {
        rate.0
    }
}

impl TryFrom<f64> for SampleRate {
    type Error = InvalidSampleRate;

    fn try_from(hz: f64) -> Result<Self, Self::Error> {
        Self::new(hz)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct HostConfig {
    pub sample_rate: SampleRate,
    /// The largest block one process call carries.
    pub max_block: NonZeroUsize,
    /// The bus width the host gives a module; a plugin declares its own.
    pub channels: NonZeroUsize,
    pub transport: TransportConfig,
}

/// A non-zero count known when compiling; zero fails the build.
pub const fn non_zero(count: usize) -> NonZeroUsize {
    match NonZeroUsize::new(count) {
        Some(count) => count,
        None => panic!("a count that must not be zero is zero"),
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            sample_rate: SampleRate::constant(48_000.0),
            max_block: non_zero(256),
            channels: non_zero(2),
            transport: TransportConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_positive_finite_rates_are_sample_rates() {
        assert_eq!(SampleRate::new(44_100.0).map(SampleRate::hz), Ok(44_100.0));
        for bad in [0.0, -48_000.0, f64::NAN, f64::INFINITY] {
            assert!(SampleRate::try_from(bad).is_err(), "{bad}");
        }
    }
}
