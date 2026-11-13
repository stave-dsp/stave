//! The Ebers–Moll BJT (Ebers & Moll 1954) in its transport form: two
//! ideal junctions coupled by forward and reverse transport.
//!
//! ```text
//!     Ibe = Is·(exp(vbe/Vt) − 1)      Ibc = Is·(exp(vbc/Vt) − 1)
//!     Ic  = Ibe − Ibc − Ibc/Br
//!     Ib  = Ibe/Bf + Ibc/Br
//! ```

use super::{Bjt, BjtCurrents, GummelPoon, Polarity};
use crate::circuit::devices::diode::THERMAL_VOLTS_27C;
use crate::circuit::devices::junction;

/// An Ebers–Moll transistor: saturation current and the two betas.
///
/// # Models
///
/// A BJT's static currents with no Early effect, high-injection
/// roll-off, leakage or ohmic resistance: the first-order model,
/// cheaper than [`GummelPoon`] (two exponentials, no base charge), right
/// at low currents and modest collector swings.
///
/// # Derivation
///
/// The module's transport equations: [`GummelPoon`] with `VAF`, `VAR`,
/// `IKF`, `IKR` infinite, `ISE`, `ISC` and the resistances zero, and
/// `NF = NR = 1`. `GummelPoon::from(em)` is that card.
///
/// # Verified
///
/// `crates/stave-test/tests/transistors_match_ngspice.rs::ebers_moll_matches_ngspice` runs the
/// `bjt_family_em` deck, a card with only `IS`, `BF`, `BR` that reads as
/// `GummelPoon::from` this model, against ngspice at 5e-7 normalized RMS
/// (measured 1.5e-7);
/// `crates/stave-dsp/src/circuit/devices/bjt/tests.rs::ebers_moll_is_gummel_poon_s_limit`
/// holds its currents to that card's to 1e-12.
///
/// # Cost
///
/// Two exponentials an evaluation, below [`GummelPoon`]'s; no datasheet
/// line of its own.
///
/// # Aliasing
///
/// None of its own (a memoryless device).
///
/// # Valid ranges
///
/// Low injection, collector swings small beside the Early voltage.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EbersMoll {
    pub polarity: Polarity,
    /// `IS` (A).
    pub saturation_amps: f64,
    /// `BF`.
    pub forward_beta: f64,
    /// `BR`.
    pub reverse_beta: f64,
    /// kT/q (V).
    pub thermal_volts: f64,
}

impl EbersMoll {
    /// SPICE's default transistor: `IS = 1e-16`, `BF = 100`, `BR = 1`, NPN.
    pub const DEFAULT_NPN: Self = Self {
        polarity: Polarity::N,
        saturation_amps: 1e-16,
        forward_beta: 100.0,
        reverse_beta: 1.0,
        thermal_volts: THERMAL_VOLTS_27C,
    };
}

impl From<EbersMoll> for GummelPoon {
    fn from(em: EbersMoll) -> Self {
        Self {
            polarity: em.polarity,
            saturation_amps: em.saturation_amps,
            forward_beta: em.forward_beta,
            reverse_beta: em.reverse_beta,
            thermal_volts: em.thermal_volts,
            ..GummelPoon::DEFAULT_NPN
        }
    }
}

impl Bjt for EbersMoll {
    fn polarity(&self) -> Polarity {
        self.polarity
    }

    #[inline]
    fn currents(&self, vbe: f64, vbc: f64) -> BjtCurrents {
        let s = self.polarity.sign();
        let (is, vt) = (self.saturation_amps, self.thermal_volts);
        let (ibe, gbe) = junction::current_and_conductance(s * vbe, is, vt);
        let (ibc, gbc) = junction::current_and_conductance(s * vbc, is, vt);
        let (bf, br) = (self.forward_beta, self.reverse_beta);
        BjtCurrents {
            collector: ibe - ibc - ibc / br,
            base: ibe / bf + ibc / br,
            collector_per_vbe: gbe,
            collector_per_vbc: -gbc - gbc / br,
            base_per_vbe: gbe / bf,
            base_per_vbc: gbc / br,
        }
        .mirrored(self.polarity)
    }

    #[inline]
    fn limit_step(&self, new: [f64; 2], old: [f64; 2]) -> [f64; 2] {
        let s = self.polarity.sign();
        let (is, vt) = (self.saturation_amps, self.thermal_volts);
        [0, 1].map(|j| s * junction::limit_step(s * new[j], s * old[j], is, vt))
    }
}
