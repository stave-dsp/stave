//! The junction FET family: the [`Jfet`] trait and its published model,
//! [`ShichmanHodges`] (SPICE's JFET), N- or P-channel by [`Polarity`].
//!
//! # Parts
//!
//! Constants of [`ShichmanHodges`] named by part number, with the
//! values of a published SPICE card; a number that starts with a digit
//! takes an `N` in front (`ShichmanHodges::N2N5457`).

pub mod shichman_hodges;

pub use super::Polarity;
pub use shichman_hodges::ShichmanHodges;

/// The currents into a JFET's drain and gate (the source's is minus
/// their sum) and their partials, at the intrinsic gate–source and
/// gate–drain voltages — the two gate junctions'.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct JfetCurrents {
    pub drain: f64,
    pub gate: f64,
    pub drain_per_vgs: f64,
    pub drain_per_vgd: f64,
    pub gate_per_vgs: f64,
    pub gate_per_vgd: f64,
}

/// The JFET family: every JFET model implements it. Voltages are
/// terminal voltages at the intrinsic device (v(g) − v(s), v(g) − v(d));
/// currents flow into the terminal.
pub trait Jfet: Copy {
    fn polarity(&self) -> Polarity;

    /// Drain and gate currents and their partials at `vgs`, `vgd`.
    fn currents(&self, vgs: f64, vgd: f64) -> JfetCurrents;

    /// Bound a Newton step on `(vgs, vgd)` (SPICE's `pnjlim` on each
    /// gate junction).
    fn limit_step(&self, new: [f64; 2], old: [f64; 2]) -> [f64; 2];
}

#[cfg(test)]
mod tests;
