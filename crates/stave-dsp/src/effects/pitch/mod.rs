//! Pitch and time: the [`PhaseVocoder`] pitch shifter (Průša's
//! phase-gradient reconstruction by default, Laroche–Dolson identity
//! phase locking as the cheap mode, [`Reconstruction`]), offline
//! [`time_stretch`] on the same frame engine, and [`Psola`] for
//! monophonic sources.
//!
//! The vocoder runs its own analysis and synthesis hops (they differ
//! whenever the time scale changes), so it is built from the spectral
//! layer's parts — [`Fft`](crate::Fft) and
//! [`Window`](crate::primitives::spectral::Window) — rather than as an
//! [`Stft`](crate::primitives::spectral::Stft) closure, whose hops are
//! one; its overlap-add normalises by the summed squared window, the
//! weighted overlap-add the `Stft` checks for COLA.

mod heap;
mod psola;
mod shifter;
mod stretch;
mod vocoder;
mod yin;

pub use psola::Psola;
pub use shifter::PhaseVocoder;
pub use stretch::{time_stretch, Stretched};
pub use vocoder::Reconstruction;

#[cfg(test)]
mod tests;
