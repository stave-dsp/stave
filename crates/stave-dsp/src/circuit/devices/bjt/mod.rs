//! The bipolar junction transistor family: the [`Bjt`] trait and its
//! published models, [`EbersMoll`] (transport form) and [`GummelPoon`]
//! (SPICE's BJT: Early effect, high-injection β roll-off, leakage
//! junctions, ohmic resistances), NPN or PNP by [`Polarity`].
//!
//! # Parts
//!
//! Parts are constants of [`GummelPoon`], named by part number with the
//! values of a published SPICE card. A part number that starts with a
//! digit takes an `N` in front (Rust names cannot start with one):
//! `GummelPoon::N2N3904` is the 2N3904; `GummelPoon::AC128` needs none.

pub mod ebers_moll;
pub mod gummel_poon;

pub use super::Polarity;
pub use ebers_moll::EbersMoll;
pub use gummel_poon::{ForwardTransit, GummelPoon, Substrate};
pub use super::capacitance::Depletion;

/// The currents into a BJT's collector and base (the emitter's is minus
/// their sum) and their partial derivatives, at the intrinsic
/// base–emitter and base–collector voltages.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct BjtCurrents {
    pub collector: f64,
    pub base: f64,
    pub collector_per_vbe: f64,
    pub collector_per_vbc: f64,
    pub base_per_vbe: f64,
    pub base_per_vbc: f64,
}

impl BjtCurrents {
    /// The P twin's currents from the N evaluation at negated voltages:
    /// currents negate, partials (two negations) do not.
    #[inline]
    pub(crate) fn mirrored(self, polarity: Polarity) -> Self {
        let s = polarity.sign();
        Self {
            collector: s * self.collector,
            base: s * self.base,
            ..self
        }
    }
}

/// The charges a BJT stores at its intrinsic junctions and their
/// partials (the base–emitter charge depends on vbc through the base
/// charge `qb`), at the intrinsic base–emitter and base–collector
/// voltages. Each charge is positive at the base for an NPN.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct BjtCharges {
    pub base_emitter: f64,
    pub base_emitter_per_vbe: f64,
    pub base_emitter_per_vbc: f64,
    pub base_collector: f64,
    pub base_collector_per_vbc: f64,
}

impl BjtCharges {
    /// The P twin's charges from the N evaluation: charges negate,
    /// capacitances do not.
    #[inline]
    pub(crate) fn mirrored(self, polarity: Polarity) -> Self {
        let s = polarity.sign();
        Self {
            base_emitter: s * self.base_emitter,
            base_collector: s * self.base_collector,
            ..self
        }
    }
}

/// The BJT family: every published BJT model implements it, so a
/// circuit takes any. Voltages are terminal voltages (v(b) − v(e),
/// v(b) − v(c)) at the intrinsic junctions, currents flow into the
/// terminal; a PNP's are negative in normal operation.
pub trait Bjt: Copy {
    fn polarity(&self) -> Polarity;

    /// Collector and base currents and their partials at `vbe`, `vbc`.
    fn currents(&self, vbe: f64, vbc: f64) -> BjtCurrents;

    /// Bound a Newton step on `(vbe, vbc)` from `old` to `new` without
    /// moving the solution (SPICE's `pnjlim` on each junction).
    fn limit_step(&self, new: [f64; 2], old: [f64; 2]) -> [f64; 2];
}

#[cfg(test)]
mod tests;
