//! **Layer 1 — primitives.** Linear, stateful building blocks grouped by
//! family: filters, time (delay lines and recirculating delays),
//! control (generators, followers, smoothers), oscillators, gain and
//! pitch helpers, randomness. Depends on the kernel only.

pub mod control;
pub mod filters;
pub mod gain;
pub mod osc;
pub mod pitch;
pub mod random;
#[cfg(test)]
pub(crate) mod resample;
pub mod solve;
pub mod time;
