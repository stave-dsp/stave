//! **Layer 5 — physical.** Models derived from mechanics or acoustics
//! rather than a schematic: springs, plates, rotors, tape transport,
//! loudspeakers, rooms and instruments.
//!
//! *Claim:* the model solves the stated physics (a dispersive spring,
//! a thin plate's modes, a rotor's Doppler) to a stated accuracy.
//!
//! *Verified by:* published modes and measurements, high-precision
//! solutions of the same equations, and hardware captures, each with an
//! error budget.
//!
//! | Module | Models |
//! |---|---|
//! | [`spring`] | [`Tank`](spring::Tank), [`Spring`](spring::Spring) (Välimäki, Parker & Abel's dispersive chirps as waveguides), [`Chirp`](spring::Chirp) |
//!
//! It may use `units`, `kernel`, `primitives`, `behavioral` and
//! `circuit`. A bucket-brigade delay is not here: the chip is a
//! schematic part, so it lives in
//! [`circuit::devices::bbd`](crate::circuit::devices::bbd) though its
//! transfer is a sampling process.

pub mod spring;
