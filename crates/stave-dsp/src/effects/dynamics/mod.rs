//! Dynamics: level detectors ([`Peak`], windowed [`Rms`],
//! [`ProgramDependent`], stereo [`Link`] modes, the [`Sidechain`]
//! filter), the [`GainComputer`] and the [`Compressor`] in feed-forward
//! and feedback [`Topology`], and the look-ahead [`Limiter`] on sample or
//! true [`Peaks`].

mod compressor;
mod detectors;
mod limiter;

pub use compressor::{Compressor, GainComputer, Topology};
pub use detectors::{Link, Peak, ProgramDependent, Rms, Sidechain};
pub use limiter::{Limiter, Peaks};

#[cfg(test)]
mod tests;
