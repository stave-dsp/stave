//! Derk Reefman's pentode laws ("Spice models for vacuum tubes using the
//! uTracer", 2016, revised 2019, §4.3–4.5): Koren's space-charge current
//! split between plate and screen by the plate voltage, so the screen
//! current rises as the plate voltage falls toward the knee.

use super::{Pentode, PentodeCurrents};
use crate::circuit::devices::triode::softplus;

/// How the screen's share of the cathode current falls with plate
/// voltage: Reefman's two published forms.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Knee {
    /// `f(Va) = 1/(1 + β·Va)`: his model "Derk" (§4.4), a pentode whose
    /// screen current is inversely proportional to the plate voltage plus
    /// a constant (small-signal pentodes, the rounded knee).
    Hyperbolic,
    /// `f(Va) = exp(−(β·Va)^{3/2})`: his model "DerkE" (§4.5), the
    /// sharper knee of beam tetrodes and critically compensated pentodes.
    Exponential,
}

/// A pentode's or beam tetrode's Reefman parameters.
///
/// # Models
///
/// Plate and screen current of a pentode or beam tetrode, from the
/// control-grid, screen and plate voltages, with the screen current's
/// rise at low plate voltage that Koren's law leaves out:
///
/// ```text
///     E1  = (Vg2k/Kp)·ln(1 + exp(Kp·(1/μ + Vg1k/√(Kvb + Vg2k²))))
///     Ip  = E1^Ex                                  (0 when E1 ≤ 0)
///     Ig2 = Ip/Kg2 · (1 + αs·f(Va))
///     Ia  = Ip · [(1/Kg1 − 1/Kg2)·(1 − f(Va)) + A·Va/Kg1]
/// ```
///
/// with `f` one of the two [`Knee`]s (`f(0)` = 1, falling to 0). The
/// cathode current `Ia + Ig2 = Ip/Kg1 · (1 + A·Va − α·f(Va))`, Reefman's
/// constant space charge with his low-voltage reduction, `α = 1 −
/// (Kg1/Kg2)(1 + αs)` (his eq. 27, which makes `Ia(0)` = 0); `Ia` above is
/// his eq. (25) with that substituted. `A` is the plate's small pull on
/// the space charge that power tubes show.
///
/// Not modelled: control-grid current, interelectrode capacitance,
/// secondary emission (Reefman's §6 term `Psec`), and variable-μ.
///
/// # Derivation
///
/// Reefman's equations (14)–(15) for the Koren current, (23)–(27) for
/// "Derk" and (28)–(32) for "DerkE", as written; `Ia` is clamped to 0
/// and `f` to 1 at or below zero plate volts. The parts are least-squares
/// fits to the R23 tables of each sheet's plate and screen curves
/// (`crates/stave-test/tests/fit_datasheet.rs`, DSP.md E11).
///
/// # Verified
///
/// `crates/stave-dsp/src/circuit/devices/pentode/reefman.rs::the_partials_are_the_derivatives`
/// and
/// `crates/stave-dsp/src/circuit/devices/pentode/reefman.rs::the_screen_rises_toward_the_knee_and_the_plate_starts_at_zero`;
/// against the parts (DSP.md R13, E11), on the digitised plate and screen
/// curves: `crates/stave-test/tests/matches_datasheet.rs::tubes::el34_reefman`,
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::t6l6gc_reefman`
/// and `crates/stave-test/tests/matches_datasheet.rs::tubes::el84_reefman`:
/// EL34 plate 3.0% and screen 1.5% of the peak reading (budgets 6.1%,
/// 9.2%), EL84 4.6% and 4.8% (6.3%, 6.4%), passes; the 6L6GC 11.3% and
/// 12.9% (6.5%, 7.3%), a known gap — one law does not hold both of
/// Tung-Sol's charts (Koren's refit holds its plate curves).
///
/// # Cost
///
/// One softplus, one square root, one `powf` and the knee's `exp` (or a
/// division) per evaluation: Koren's pentode plus the knee.
///
/// # Aliasing
///
/// None of its own (a memoryless law); a hard-driven output stage makes
/// harmonics, so run it oversampled.
///
/// # Valid ranges
///
/// Screen voltage positive (all currents are 0 at or below it), plate
/// voltage at or above 0 (`Ia` = 0 below). The fits hold over the
/// sheets' curves: plates to about 500 V, grids from cutoff to 0 V, the
/// screen at the sheet's voltage (each table holds one).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Reefman {
    pub name: &'static str,
    pub mu: f64,
    pub ex: f64,
    pub kg1: f64,
    pub kg2: f64,
    pub kp: f64,
    pub kvb: f64,
    /// `A`: the plate's linear pull on the cathode current, per volt.
    pub a: f64,
    /// `αs`: the screen current's excess at zero plate volts.
    pub alpha_s: f64,
    /// `β`: the knee's scale, per volt.
    pub beta: f64,
    pub knee: Knee,
}

impl Reefman {
    /// EL34: fitted to Philips' plate curves at Vg2 = 250 V (Electronic
    /// Tube Handbook, "EL34", sheet B, drawing 7R02614) and the screen
    /// current of its transfer chart (sheet A, drawing 57380, Va = Vg2 =
    /// 250 V) together, the exponential knee
    /// (`crates/stave-test/tests/fit_datasheet.rs::el34_reefman`).
    pub const EL34_PHILIPS: Self = Self {
        name: "EL34 (Philips)",
        mu: 15.40536,
        ex: 1.0,
        kg1: 63.20474,
        kg2: 465.3983,
        kp: 59.48792,
        kvb: 1.0,
        a: 8.672855e-4,
        alpha_s: 2.681676,
        beta: 3.367023e-2,
        knee: Knee::Exponential,
    };

    /// 6L6GC: fitted to Tung-Sol's plate and screen-current curves at
    /// Vg2 = 250 V (tentative data sheet, April 1960) together, the
    /// hyperbolic knee (μ at its bound, 30)
    /// (`crates/stave-test/tests/fit_datasheet.rs::t6l6gc_reefman`). The
    /// part starts with a digit, so it is spelled with the family's tube
    /// prefix.
    pub const T6L6GC_TUNGSOL: Self = Self {
        name: "6L6GC (Tung-Sol)",
        mu: 30.0,
        ex: 1.0,
        kg1: 77.52796,
        kg2: 1456.315,
        kp: 18.43902,
        kvb: 1.0,
        a: 1.167864e-4,
        alpha_s: 8.392193,
        beta: 8.69517e-2,
        knee: Knee::Hyperbolic,
    };

    /// EL84: fitted to Philips' plate curves at Vg2 = 250 V (drawing
    /// 7R03703) and Mullard's screen-current curves at the same screen
    /// voltage (February 1961, page C3) together, the hyperbolic knee
    /// (`crates/stave-test/tests/fit_datasheet.rs::el84_reefman`).
    pub const EL84_PHILIPS: Self = Self {
        name: "EL84 (Philips)",
        mu: 25.8247,
        ex: 1.082762,
        kg1: 75.00501,
        kg2: 943.6276,
        kp: 80.27989,
        kvb: 1.0,
        a: 3.90944e-4,
        alpha_s: 11.58086,
        beta: 0.1007982,
        knee: Knee::Hyperbolic,
    };

    pub const ALL: [Self; 3] = [Self::EL34_PHILIPS, Self::T6L6GC_TUNGSOL, Self::EL84_PHILIPS];
}

impl Reefman {
    /// The knee function and its derivative at plate volts `va` (> 0).
    #[inline]
    fn knee_at(&self, va: f64) -> (f64, f64) {
        match self.knee {
            Knee::Hyperbolic => {
                let d = 1.0 + self.beta * va;
                (1.0 / d, -self.beta / (d * d))
            }
            Knee::Exponential => {
                let x = self.beta * va;
                let r = x.sqrt();
                let f = (-x * r).exp();
                (f, -1.5 * self.beta * r * f)
            }
        }
    }
}

impl Pentode for Reefman {
    #[inline]
    fn currents(&self, vg1k: f64, vg2k: f64, vpk: f64) -> PentodeCurrents {
        if vg2k <= 0.0 {
            return PentodeCurrents::default();
        }
        let root = (self.kvb + vg2k * vg2k).sqrt();
        let z = self.kp * (1.0 / self.mu + vg1k / root);
        let (s, sigma) = softplus(z);
        let e1 = vg2k * s / self.kp;
        if e1 <= 0.0 {
            return PentodeCurrents::default();
        }
        let de1_dg1 = vg2k * sigma / root;
        let dz_dg2 = -self.kp * vg1k * vg2k / (root * root * root);
        let de1_dg2 = s / self.kp + vg2k * sigma * dz_dg2 / self.kp;
        let pow = e1.powf(self.ex - 1.0);
        let ip = pow * e1;
        let dip_de1 = self.ex * pow;
        let (f, df) = if vpk > 0.0 {
            self.knee_at(vpk)
        } else {
            (1.0, 0.0)
        };
        let screen = (1.0 + self.alpha_s * f) / self.kg2;
        let share = 1.0 / self.kg1 - 1.0 / self.kg2;
        let (plate, plate_per_va) = if vpk > 0.0 {
            (
                share * (1.0 - f) + self.a * vpk / self.kg1,
                -share * df + self.a / self.kg1,
            )
        } else {
            (0.0, 0.0)
        };
        PentodeCurrents {
            plate: ip * plate,
            screen: ip * screen,
            plate_per_grid_volt: dip_de1 * de1_dg1 * plate,
            plate_per_screen_volt: dip_de1 * de1_dg2 * plate,
            plate_per_plate_volt: ip * plate_per_va,
            screen_per_grid_volt: dip_de1 * de1_dg1 * screen,
            screen_per_screen_volt: dip_de1 * de1_dg2 * screen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_partials_are_the_derivatives() {
        for tube in Reefman::ALL.into_iter().flat_map(|t| {
            [
                t,
                Reefman {
                    knee: Knee::Hyperbolic,
                    ..t
                },
            ]
        }) {
            for (g1, g2, p) in [
                (-10.0, 250.0, 200.0),
                (-30.0, 400.0, 60.0),
                (-2.0, 300.0, 15.0),
            ] {
                let c = tube.currents(g1, g2, p);
                let h = 1e-4;
                let d = |f: &dyn Fn(f64) -> PentodeCurrents| {
                    let (a, b) = (f(-h), f(h));
                    (
                        (b.plate - a.plate) / (2.0 * h),
                        (b.screen - a.screen) / (2.0 * h),
                    )
                };
                let (pg1, sg1) = d(&|e| tube.currents(g1 + e, g2, p));
                let (pg2, sg2) = d(&|e| tube.currents(g1, g2 + e, p));
                let (pp, _) = d(&|e| tube.currents(g1, g2, p + e));
                let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * b.abs().max(1e-9);
                let what = format!("{} {:?} at {g1}, {g2}, {p}: {c:?}", tube.name, tube.knee);
                assert!(close(c.plate_per_grid_volt, pg1), "{what} {pg1}");
                assert!(close(c.plate_per_screen_volt, pg2), "{what} {pg2}");
                assert!(close(c.plate_per_plate_volt, pp), "{what} {pp}");
                assert!(close(c.screen_per_grid_volt, sg1), "{what} {sg1}");
                assert!(close(c.screen_per_screen_volt, sg2), "{what} {sg2}");
            }
        }
    }

    #[test]
    fn the_screen_rises_toward_the_knee_and_the_plate_starts_at_zero() {
        for tube in Reefman::ALL {
            let at = |va: f64| tube.currents(-5.0, 250.0, va);
            assert_eq!(at(0.0).plate, 0.0, "{}", tube.name);
            assert!(at(10.0).screen > at(250.0).screen, "{}", tube.name);
            assert!(at(250.0).plate > at(10.0).plate, "{}", tube.name);
            let none = tube.currents(-5.0, 0.0, 250.0);
            assert_eq!(none, PentodeCurrents::default());
        }
    }
}
