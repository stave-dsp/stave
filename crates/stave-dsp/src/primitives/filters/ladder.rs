//! Ladder filters as their published structural models: the transistor
//! ladder ([`Huovilainen`], the Moog) and the diode ladder
//! ([`Stinchcombe`], the TB-303). Each is four `tanh` stages integrated
//! by the trapezoidal rule with zero-delay feedback, Newton solving each
//! sample's implicit step — the cost of a structure, not of a circuit.
//! The circuits they reduce are netlists in
//! [`circuit::topologies`](crate::circuit::topologies)
//! ([`transistor_ladder`](crate::circuit::topologies::transistor_ladder),
//! [`diode_ladder`](crate::circuit::topologies::diode_ladder)), and each
//! model is held to its circuit through `Circuit<Dk>`.
//!
//! Both take the same knobs as every nonlinear filter here: a cutoff in
//! [`Hz`](crate::units::Hz) that is the self-oscillation pitch, a
//! resonance where 1 is the onset of self-oscillation, and a drive in
//! [`Db`](crate::units::Db) into the stages' `tanh` (the output is
//! divided by it, so the small-signal level does not move with drive).
//!
//! ```
//! use stave_dsp::primitives::filters::ladder::Huovilainen;
//! use stave_dsp::units::{db, hz};
//! use stave_dsp::{Oversampled2x, Stage};
//!
//! let mut ladder = Huovilainen::new(hz(800.0), 0.7);
//! ladder.set_drive(db(12.0));
//! // Its anti-aliasing: four times the host rate.
//! let mut filter = Oversampled2x::front(Oversampled2x::nested(ladder));
//! filter.prepare(48_000.0);
//! let y: Vec<f32> = (0..480).map(|n| filter.tick(if n % 120 < 60 { 0.5 } else { -0.5 })).collect();
//! assert!(y.iter().all(|v| v.is_finite()));
//! assert_eq!(filter.inner.inner.stats().capped, 0);
//! ```

mod huovilainen;
mod stinchcombe;

pub use huovilainen::Huovilainen;
pub use stinchcombe::Stinchcombe;

#[cfg(test)]
mod tests;
