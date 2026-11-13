//! **Layer 0 — units.** Quantities as types: `Hz`, `Db`, `Seconds`,
//! `Ohms` and the rest, so a frequency cannot be passed where a level
//! belongs and a datasheet value reads as written.
//!
//! *Claim:* a unit type converts exactly as its definition says
//! (`Db::to_gain`, `Seconds::samples(rate)`) and costs nothing at run
//! time.
//!
//! *Verified by:* unit tests of every conversion and its inverse, and
//! Kani proofs of the pure conversions where they can overflow or round.
//!
//! Empty until its first type lands (DSP.md U1–U3). It depends on
//! nothing, and every other layer may use it.
