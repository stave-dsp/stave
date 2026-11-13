//! **Layer 7 — effects.** Algorithmic designs that model no hardware:
//! [`reverb`] (FDN, Dattorro plate, Schroeder/Moorer) and [`dynamics`]
//! (detectors, gain computers, compressor, look-ahead limiter), [`modulation`]
//! (frequency shifter, single sideband); pitch,
//! granular and EQ to come.
//!
//! *Claim:* the design has the properties it states — a decay time in
//! seconds, a lossless feedback matrix, a gain computer's static curve,
//! a ceiling — and nothing about hardware.
//!
//! *Verified by:* analytic properties (energy, decay, static curves,
//! measured true peak) and golden audio at fixed settings.
//!
//! It may use every layer above it.

pub mod dynamics;
pub mod modulation;
pub mod reverb;
