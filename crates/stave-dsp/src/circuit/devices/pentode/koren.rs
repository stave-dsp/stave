//! Norman Koren's pentode law ("Improved vacuum tube models for SPICE
//! simulations", Glass Audio 8(5), 1996), plate and screen current.

use super::{Pentode, PentodeCurrents};
use crate::circuit::devices::triode::softplus;

/// One pentode's or beam tetrode's Koren parameters, as the published
/// subcircuit's `PARAMS:` line holds them.
///
/// # Models
///
/// Plate and screen current of a pentode or beam tetrode, from the
/// control-grid, screen and plate voltages:
///
/// ```text
///     E1  = (Vg2k/Kp)·ln(1 + exp(Kp·(1/μ + Vg1k/Vg2k)))
///     Ip  = 2·E1^Ex/Kg1 · atan(Vpk/Kvb)
///     Ig2 = (Vg1k + Vg2k/μ)^Ex / Kg2        (0 when the base is negative)
/// ```
///
/// Not modelled: control-grid current, interelectrode capacitance (the
/// published subcircuits add them as separate parts), the screen current's
/// rise at low plate voltage, and secondary emission.
///
/// # Derivation
///
/// Koren's subcircuit equations (normankoren.com, "Improved vacuum tube
/// models for SPICE simulations", parts 1 and 2: the `E1`, `G1` and `G2`
/// lines of his 6550 listing), solved as written. Parameters were fitted
/// by Koren to published plate curves, except EL84's (see
/// [`Koren::EL84`]).
///
/// # Verified
///
/// `crates/stave-test/tests/dk_matches_ngspice.rs::pentode_plate_curves_match_ngspice`
/// (EL34 plate current over 0–500 V at grid voltages 0 to −40 V, screen
/// through 1 kΩ: normalized RMS 2.2e-7, budget 5e-7) and
/// `crates/stave-test/tests/dk_matches_ngspice.rs::push_pull_pentode_matches_ngspice`
/// (an EL34 class-AB pair: 8.6e-9, budget 3e-8), the element against
/// ngspice running Koren's equations;
/// `crates/stave-dsp/src/circuit/devices/pentode/koren.rs::the_partials_are_the_derivatives`
/// and
/// `crates/stave-dsp/src/circuit/devices/pentode/koren.rs::the_fits_land_near_the_datasheet_class_a_points`
/// (within 20% of each sheet's class-A plate current; measured 3–16%).
///
/// Against the parts (DSP.md R13), on digitised plate and screen curves:
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::el84_plate`
/// passes (5.6% of the peak reading, budget 6.3%);
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::el34_plate`,
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::t6l6gc_plate`
/// and `crates/stave-test/tests/matches_datasheet.rs::tubes::kt88_plate`
/// miss by 10–14% (known gaps), and every screen curve
/// (`crates/stave-test/tests/matches_datasheet.rs::tubes::el34_screen`,
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::t6l6gc_screen`,
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::el84_screen`) by
/// 38–89%: the screen law is the weak part. The refits to those sheets
/// (DSP.md E11) pass on their plate curves:
/// `crates/stave-test/tests/matches_datasheet.rs::tubes::el34_philips` (3.6%,
/// budget 6.1%), `crates/stave-test/tests/matches_datasheet.rs::tubes::t6l6gc_tungsol`
/// (3.9%, budget 6.5%) and `crates/stave-test/tests/matches_datasheet.rs::tubes::kt88_gec`
/// (5.4%, budget 6.6%); refitting `Kg2` leaves the EL34 and 6L6GC screens
/// 20.5% and 39.6% off, the law's shape ([`Reefman`](super::reefman::Reefman)
/// has the rise).
///
/// # Cost
///
/// The datasheet's `"Circuit<Dk> EL34 push-pull x100 (Koren)"` (two
/// tubes: three controls and two outputs each in the DK solve); the
/// hand-derived [`PushPull`](crate::PushPull) tabulates the same law
/// once, `"PushPull (EL34) x100"`.
///
/// # Aliasing
///
/// None of its own (a memoryless law); a hard-driven output stage makes
/// harmonics, so run it oversampled or through ADAA.
///
/// # Valid ranges
///
/// Screen voltage positive (the law's `1/Vg2k` diverges at 0; all
/// currents are 0 at or below it), plate voltage positive (0 below).
/// The fits hold over the datasheet's plate curves: plates to about
/// 500 V, grids from cutoff to 0 V.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Koren {
    pub name: &'static str,
    pub mu: f64,
    pub ex: f64,
    pub kg1: f64,
    pub kg2: f64,
    pub kp: f64,
    pub kvb: f64,
}

impl Koren {
    /// EL34 (6CA7): Koren's original fit, `EL34_OLD` in the Koren tube
    /// library (Malcolm-Brown's 2008 revision keeps Koren's models under
    /// `_OLD` names; normankoren.com itself lists no EL34).
    pub const EL34: Self = Self {
        name: "EL34",
        mu: 11.0,
        ex: 1.35,
        kg1: 650.0,
        kg2: 4200.0,
        kp: 60.0,
        kvb: 24.0,
    };

    /// 6L6GC (beam tetrode): Koren's Table 1 (normankoren.com, part 1).
    /// The part starts with a digit, so it is spelled with the family's
    /// tube prefix, as [`T12AX7`](crate::circuit::devices::triode::koren::Koren::T12AX7) is.
    pub const T6L6GC: Self = Self {
        name: "6L6GC",
        mu: 8.7,
        ex: 1.35,
        kg1: 1460.0,
        kg2: 4500.0,
        kp: 48.0,
        kvb: 12.0,
    };

    /// KT88 (beam tetrode): Koren's Table 1 (normankoren.com, part 1),
    /// `Kg2` = 4200 as the table gives it (the tube library's `KT88_OLD`
    /// has 4800; Koren notes `Kg2` "does not need to be estimated with
    /// great accuracy").
    pub const KT88: Self = Self {
        name: "KT88",
        mu: 8.8,
        ex: 1.35,
        kg1: 730.0,
        kg2: 4200.0,
        kp: 32.0,
        kvb: 16.0,
    };

    /// EL84 (6BQ5): Koren never published one; this is Suusi
    /// Malcolm-Brown's Koren-form fit to the Mullard sheet, in her 2008
    /// revision of the Koren tube library (the most-cited EL84 set; `Kg2`
    /// = 4500 is that library's unfitted default).
    pub const EL84: Self = Self {
        name: "EL84",
        mu: 21.29,
        ex: 1.240,
        kg1: 401.7,
        kg2: 4500.0,
        kp: 111.04,
        kvb: 17.9,
    };

    /// EL34 refitted to Philips' plate curves at Vg2 = 250 V (Electronic
    /// Tube Handbook, "EL34", sheet B, drawing 7R02614), `Kg2` to the
    /// screen current of its transfer chart (sheet A), by least squares
    /// (`crates/stave-test/tests/fit_datasheet.rs::el34_koren`, DSP.md
    /// E11). Koren's own fit is [`Self::EL34`]. Koren's screen law cannot
    /// rise at low plate voltage; [`Reefman`](super::reefman::Reefman)'s
    /// can.
    pub const EL34_PHILIPS: Self = Self {
        name: "EL34 (Philips)",
        mu: 14.3778,
        ex: 1.0,
        kg1: 177.3962,
        kg2: 427.3643,
        kp: 62.00953,
        kvb: 31.06504,
    };

    /// 6L6GC refitted to Tung-Sol's plate curves at Vg2 = 250 V
    /// (tentative data sheet, April 1960), `Kg2` to the same sheet's
    /// screen curves, by least squares
    /// (`crates/stave-test/tests/fit_datasheet.rs::t6l6gc_koren`, DSP.md
    /// E11; `Ex` at its bound, 1.6). Koren's own fit is [`Self::T6L6GC`].
    pub const T6L6GC_TUNGSOL: Self = Self {
        name: "6L6GC (Tung-Sol)",
        mu: 9.707654,
        ex: 1.6,
        kg1: 2918.557,
        kg2: 9214.315,
        kp: 25.35764,
        kvb: 19.84447,
    };

    /// KT88 refitted to GEC's plate curves at Vg2 = 300 V by least
    /// squares (`crates/stave-test/tests/fit_datasheet.rs::kt88_koren`,
    /// DSP.md E11; `Ex` at its bound, 1.6); the sheet prints no screen
    /// curve, so `Kg2` is Koren's. Koren's own fit is [`Self::KT88`].
    pub const KT88_GEC: Self = Self {
        name: "KT88 (GEC)",
        mu: 7.048145,
        ex: 1.6,
        kg1: 2868.033,
        kg2: 4200.0,
        kp: 36.0887,
        kvb: 19.36558,
    };

    pub const ALL: [Self; 7] = [
        Self::EL34,
        Self::T6L6GC,
        Self::KT88,
        Self::EL84,
        Self::EL34_PHILIPS,
        Self::T6L6GC_TUNGSOL,
        Self::KT88_GEC,
    ];
}

impl Pentode for Koren {
    #[inline]
    fn currents(&self, vg1k: f64, vg2k: f64, vpk: f64) -> PentodeCurrents {
        if vg2k <= 0.0 {
            return PentodeCurrents::default();
        }
        let mut out = PentodeCurrents::default();
        // Screen: (Vg1k + Vg2k/μ)^Ex / Kg2.
        let base = vg1k + vg2k / self.mu;
        if base > 0.0 {
            let pow = base.powf(self.ex - 1.0);
            out.screen = pow * base / self.kg2;
            out.screen_per_grid_volt = self.ex * pow / self.kg2;
            out.screen_per_screen_volt = out.screen_per_grid_volt / self.mu;
        }
        if vpk <= 0.0 {
            return out;
        }
        let z = self.kp * (1.0 / self.mu + vg1k / vg2k);
        let (s, sigma) = softplus(z);
        let e1 = vg2k * s / self.kp;
        if e1 <= 0.0 {
            return out;
        }
        let de1_dg1 = sigma;
        let de1_dg2 = s / self.kp - sigma * vg1k / vg2k;
        let r = vpk / self.kvb;
        let arc = r.atan();
        let pow = e1.powf(self.ex - 1.0);
        let scale = 2.0 / self.kg1;
        out.plate = scale * pow * e1 * arc;
        let dip_de1 = scale * self.ex * pow * arc;
        out.plate_per_grid_volt = dip_de1 * de1_dg1;
        out.plate_per_screen_volt = dip_de1 * de1_dg2;
        out.plate_per_plate_volt = scale * pow * e1 / (self.kvb * (1.0 + r * r));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_partials_are_the_derivatives() {
        for tube in Koren::ALL {
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
                assert!(
                    close(c.plate_per_grid_volt, pg1),
                    "{} {g1}: {c:?} {pg1}",
                    tube.name
                );
                assert!(
                    close(c.plate_per_screen_volt, pg2),
                    "{} {g1}: {c:?} {pg2}",
                    tube.name
                );
                assert!(
                    close(c.plate_per_plate_volt, pp),
                    "{} {g1}: {c:?} {pp}",
                    tube.name
                );
                assert!(
                    close(c.screen_per_grid_volt, sg1),
                    "{} {g1}: {c:?} {sg1}",
                    tube.name
                );
                assert!(
                    close(c.screen_per_screen_volt, sg2),
                    "{} {g1}: {c:?} {sg2}",
                    tube.name
                );
            }
        }
    }

    /// Koren's fits land near each datasheet's class-A point (plate and
    /// screen at 250 V): EL34 100 mA at −13.5 V (Philips-format sheet,
    /// 1969), 6L6GC 72 mA at −14 V (RCA, 1960), KT88 140 mA at −15 V (GEC,
    /// 1961), EL84 48 mA at −7.3 V (1969 sheet). A fit is not the sheet:
    /// within 20% (measured 3–16%).
    #[test]
    fn the_fits_land_near_the_datasheet_class_a_points() {
        for (tube, bias, ma) in [
            (Koren::EL34, -13.5, 100.0),
            (Koren::T6L6GC, -14.0, 72.0),
            (Koren::KT88, -15.0, 140.0),
            (Koren::EL84, -7.3, 48.0),
        ] {
            let got = tube.currents(bias, 250.0, 250.0).plate * 1e3;
            assert!(
                (got / ma - 1.0).abs() < 0.2,
                "{}: {got:.1} mA vs {ma}",
                tube.name
            );
        }
    }

    #[test]
    fn no_screen_voltage_no_current() {
        let c = Koren::EL34.currents(-5.0, 0.0, 300.0);
        assert_eq!(c, PentodeCurrents::default());
        let cut = Koren::EL34.currents(-80.0, 250.0, 300.0);
        assert_eq!(cut.screen, 0.0);
        assert!(cut.plate < 1e-6, "{cut:?}");
    }
}
