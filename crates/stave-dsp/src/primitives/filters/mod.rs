//! Filters: biquads, the state-variable filter, first-order sections,
//! allpasses, Linkwitz-Riley crossovers (the N-band [`crossover`], LR or
//! linear phase), [`formant`] (vowel) filters and the Hilbert pair (linear);
//! and the published nonlinear filter structures — the [`ladder`]s
//! ([`Huovilainen`](ladder::Huovilainen) transistor,
//! [`Stinchcombe`](ladder::Stinchcombe) diode), the
//! [`SallenKey`](sallen_key::SallenKey) and the
//! [`Korg35`](korg35::Korg35) — each a zero-delay-feedback
//! integration of its model with a drive and a resonance whose 1 is the
//! onset of self-oscillation. A structure that reduces a circuit is held
//! to that circuit's netlist in [`circuit::topologies`](crate::circuit::topologies).

pub mod allpass;
pub mod biquad;
pub mod crossover;
pub mod formant;
pub mod hilbert;
pub mod korg35;
pub mod ladder;
pub mod linkwitz_riley;
pub mod one_pole;
pub mod sallen_key;
pub mod svf;
mod zdf;

#[cfg(test)]
pub(crate) mod probe;
