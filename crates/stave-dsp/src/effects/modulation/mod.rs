//! Modulation on the analytic signal: the [`FrequencyShifter`] (every
//! partial moved by the same hertz, the Bode shifter's sound) and
//! [`SingleSideband`] modulation of a signal by any carrier, both on
//! the [`Hilbert`](crate::primitives::filters::hilbert::Hilbert) pair.
//!
//! ```
//! use stave_dsp::effects::modulation::FrequencyShifter;
//! use stave_dsp::units::hz;
//! use stave_dsp::Stage;
//!
//! let mut up = FrequencyShifter::new(hz(100.0));
//! up.prepare(48_000.0);
//! // 1 kHz in, 1.1 kHz out: once the allpasses settle, a steady tone
//! // (a 900 Hz image would beat against it).
//! let y: Vec<f32> = (0..9_600)
//!     .map(|n| up.tick((std::f32::consts::TAU * 1_000.0 * n as f32 / 48_000.0).sin()))
//!     .collect();
//! let peak = y[4_800..].iter().fold(0.0f32, |p, v| p.max(v.abs()));
//! assert!((peak - 1.0).abs() < 0.01, "{peak}");
//! ```

mod frequency_shifter;
mod single_sideband;

pub use frequency_shifter::FrequencyShifter;
pub use single_sideband::{Sideband, SingleSideband};

#[cfg(test)]
mod tests;
