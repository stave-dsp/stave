//! The MOSFET family: the [`Mosfet`] trait and its published model,
//! [`ShichmanHodges`] (the square law, SPICE's MOSFET level 1), N- or
//! P-channel by [`Polarity`]. EKV, for subthreshold conduction, is not
//! here yet: the square law conducts nothing below threshold.
//!
//! # Parts
//!
//! Constants of [`ShichmanHodges`] named by part number, with the
//! values of a published level-1 card and its width and length folded
//! into `beta` (= `KP·W/L`). An IC's two devices take `_N` and `_P`
//! (`ShichmanHodges::CD4007_N`).

pub mod shichman_hodges;

pub use super::Polarity;
pub use shichman_hodges::ShichmanHodges;

/// The drain current (into the drain, out of the source; the gate draws
/// none) and its partials, at the intrinsic gate–source, drain–source
/// and bulk–source voltages.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct MosfetCurrents {
    pub drain: f64,
    pub drain_per_vgs: f64,
    pub drain_per_vds: f64,
    pub drain_per_vbs: f64,
}

/// The MOSFET family: every MOSFET model implements it. Voltages are
/// terminal voltages at the intrinsic device; the current flows into the
/// drain.
pub trait Mosfet: Copy {
    fn polarity(&self) -> Polarity;

    /// The drain current and its partials at `vgs`, `vds`, `vbs`.
    fn currents(&self, vgs: f64, vds: f64, vbs: f64) -> MosfetCurrents;
}

#[cfg(test)]
mod tests;
