//! Equalisers: parametric [`Band`]s as a [`MinimumPhase`] biquad
//! cascade, as one [`LinearPhase`] FIR, and as a [`Dynamic`] EQ whose
//! bands follow their own level, each minimum- or linear-phase.
//!
//! All three share the band's response: the cookbook biquad of
//! [`Band::spec`] at the prepared rate. The minimum-phase EQ runs it;
//! the linear-phase one designs a FIR with its magnitude by frequency
//! sampling ([`crate::primitives::spectral::FrequencySampling`]) and
//! convolves it ([`crate::primitives::spectral::Convolver`]), so the
//! two differ in phase only, within the FIR's stated budget.

mod band;
mod dynamic;
mod linear_phase;
mod minimum_phase;
mod section;

pub use band::Band;
pub use dynamic::{Detector, Dynamic, DynamicBand, Dynamics, Phase};
pub use linear_phase::LinearPhase;
pub use minimum_phase::MinimumPhase;

#[cfg(test)]
mod tests;
