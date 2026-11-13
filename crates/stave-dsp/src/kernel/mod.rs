//! **Layer 1 — the kernel.** The processing contract ([`Stage`],
//! [`Tail`]) and the forms every DSP graph is built from: [`Serial`]
//! composition, parallel [`Par2`] with its latency [`Compensator`],
//! feedback through an explicit delay ([`Loop`]), a 2× clock domain
//! ([`Oversampled2x`], and [`Oversampled`] at any factor and either
//! phase), channel lanes ([`PerChannel`]), control routing
//! ([`Router`]) and event-keyed voices ([`VoiceRack`]), plus the
//! denormal/non-finite guards every recursion uses. The kernel depends
//! on nothing above it.
//!
//! [`Stage`]: node::Stage
//! [`Tail`]: node::Tail
//! [`Serial`]: sequence::Serial
//! [`Par2`]: parallel::Par2
//! [`Compensator`]: parallel::Compensator
//! [`Loop`]: feedback::Loop
//! [`Oversampled2x`]: oversampled::Oversampled2x
//! [`Oversampled`]: oversampled::Oversampled
//! [`PerChannel`]: lanes::PerChannel
//! [`Router`]: control::Router
//! [`VoiceRack`]: voices::VoiceRack

pub mod control;
pub mod denormal;
pub mod feedback;
pub mod lanes;
pub mod node;
pub mod oversampled;
pub mod parallel;
pub(crate) mod resample;
pub(crate) mod ring;
pub mod sequence;
pub(crate) mod simd;
pub mod voices;
