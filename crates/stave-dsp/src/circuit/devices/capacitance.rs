//! Charge stored in a semiconductor junction, as SPICE's device loads
//! evaluate it: the depletion (space-charge) charge of a graded junction,
//! its linear continuation past the forward-bias corner, and the
//! substrate junction's variant. A device adds its transit-time
//! (diffusion) charge to this itself, since that charge follows its
//! currents.
//!
//! ```text
//!     v < FC·VJ:  q(v) = VJ·CJ0·(1 − (1 − v/VJ)^(1−M)) / (1 − M)
//!                 C(v) = CJ0·(1 − v/VJ)^(−M)
//!     v ≥ FC·VJ:  q(v) = CJ0·F1 + CJ0/F2·(F3·(v − FC·VJ) + M/(2·VJ)·(v² − (FC·VJ)²))
//!                 C(v) = CJ0/F2·(F3 + M·v/VJ)
//!     F1 = VJ·(1 − (1 − FC)^(1−M))/(1 − M),  F2 = (1 − FC)^(1+M),  F3 = 1 − FC·(1 + M)
//! ```
//!
//! Above the corner the capacitance is the tangent line of the depletion
//! law, so `q` and `C` are continuous and finite at any forward bias.

/// A junction's depletion capacitance as a SPICE card gives it: `CJO`
/// (or `CJE`, `CJC`, `CGS`, `CBD` …), the built-in potential `VJ`, the
/// grading coefficient `M`, and the forward-bias corner coefficient `FC`.
///
/// # Models
///
/// The charge of a graded pn junction's depletion layer (an abrupt
/// junction has `M` = ½, a linearly graded one ⅓), continued linearly
/// past `FC·VJ` where the law would diverge.
///
/// # Derivation
///
/// The module's equations, as ngspice-42 evaluates them in `dioload.c`
/// (diode), `bjtload.c` (base–emitter, base–collector, base–external
/// collector), `jfetload.c` (gate junctions, `M` fixed at ½) and
/// `mos1load.c` (bulk junctions). Temperature scaling of `CJ0` and `VJ`
/// is the identity at the card's temperature, the only one modelled.
///
/// # Verified
///
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::junction_capacitance_matches_ngspice`
/// holds `C(v)` to ngspice's own capacitance across a bias sweep;
/// `crates/stave-dsp/src/circuit/devices/capacitance.rs::the_capacitance_is_the_charge_s_derivative`
/// and `crates/stave-dsp/src/circuit/devices/capacitance.rs::charge_and_capacitance_are_continuous_at_the_corner`
/// hold `C` to the charge's derivative and both to continuity at the corner.
///
/// # Cost
///
/// One `powf` (a log and an exponential) below the corner, a few
/// products above it.
///
/// # Aliasing
///
/// None of its own: a charge, made a current by the circuit method's
/// integration.
///
/// # Valid ranges
///
/// `0 ≤ M < 1`, `0 ≤ FC < 1`, `VJ > 0`; any voltage.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Depletion {
    /// `CJO`: the zero-bias capacitance (F). Zero means no junction
    /// capacitance.
    pub zero_bias_farads: f64,
    /// `VJ`: the built-in potential (V).
    pub potential_volts: f64,
    /// `M`: the grading coefficient.
    pub grading: f64,
    /// `FC`: where the law turns linear, as a fraction of `VJ`.
    pub forward_coefficient: f64,
}

impl Depletion {
    /// No capacitance, at SPICE's default `VJ` = 1 V, `M` = ½, `FC` = ½
    /// (the diode card's defaults; each family's parser sets its own).
    pub const NONE: Self = Self {
        zero_bias_farads: 0.0,
        potential_volts: 1.0,
        grading: 0.5,
        forward_coefficient: 0.5,
    };

    /// True when the junction stores no depletion charge.
    pub fn is_none(&self) -> bool {
        self.zero_bias_farads == 0.0
    }

    /// The same law at `scale` times the zero-bias capacitance (an area
    /// factor, or `XCJC`'s split of the base–collector junction).
    pub fn scaled(self, scale: f64) -> Self {
        Self {
            zero_bias_farads: self.zero_bias_farads * scale,
            ..self
        }
    }

    /// `(q, C)`: the charge (C) and the capacitance `dq/dv` (F) at `volts`
    /// across the junction (forward positive).
    #[inline]
    pub fn charge(&self, volts: f64) -> (f64, f64) {
        let Self {
            zero_bias_farads: c0,
            potential_volts: vj,
            grading: m,
            forward_coefficient: fc,
        } = *self;
        if c0 == 0.0 {
            return (0.0, 0.0);
        }
        let corner = fc * vj;
        if volts < corner {
            let arg = 1.0 - volts / vj;
            let sarg = (-m * arg.ln()).exp();
            (vj * c0 * (1.0 - arg * sarg) / (1.0 - m), c0 * sarg)
        } else {
            let xfc = (1.0 - fc).ln();
            let f1 = vj * (1.0 - ((1.0 - m) * xfc).exp()) / (1.0 - m);
            let f2 = ((1.0 + m) * xfc).exp();
            let f3 = 1.0 - fc * (1.0 + m);
            let czf2 = c0 / f2;
            (
                c0 * f1 + czf2 * (f3 * (volts - corner) + m / (2.0 * vj) * (volts * volts - corner * corner)),
                czf2 * (f3 + m * volts / vj),
            )
        }
    }

    /// `(q, C)` as SPICE evaluates a BJT's substrate junction: the
    /// depletion law in reverse bias, and in forward bias the tangent at
    /// zero, `C = CJ0·(1 + M·v/VJ)` (no `FC` corner) — `bjtload.c`.
    #[inline]
    pub fn substrate_charge(&self, volts: f64) -> (f64, f64) {
        let Self {
            zero_bias_farads: c0,
            potential_volts: vj,
            grading: m,
            ..
        } = *self;
        if c0 == 0.0 {
            return (0.0, 0.0);
        }
        if volts < 0.0 {
            let arg = 1.0 - volts / vj;
            let sarg = (-m * arg.ln()).exp();
            (vj * c0 * (1.0 - arg * sarg) / (1.0 - m), c0 * sarg)
        } else {
            (volts * c0 * (1.0 + m * volts / (2.0 * vj)), c0 * (1.0 + m * volts / vj))
        }
    }
}

impl Default for Depletion {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAWS: [Depletion; 3] = [
        Depletion {
            zero_bias_farads: 4e-12,
            potential_volts: 1.0,
            grading: 0.4,
            forward_coefficient: 0.5,
        },
        Depletion {
            zero_bias_farads: 4.493e-12,
            potential_volts: 0.75,
            grading: 0.2593,
            forward_coefficient: 0.5,
        },
        Depletion {
            zero_bias_farads: 3e-12,
            potential_volts: 1.0,
            grading: 0.5,
            forward_coefficient: 0.95,
        },
    ];

    #[test]
    fn the_capacitance_is_the_charge_s_derivative() {
        for law in LAWS {
            for i in -200..=200 {
                let v = f64::from(i) * 0.05;
                let h = 1e-6;
                for f in [Depletion::charge, Depletion::substrate_charge] {
                    let numeric = (f(&law, v + h).0 - f(&law, v - h).0) / (2.0 * h);
                    let (_, c) = f(&law, v);
                    assert!((numeric - c).abs() <= 1e-6 * c.abs() + 1e-20, "{law:?} at {v}: {numeric} vs {c}");
                }
            }
        }
    }

    #[test]
    fn charge_and_capacitance_are_continuous_at_the_corner() {
        for law in LAWS {
            let corner = law.forward_coefficient * law.potential_volts;
            let (below, above) = (law.charge(corner - 1e-12), law.charge(corner + 1e-12));
            assert!((below.0 - above.0).abs() < 1e-22, "{law:?}");
            assert!((below.1 - above.1).abs() < 1e-9 * below.1, "{law:?}");
        }
        assert_eq!(Depletion::NONE.charge(0.3), (0.0, 0.0));
    }
}
