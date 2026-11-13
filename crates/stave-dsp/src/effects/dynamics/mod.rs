//! Dynamics: level detectors ([`Peak`], windowed [`Rms`],
//! [`ProgramDependent`], stereo [`Link`] modes, the [`Sidechain`]
//! filter), the [`GainComputer`] and the [`Compressor`] in feed-forward
//! and feedback [`Topology`], and the look-ahead [`Limiter`] on sample or
//! true [`Peaks`], and the [`Multiband`] frame that runs any of them
//! (any [`Stage`](crate::Stage), [`Keyed`] for link and sidechain) per
//! band of a crossover.

mod compressor;
mod detectors;
mod limiter;
mod multiband;

pub use compressor::{Compressor, GainComputer, Topology};
pub use detectors::{Link, Peak, ProgramDependent, Rms, Sidechain};
pub use limiter::{Limiter, Peaks};
pub use multiband::{Keyed, Multiband};

#[cfg(test)]
mod tests;
