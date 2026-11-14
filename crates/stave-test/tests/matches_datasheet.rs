//! Device models against their parts' published curves (DSP.md R13,
//! R23). ngspice proves we solve each model; these prove the model is
//! the part — as far as a datasheet's typical curves can.
//!
//! Each table under `crates/stave-test/datasheet/` was digitised once
//! from the figure its header cites, with the digitising method and an
//! honest uncertainty. A budget is that uncertainty plus a stated model
//! tolerance:
//!
//! - **Linear axes** (plate curves, output families, hFE): the reading's
//!   uncertainty plus [`TOLERANCE`] of the table's largest reading (its
//!   peak; the errors are reported as a percentage of it). A datasheet
//!   curve is a typical part, and production spread is ±20% or more, so
//!   5% of the peak asks a fit to land on the typical curve, not on one
//!   sample.
//! - **Log axes** (transconductance against bias, open-loop gain): the
//!   reading's uncertainty plus [`LOG_TOLERANCE`], compared as ln(value).
//! - **Printed figures** (slew rate, gain-bandwidth, a pulse edge's
//!   slope): [`LOG_TOLERANCE`] of the figure, plus a slope's reading
//!   uncertainty.
//! - **Diode forward curves**: forward voltage at each current read off
//!   the graph, within the reading's voltage uncertainty plus
//!   [`DIODE_VOLTS`].
//!
//! A reading's abscissa is uncertain too: the model is evaluated across
//! `x ± uncertainty-x` and the value nearest the reading counts, so a
//! steep curve is not failed for a pixel of horizontal error.
//!
//! A model that misses by more than its budget is a **known gap**: the
//! test still runs, asserts the miss is still a miss (so a refit that
//! closes it fails here until the test is promoted to a pass), and that
//! it is no worse than the error measured when the gap was recorded. The
//! gaps are the refit list in DSP.md.

mod bench;

use bench::{bisect, bjt, diode_volts, nearest, opamp};
use stave_test::reference::{Case, Curve, Datasheet, Metric, Outcome, Uncertainty};

/// Model tolerance on a linear axis, as a fraction of the table's peak
/// reading.
const TOLERANCE: f64 = 0.05;
/// Model tolerance on a log axis or a printed figure, as a fraction of
/// the value.
const LOG_TOLERANCE: f64 = 0.10;
/// Model tolerance on a diode's forward voltage, volts.
const DIODE_VOLTS: f64 = 0.020;

/// A family's outcomes: one per curve, all on one budget.
struct Family {
    table: String,
    outcomes: Vec<Outcome>,
    /// The worst curve's error and the budget, in the comparison's
    /// units (nepers on a log axis), and the peak reading a linear
    /// axis's are reported against.
    worst: f64,
    budget: f64,
    scale: f64,
    log: bool,
}

impl Family {
    /// An error as a percentage for the report: of the peak reading on
    /// a linear axis, of the value (the ratio's excess, e^error − 1) on a
    /// log one.
    fn percent(&self, e: f64) -> f64 {
        if self.log {
            100.0 * e.exp_m1()
        } else {
            100.0 * e / self.scale
        }
    }

    /// The family's result as `part`'s, under `model`.
    fn of(self, part: &str, model: &str) -> Measured {
        let worst = self
            .outcomes
            .iter()
            .max_by(|a, b| a.error.total_cmp(&b.error))
            .map_or("", |o| o.case.as_str());
        Measured {
            what: format!("{part} | {model} | {} (worst: {worst})", self.table),
            error: self.percent(self.worst),
            budget: self.percent(self.budget),
            unit: if self.log { "% of value" } else { "% of peak" },
        }
    }
}

/// One check's result, error and budget in the unit its report states.
struct Measured {
    what: String,
    error: f64,
    budget: f64,
    unit: &'static str,
}

impl Measured {
    fn report(&self, verdict: &str) {
        println!(
            "R13 | {} | {:.1} {} | budget {:.1} {} | {verdict}",
            self.what, self.error, self.unit, self.budget, self.unit
        );
    }
}

/// Hold every curve of `table` to `model(curve, x)` on a linear value
/// axis: budget = the reading's uncertainty + [`TOLERANCE`] of full scale.
fn linear(table: &str, model: impl Fn(&Curve, f64) -> f64) -> Family {
    let sheet = Datasheet::load(table);
    let scale = sheet
        .curves()
        .iter()
        .flat_map(|c| c.reference.values())
        .fold(0.0, |m: f64, v| m.max(v.abs()));
    let budget = sheet.uncertainty_y().at(scale) + TOLERANCE * scale;
    let outcomes = sheet
        .curves()
        .iter()
        .map(|curve| {
            let r = &curve.reference;
            let run: Vec<f64> = r
                .abscissa()
                .iter()
                .zip(r.values())
                .map(|(&x, &y)| {
                    let ux = sheet.uncertainty_x().at(x);
                    nearest(x, y, ux, &|x| model(curve, x))
                })
                .collect();
            Case::new(r.name(), r.clone())
                .budget(Metric::MaxAbs, budget)
                .compare(&run)
        })
        .collect();
    family(table, outcomes, budget, Some(scale))
}

/// Hold every curve of `table` to `model(curve, x)` on a log value axis,
/// compared as ln(value): budget = ln(1 + the reading's uncertainty) +
/// ln(1 + [`LOG_TOLERANCE`]).
fn logarithmic(table: &str, model: impl Fn(&Curve, f64) -> f64) -> Family {
    let sheet = Datasheet::load(table);
    let Uncertainty::Relative(uy) = sheet.uncertainty_y() else {
        panic!("{table}: a log comparison needs a relative uncertainty-y");
    };
    let budget = uy.ln_1p() + LOG_TOLERANCE.ln_1p();
    let outcomes = sheet
        .curves()
        .iter()
        .map(|curve| {
            let r = &curve.reference;
            let run: Vec<f64> = r
                .abscissa()
                .iter()
                .zip(r.values())
                .map(|(&x, &y)| {
                    let ux = sheet.uncertainty_x().at(x);
                    nearest(x, y.ln(), ux, &|x| model(curve, x).ln())
                })
                .collect();
            Case::new(r.name(), r.clone())
                .map_reference("ln", f64::ln)
                .budget(Metric::MaxAbs, budget)
                .compare(&run)
        })
        .collect();
    family(table, outcomes, budget, None)
}

/// `scale`: the peak a linear axis's errors are reported against; `None`
/// for a log axis.
fn family(table: &str, outcomes: Vec<Outcome>, budget: f64, scale: Option<f64>) -> Family {
    let worst = outcomes.iter().map(|o| o.error).fold(0.0, f64::max);
    for o in &outcomes {
        println!("{o}");
    }
    Family {
        table: table.to_string(),
        outcomes,
        worst,
        budget,
        scale: scale.unwrap_or(1.0),
        log: scale.is_none(),
    }
}

/// Within budget.
#[track_caller]
fn pass(m: Measured) {
    let ok = m.error <= m.budget;
    m.report(if ok { "pass" } else { "FAILED" });
    assert!(
        ok,
        "{}: {:.2} {} > budget {:.2}",
        m.what, m.error, m.unit, m.budget
    );
}

/// A known gap (see the module docs): still over budget, and no worse
/// than `recorded` (in the check's unit) + 5% of itself.
#[track_caller]
fn gap(m: Measured, recorded: f64) {
    m.report("known gap");
    assert!(
        m.error > m.budget,
        "{} now meets its budget ({:.2} ≤ {:.2} {}): promote the known gap to a pass",
        m.what,
        m.error,
        m.budget,
        m.unit
    );
    assert!(
        m.error <= recorded * 1.05,
        "{} is further off than when the gap was recorded: {:.2} > {recorded} {}",
        m.what,
        m.error,
        m.unit
    );
}

/// A table limit (a minimum, a maximum, or both) on a figure the sheet
/// prints only as a number: `value` at `at` within `[min, max]`.
#[track_caller]
fn limit(part: &str, model: &str, figure: &str, at: f64, value: f64, min: f64, max: f64) {
    let ok = (min..=max).contains(&value);
    println!(
        "R13 | {part} | {model} | {figure} at {at:e} | {value:.4} | limit [{min}, {max}] | {}",
        if ok { "pass" } else { "GAP" }
    );
    assert!(
        ok,
        "{part} ({model}): {figure} at {at:e} is {value}, outside [{min}, {max}]"
    );
}

// ----------------------------------------------------------------- tubes

mod tubes {
    use stave_dsp::circuit::devices::pentode::{koren::Koren as KorenPentode, reefman::Reefman, Pentode};
    use stave_dsp::circuit::devices::triode::dempwolf::Dempwolf;
    use stave_dsp::circuit::devices::triode::koren::Koren;
    use stave_dsp::circuit::devices::triode::Triode;

    use super::{gap, linear, pass};

    fn triode(table: &str, tube: impl Triode) -> super::Family {
        linear(table, |c, va| tube.currents(c.parameter("vg"), va).plate)
    }

    /// Plate current at the table's screen voltage (a condition, or a
    /// curve's own).
    fn plate(table: &str, tube: impl Pentode) -> super::Family {
        let vg2 = stave_test::reference::Datasheet::load(table).condition("vg2");
        linear(table, move |c, va| {
            let vg2 = c.get("vg2").or(vg2).expect("a screen voltage");
            tube.currents(c.parameter("vg1"), vg2, va).plate
        })
    }

    /// Screen current against plate voltage at each curve's Vg1, or,
    /// when a curve fixes the plate voltage (`va`, the EL34's transfer
    /// chart), against the control grid: `x` is Vg1 there.
    fn screen(table: &str, tube: impl Pentode) -> super::Family {
        let vg2 = stave_test::reference::Datasheet::load(table).condition("vg2");
        linear(table, move |c, x| {
            let vg2 = c.get("vg2").or(vg2).expect("a screen voltage");
            match c.get("va") {
                Some(va) => tube.currents(x, vg2, va).screen,
                None => tube.currents(c.parameter("vg1"), vg2, x).screen,
            }
        })
    }

    /// Koren's published 12AX7, kept beside the fit to this sheet.
    #[test]
    fn ecc83_koren() {
        gap(
            triode("ecc83_plate", Koren::T12AX7).of("12AX7/ECC83", "Koren T12AX7"),
            11.7,
        );
    }

    /// E11: Koren's law fitted to this sheet.
    #[test]
    fn ecc83_philips() {
        pass(triode("ecc83_plate", Koren::ECC83_PHILIPS).of("12AX7/ECC83", "Koren ECC83_PHILIPS"));
    }

    #[test]
    fn ecc83_dempwolf() {
        // Measured 12AX7s (Dempwolf & Zölzer's table 1), not the Philips
        // average tube: each misses it as the Koren fit does.
        for (tube, recorded) in [
            (Dempwolf::RSD_1, 11.4),
            (Dempwolf::RSD_2, 13.8),
            (Dempwolf::EHX_1, 11.5),
        ] {
            let model = format!("Dempwolf {}", tube.name);
            gap(
                triode("ecc83_plate", tube).of("12AX7/ECC83", &model),
                recorded,
            );
        }
    }

    #[test]
    fn el34_plate() {
        gap(
            plate("el34_plate", KorenPentode::EL34).of("EL34", "Koren EL34"),
            12.1,
        );
    }

    /// The screen current against the control grid at Va = Vg2 = 250 V
    /// (the sheet's transfer chart): `x` is Vg1 here.
    #[test]
    fn el34_screen() {
        gap(
            screen("el34_screen", KorenPentode::EL34).of("EL34", "Koren EL34 (screen)"),
            53.7,
        );
    }

    /// E11: Koren's law refitted to Philips' sheet (plate; `Kg2` to the
    /// screen chart, which the law's shape still misses).
    #[test]
    fn el34_philips() {
        pass(plate("el34_plate", KorenPentode::EL34_PHILIPS).of("EL34", "Koren EL34_PHILIPS"));
        gap(
            screen("el34_screen", KorenPentode::EL34_PHILIPS).of("EL34", "Koren EL34_PHILIPS (screen)"),
            20.5,
        );
    }

    /// E11: Reefman's law, plate and screen fitted together.
    #[test]
    fn el34_reefman() {
        let tube = Reefman::EL34_PHILIPS;
        pass(plate("el34_plate", tube).of("EL34", "Reefman EL34_PHILIPS"));
        pass(screen("el34_screen", tube).of("EL34", "Reefman EL34_PHILIPS (screen)"));
    }

    #[test]
    fn t6l6gc_plate() {
        gap(
            plate("6l6gc_plate", KorenPentode::T6L6GC).of("6L6GC", "Koren 6L6GC"),
            10.0,
        );
    }

    #[test]
    fn t6l6gc_screen() {
        gap(
            screen("6l6gc_screen", KorenPentode::T6L6GC).of("6L6GC", "Koren 6L6GC (screen)"),
            38.4,
        );
    }

    /// E11: Koren's law refitted to Tung-Sol's plate curves.
    #[test]
    fn t6l6gc_tungsol() {
        pass(plate("6l6gc_plate", KorenPentode::T6L6GC_TUNGSOL).of("6L6GC", "Koren T6L6GC_TUNGSOL"));
        gap(
            screen("6l6gc_screen", KorenPentode::T6L6GC_TUNGSOL)
                .of("6L6GC", "Koren T6L6GC_TUNGSOL (screen)"),
            39.6,
        );
    }

    /// E11: Reefman's law, plate and screen fitted together. One law
    /// cannot hold both of Tung-Sol's charts to budget: the screen's slow
    /// fall to 400 V pulls the knee away from the plate curves' (the
    /// Koren refit, plate only, passes).
    #[test]
    fn t6l6gc_reefman() {
        let tube = Reefman::T6L6GC_TUNGSOL;
        let plate = plate("6l6gc_plate", tube).of("6L6GC", "Reefman T6L6GC_TUNGSOL");
        let screen = screen("6l6gc_screen", tube).of("6L6GC", "Reefman T6L6GC_TUNGSOL (screen)");
        gap(plate, 11.3);
        gap(screen, 12.9);
    }

    #[test]
    fn kt88_plate() {
        gap(
            plate("kt88_plate", KorenPentode::KT88).of("KT88", "Koren KT88"),
            14.1,
        );
    }

    /// E11: Koren's law refitted to GEC's plate curves.
    #[test]
    fn kt88_gec() {
        pass(plate("kt88_plate", KorenPentode::KT88_GEC).of("KT88", "Koren KT88_GEC"));
    }

    #[test]
    fn el84_plate() {
        pass(plate("el84_plate", KorenPentode::EL84).of("EL84", "Koren EL84"));
    }

    #[test]
    fn el84_screen() {
        gap(
            screen("el84_screen", KorenPentode::EL84).of("EL84", "Koren EL84 (screen)"),
            89.0,
        );
    }

    /// E11: Reefman's law, Philips' plate and Mullard's screen curves
    /// fitted together.
    #[test]
    fn el84_reefman() {
        let tube = Reefman::EL84_PHILIPS;
        pass(plate("el84_plate", tube).of("EL84", "Reefman EL84_PHILIPS"));
        pass(screen("el84_screen", tube).of("EL84", "Reefman EL84_PHILIPS (screen)"));
    }
}

// ------------------------------------------------- transistors and diodes

mod transistors {
    use stave_dsp::circuit::devices::bjt::{EbersMoll, GummelPoon};

    use super::bjt::{collector_at, hfe_at, terminal_vbe, vce_for};
    use super::{gap, limit, linear, logarithmic, pass, Family};

    /// The card's Ebers–Moll core: its IS, BF, BR and Vt alone, so the
    /// table shows what Gummel–Poon's Early effect, roll-off, leakage and
    /// ohmic resistances add.
    fn ebers_moll(q: GummelPoon) -> GummelPoon {
        GummelPoon::from(EbersMoll {
            polarity: q.polarity,
            saturation_amps: q.saturation_amps,
            forward_beta: q.forward_beta,
            reverse_beta: q.reverse_beta,
            thermal_volts: q.thermal_volts,
        })
    }

    /// hFE over its peak across the table's range of collector current
    /// (the sheet's normalisation).
    fn normalised_hfe(table: &str, q: GummelPoon) -> Family {
        let sheet = stave_test::reference::Datasheet::load(table);
        let vce = sheet.condition("vce").expect("the chart's VCE");
        let xs = sheet.curves()[0].reference.abscissa();
        let (lo, hi) = (xs[0].ln(), xs[xs.len() - 1].ln());
        let peak = (0..=200)
            .map(|k| hfe_at(&q, (lo + (hi - lo) * f64::from(k) / 200.0).exp(), vce))
            .fold(0.0, f64::max);
        logarithmic(table, move |_, ic| hfe_at(&q, ic, vce) / peak)
    }

    /// VBE at VCE = 1 V, and VBE(sat), VCE(sat) at IC/IB = 10, against IC.
    fn on_voltages(q: GummelPoon) -> Family {
        linear("2n3904_on_voltages", move |c, ic| {
            if let Some(vce) = c.get("vce") {
                let ib = ic / hfe_at(&q, ic, vce);
                return terminal_vbe(&q, ib, vce);
            }
            let ib = ic / c.parameter("ic_over_ib");
            let vce = vce_for(&q, ib, ic);
            match c.parameter("kind") as u8 {
                1 => terminal_vbe(&q, ib, vce),
                _ => vce,
            }
        })
    }

    /// VCE against IB at fixed IC: the saturation region.
    fn saturation(q: GummelPoon) -> Family {
        linear("2n3904_saturation", move |c, ib| {
            vce_for(&q, ib, c.parameter("ic"))
        })
    }

    #[test]
    fn n2n3904_gummel_poon() {
        let q = GummelPoon::N2N3904;
        // IKF = 66.78 mA rolls hFE off too gently: at 195 mA the card
        // keeps 0.42 of its peak, the sheet 0.14.
        gap(
            normalised_hfe("2n3904_hfe", q).of("2N3904", "Gummel–Poon N2N3904"),
            206.3,
        );
        // VBE at high current: 0.87 V against the sheet's 1.00 at 195 mA
        // (RB = 10 Ω, RE = 0 are too small).
        gap(on_voltages(q).of("2N3904", "Gummel–Poon N2N3904"), 24.3);
        // Fig. 16 leaves saturation at IB ≈ 19 µA for 1 mA (β ≈ 52); the
        // card's β is 133 there (the sheet's own table: ≥ 70).
        gap(saturation(q).of("2N3904", "Gummel–Poon N2N3904"), 85.9);
        hfe_limits(q, "Gummel–Poon N2N3904");
    }

    /// E11: the card fitted to onsemi's Figures 15–17 within the table's
    /// hFE limits. Figure 16's IC = 1 mA curve (β ≈ 52) contradicts the
    /// table's hFE ≥ 70 at 1 mA, so the saturation region stays a gap;
    /// held to the table's limits, the curves trade among themselves
    /// (VCE(sat) at 195 mA is the ON-voltage figure's worst, a little
    /// worse than the Fairchild card's 24.3%).
    #[test]
    fn n2n3904_onsemi() {
        let q = GummelPoon::N2N3904_ONSEMI;
        let model = "Gummel–Poon N2N3904_ONSEMI";
        gap(normalised_hfe("2n3904_hfe", q).of("2N3904", model), 33.8);
        gap(on_voltages(q).of("2N3904", model), 25.3);
        gap(saturation(q).of("2N3904", model), 78.8);
        hfe_limits(q, model);
    }

    /// The table's hFE limits at VCE = 1 V (onsemi 2N3903/D).
    fn hfe_limits(q: GummelPoon, model: &str) {
        for (ic, min, max) in [
            (0.1e-3, 40.0, f64::INFINITY),
            (1e-3, 70.0, f64::INFINITY),
            (10e-3, 100.0, 300.0),
            (50e-3, 60.0, f64::INFINITY),
            (100e-3, 30.0, f64::INFINITY),
        ] {
            limit(
                "2N3904",
                model,
                "hFE",
                ic,
                hfe_at(&q, ic, 1.0),
                min,
                max,
            );
        }
    }

    /// The AC128 card is Holmes, Holters & van Walstijn's measured
    /// vintage part (DAFx-17), not Mullard's typical one; the sheet's own
    /// hFE spread is 55–175. Magnitudes throughout: a PNP.
    const AC128: (&str, &str) = ("AC128", "Gummel–Poon AC128");

    /// |IC| against |VCE| at forced |IB|.
    fn ac128_output_of(q: GummelPoon) -> Family {
        linear("ac128_output", |c, vce| collector_at(&q, c.parameter("ib"), vce))
    }

    /// hFE against |IC| at |VCE| = 1 V.
    fn ac128_hfe_of(q: GummelPoon) -> Family {
        linear("ac128_hfe", |c, ic| hfe_at(&q, ic, c.parameter("vce")))
    }

    /// |IC| against |IB| at |VCE| = 1 V.
    fn ac128_transfer_of(q: GummelPoon) -> Family {
        linear("ac128_transfer", |c, ib| collector_at(&q, ib, c.parameter("vce")))
    }

    /// |IB| against |VBE| at |VCE| = 1 V.
    fn ac128_input_of(q: GummelPoon) -> Family {
        linear("ac128_input", |c, vbe| {
            let vce = c.parameter("vce");
            super::bisect(0.0, 0.1, |ib| terminal_vbe(&q, ib, vce) - vbe)
        })
    }

    #[test]
    fn ac128_output() {
        gap(ac128_output_of(GummelPoon::AC128).of(AC128.0, AC128.1), 25.0);
    }

    /// hFE against |IC|, and the table's static hFE limits at VCB = 0.
    #[test]
    fn ac128_hfe() {
        let q = GummelPoon::AC128;
        gap(ac128_hfe_of(q).of(AC128.0, AC128.1), 11.9);
        ac128_hfe_limits(q, AC128.1);
    }

    /// E11: the typical-part card, fitted to every Mullard curve at once
    /// within the table's hFE limits; the DAFx-17 vintage card stays
    /// beside it.
    #[test]
    fn ac128_mullard() {
        let q = GummelPoon::AC128_MULLARD;
        let model = "Gummel–Poon AC128_MULLARD";
        pass(ac128_output_of(q).of(AC128.0, model));
        pass(ac128_hfe_of(q).of(AC128.0, model));
        pass(ac128_transfer_of(q).of(AC128.0, model));
        pass(ac128_input_of(q).of(AC128.0, model));
        ac128_hfe_limits(q, model);
    }

    /// The table's static hFE at VCB = 0: |VCE| = |VBE|, solved with the
    /// operating point.
    fn ac128_hfe_limits(q: GummelPoon, model: &str) {
        for (ic, min, max) in [
            (50e-3, 55.0, 175.0),
            (300e-3, 55.0, 175.0),
            (1.0, 45.0, 165.0),
        ] {
            let ib = super::bisect(0.0, ic, |ib| {
                let vce = super::bisect(0.0, 2.0, |v| v - terminal_vbe(&q, ib, v));
                collector_at(&q, ib, vce) - ic
            });
            limit(AC128.0, model, "hFE (VCB = 0)", ic, ic / ib, min, max);
        }
    }

    #[test]
    fn ac128_transfer() {
        gap(ac128_transfer_of(GummelPoon::AC128).of(AC128.0, AC128.1), 24.3);
    }

    #[test]
    fn ac128_input() {
        gap(ac128_input_of(GummelPoon::AC128).of(AC128.0, AC128.1), 45.2);
    }

    #[test]
    fn n2n3904_ebers_moll() {
        let q = ebers_moll(GummelPoon::N2N3904);
        // No roll-off, no ohmic drops: what Gummel–Poon's terms buy.
        gap(
            normalised_hfe("2n3904_hfe", q).of("2N3904", "Ebers–Moll core of N2N3904"),
            623.1,
        );
        gap(
            on_voltages(q).of("2N3904", "Ebers–Moll core of N2N3904"),
            48.1,
        );
        gap(
            saturation(q).of("2N3904", "Ebers–Moll core of N2N3904"),
            88.6,
        );
    }
}

mod diodes {
    use stave_dsp::circuit::devices::diode::Shockley;
    use stave_test::reference::{Case, Datasheet, Metric, Uncertainty};

    use super::{diode_volts, gap, limit, pass, Measured, DIODE_VOLTS};

    /// Forward voltage at each current read off the graph: budget = the
    /// reading's voltage uncertainty, plus its current uncertainty as
    /// volts on the model's own slope, plus [`DIODE_VOLTS`].
    fn forward(table: &str, part: &str, model: &str, d: Shockley) -> Measured {
        let sheet = Datasheet::load(table);
        let curve = &sheet.curves()[0];
        let Uncertainty::Relative(ui) = sheet.uncertainty_y() else {
            panic!("{table}: a log current axis");
        };
        let ux = sheet.uncertainty_x().at(1.0);
        let reference = curve.transposed();
        let slope = reference
            .abscissa()
            .iter()
            .map(|&i| (diode_volts(&d, i * (1.0 + ui)) - diode_volts(&d, i)).abs())
            .fold(0.0, f64::max);
        let budget = ux + slope + DIODE_VOLTS;
        let outcome = Case::new(reference.name(), reference.clone())
            .budget(Metric::MaxAbs, budget)
            .pointwise(|i| diode_volts(&d, i));
        println!("{outcome}");
        Measured {
            what: format!("{part} | {model} | {table} (forward volts at the read currents)"),
            error: outcome.error * 1e3,
            budget: budget * 1e3,
            unit: "mV",
        }
    }

    #[test]
    fn n4148() {
        // 37.5 mV low at 20 mA: the card's RS = 0.568 Ω is light for the
        // Vishay curve's high-current end.
        gap(
            forward(
                "1n4148_forward",
                "1N4148",
                "Shockley N4148",
                Shockley::N4148,
            ),
            37.5,
        );
    }

    /// E11: `IS`, `N` and `RS` fitted to the Vishay curve.
    #[test]
    fn n4148_vishay() {
        pass(forward(
            "1n4148_forward",
            "1N4148",
            "Shockley N4148_VISHAY",
            Shockley::N4148_VISHAY,
        ));
    }

    /// The sheet prints no curve, only a maximum: VF ≤ 1.00 V at 5 mA.
    #[test]
    fn n34a() {
        let sheet = Datasheet::load("1n34a_forward");
        let r = &sheet.curves()[0].reference;
        for (&vf, &amps) in r.abscissa().iter().zip(r.values()) {
            let got = diode_volts(&Shockley::N34A, amps);
            limit("1N34A", "Shockley N34A", "VF (V)", amps, got, 0.0, vf);
        }
    }
}

mod opamps {
    use stave_dsp::circuit::devices::opamp::Macromodel;
    use stave_test::reference::{Case, Datasheet, Metric};

    use super::opamp::{edges, follower, slew, swing_at};
    use super::{bisect, gap, linear, logarithmic, pass, Family, Measured, LOG_TOLERANCE};

    /// The follower's −3 dB frequency through `Circuit<Dk>`'s
    /// small-signal response: the unity-gain bandwidth a bench measures.
    fn bandwidth(part: Macromodel, supply: (f64, f64)) -> f64 {
        let dk = follower(part, supply);
        let lf = bisect(3.0, 9.0, |lf| {
            std::f64::consts::FRAC_1_SQRT_2
                - dk.analog_response(10f64.powf(lf)).unwrap().magnitude()
        });
        10f64.powf(lf)
    }

    /// A figure the sheet's table prints (slew rate, gain-bandwidth),
    /// against the model measured the way the table defines it: within
    /// the transcription's uncertainty (none) plus [`LOG_TOLERANCE`]
    /// (10%) of the figure.
    fn figures(table: &str, part: Macromodel) -> Vec<Measured> {
        let sheet = Datasheet::load(table);
        sheet
            .curves()
            .iter()
            .map(|c| {
                let r = &c.reference;
                let run: Vec<f64> = r
                    .abscissa()
                    .iter()
                    .map(|&vcc| {
                        let supply = (-vcc, vcc);
                        if c.get("slew").is_some() {
                            slew(part, supply, -5.0, 5.0)
                        } else {
                            bandwidth(part, supply)
                        }
                    })
                    .collect();
                let o = Case::new(r.name(), r.clone())
                    .budget(Metric::MaxRelative, LOG_TOLERANCE)
                    .compare(&run);
                println!("{o}");
                let figure = if c.get("slew").is_some() {
                    "slew rate"
                } else {
                    "GBW"
                };
                Measured {
                    what: format!(
                        "{} | Macromodel {} | {table} {figure}",
                        part.name, part.name
                    ),
                    error: 100.0 * o.error,
                    budget: 100.0 * LOG_TOLERANCE,
                    unit: "% of value",
                }
            })
            .collect()
    }

    /// A check that yields exactly two results, as an array.
    fn two(v: Vec<Measured>) -> [Measured; 2] {
        v.try_into()
            .unwrap_or_else(|v: Vec<Measured>| panic!("wanted two results, got {}", v.len()))
    }

    /// The macromodel's small-signal open loop, gm/(G + sC), from its
    /// prepared constants on the table's supply.
    fn open_loop(table: &str, part: Macromodel) -> Family {
        let vcc = Datasheet::load(table)
            .condition("vcc")
            .expect("the chart's supply");
        let p = part.prepare(-vcc, vcc);
        let c = Macromodel::COMPENSATION_FARADS;
        logarithmic(table, move |_, f| {
            p.gm / p.leak.hypot(std::f64::consts::TAU * f * c)
        })
    }

    /// The voltage-follower pulse response's edges, sheet and model read
    /// the same way ([`edges`]): budget = the slope's reading uncertainty
    /// + [`LOG_TOLERANCE`].
    fn pulse(table: &str, part: Macromodel) -> Vec<Measured> {
        edges(table, part)
            .into_iter()
            .map(|e| {
                let error = (e.model / e.sheet - 1.0).abs();
                Measured {
                    what: format!(
                        "{} | Macromodel {} | {table} {} edge (sheet {:.2} V/µs, model {:.2} V/µs)",
                        part.name,
                        part.name,
                        e.name,
                        e.sheet * 1e-6,
                        e.model * 1e-6
                    ),
                    error: 100.0 * error,
                    budget: 100.0 * (e.reading + LOG_TOLERANCE),
                    unit: "% of value",
                }
            })
            .collect()
    }

    /// The largest undistorted swing against frequency (full-power
    /// bandwidth): [`swing_at`].
    fn swing(table: &str, part: Macromodel) -> Family {
        let vcc = Datasheet::load(table)
            .condition("vcc")
            .expect("the chart's supply");
        let model = swing_at(part, vcc);
        linear(table, move |_, f| model(f))
    }

    /// E11: the TL072 fitted to SLOS080W's curves (the drawn pulse's
    /// asymmetric slew, the open-loop plot).
    #[test]
    fn tl072() {
        let part = Macromodel::TL072;
        let [slew, gbw] = two(figures("tl072_figures", part));
        // This revision's table prints 20 V/µs; its own drawn pulse rises
        // at 9.9 and falls at 12.8 V/µs. The model follows the drawing,
        // so the table figure stays a known gap (35.0% when the model
        // took SLOS080J's 13 V/µs; the rising edge's 9.9 measures 50.7%).
        gap(slew, 50.7);
        pass(gbw);
        let [rising, falling] = two(pulse("tl072_pulse", part));
        pass(rising);
        pass(falling);
        // The plot is drawn as its asymptotes with a sharp knee; one real
        // pole sits 3 dB under the drawn corner (38.0% at SLOS080J's
        // figures, 22.0% fitted).
        gap(
            open_loop("tl072_open_loop", part).of("TL072", "Macromodel TL072"),
            22.0,
        );
    }

    /// SLOS080J's table figures, the TL072 until E11: symmetric slew.
    #[test]
    fn tl072_slos080j() {
        let part = Macromodel::TL072_SLOS080J;
        let [slew, gbw] = two(figures("tl072_figures", part));
        // The macromodel takes SLOS080J's 13 V/µs; this revision's table
        // prints 20 V/µs for the same TL07xC rows.
        gap(slew, 35.0);
        pass(gbw);
        // The drawn follower response rises at ~10 V/µs and falls at
        // ~12.8: the classic part slews asymmetrically, this card does not.
        let [rising, falling] = two(pulse("tl072_pulse", part));
        gap(rising, 31.8);
        pass(falling);
        // The plotted open loop's dominant pole sits above the card's
        // GBW/A0 (15 Hz): 38% low at 20 Hz.
        gap(
            open_loop("tl072_open_loop", part).of("TL072", "Macromodel TL072_SLOS080J"),
            38.0,
        );
    }

    /// E11: the JRC4558 fitted to the sheet's typical curves, which
    /// contradict its table (110 dB and ~1.9 V/µs against 100 dB and
    /// 1 V/µs): the curves are what the part does to a signal.
    #[test]
    fn jrc4558() {
        let part = Macromodel::JRC4558;
        let [slew, gbw] = two(figures("jrc4558_figures", part));
        // The table's 1 V/µs, which the swing plot contradicts.
        gap(slew, 90.1);
        pass(gbw);
        pass(open_loop("jrc4558_open_loop", part).of("JRC4558", "Macromodel JRC4558"));
        // The plotted swing implies a slew rate that climbs with frequency
        // (1.7 V/µs past the knee, 3.4 V/µs at 300 kHz, where the
        // gain-bandwidth also limits); one constant rate cannot follow it.
        gap(
            swing("jrc4558_swing", part).of("JRC4558", "Macromodel JRC4558"),
            7.4,
        );
    }

    /// The table's figures, the JRC4558 until E11.
    #[test]
    fn jrc4558_table() {
        let part = Macromodel::JRC4558_TABLE;
        for figure in figures("jrc4558_figures", part) {
            pass(figure);
        }
        // The plots contradict the table: 110 dB DC gain (the table's,
        // and this card's, 100 dB), and a swing knee implying ~1.8 V/µs
        // (the table's, and this card's, 1 V/µs).
        gap(
            open_loop("jrc4558_open_loop", part).of("JRC4558", "Macromodel JRC4558_TABLE"),
            214.4,
        );
        gap(
            swing("jrc4558_swing", part).of("JRC4558", "Macromodel JRC4558_TABLE"),
            40.8,
        );
    }

    /// The LM741C's table slew rate and its rise-time bandwidth (the
    /// sheet's own 0.35 / rise time), read back from a follower.
    #[test]
    fn lm741() {
        for figure in figures("lm741_figures", Macromodel::LM741) {
            pass(figure);
        }
    }

    /// The NJM2904's table slew rate and unity-gain bandwidth, read back
    /// from a follower. Its swing plot's knee implies a slower slew
    /// (about 0.22 V/µs) than the table's 0.5; the model follows the
    /// table.
    #[test]
    fn njm2904() {
        for figure in figures("njm2904_figures", Macromodel::NJM2904) {
            pass(figure);
        }
    }
}

mod otas {
    use stave_dsp::circuit::devices::ota::BipolarPair;

    use super::{linear, logarithmic, pass};

    fn check(part: &str, ota: BipolarPair) {
        let model = format!("BipolarPair {}", ota.name);
        let lower = part.to_ascii_lowercase();
        pass(logarithmic(&format!("{lower}_gm"), |_, i| ota.transconductance(i)).of(part, &model));
        pass(linear(&format!("{lower}_bias_volts"), |_, i| ota.bias_volts_at(i)).of(part, &model));
    }

    #[test]
    fn ca3080() {
        check("CA3080", BipolarPair::CA3080);
    }

    #[test]
    fn lm13700() {
        check("LM13700", BipolarPair::LM13700);
    }
}

mod compandors {
    use stave_dsp::circuit::devices::compander::{Chip, Compander, Mode};
    use stave_dsp::Stage;
    use stave_test::reference::Datasheet;

    use super::{limit, linear, pass, Measured, LOG_TOLERANCE};

    /// 0 dBm, volts RMS.
    const DBM_0: f64 = 0.774_6;
    /// The rectifier's input resistor, `R1` (Figure 1).
    const R1: f64 = 10e3;

    /// The NE570's output stage, cell and rectifier against its table
    /// and text figures, and its cell against Figure 15.
    #[test]
    fn ne570() {
        let chip = Chip::NE570;
        let model = "Compander NE570";
        // Figure 15: the cell's largest output at each gain, in dBm.
        let cell = Compander::new(chip, Mode::Expand);
        pass(
            linear("ne570_max_signal", |_, gain_db| {
                let peak = cell.vca(1e3, 10f64.powf(gain_db / 20.0));
                20.0 * (peak / std::f64::consts::SQRT_2 / DBM_0).log10()
            })
            .of("NE570", model),
        );
        let sheet = Datasheet::load("ne570_figures");
        let figure = |key: &str| sheet.curve(&[(key, 1.0)]).reference.values()[0];
        let printed = |what: &str, value: f64, want: f64| {
            let error = (value / want - 1.0).abs();
            pass(Measured {
                what: format!("NE570 | {model} | ne570_figures {what} (sheet {want:e}, model {value:e})"),
                error: 100.0 * error,
                budget: 100.0 * LOG_TOLERANCE,
                unit: "% of value",
            });
        };
        // Slew: a hot step from rest at 768 kHz, the output's climb.
        let rate = 768_000.0f32;
        let mut c = Compander::new(chip, Mode::Compress);
        c.prepare(rate);
        let y: Vec<f64> = (0..4).map(|_| c.tick_f64(1.0)).collect();
        printed("slew rate", (y[3] - y[0]) / 3.0 * f64::from(rate), figure("slew"));
        // Cell: the cell at unity gain, driven far past saturation.
        printed("cell saturation", cell.vca(1e3, 1.0), figure("cell"));
        // Rectifier: the expander's averaged input under 20 V DC, as a
        // current into R1.
        let mut e = Compander::new(chip, Mode::Expand);
        e.prepare(48_000.0);
        for _ in 0..48_000 {
            e.tick_f64(20.0);
        }
        let average = e.gain() * chip.unity.get() * 2.0 * std::f64::consts::SQRT_2 / std::f64::consts::PI;
        printed("rectifier saturation", average / R1, figure("rectifier"));
        // Swing: a hot square after silence, its peak; at least the
        // sheet's +13 dBm into 300 Ω, at most half the 15 V supply.
        let mut c = Compander::new(chip, Mode::Compress);
        c.prepare(48_000.0);
        let peak = (0..4_800)
            .map(|i| c.tick_f64(if i % 48 < 24 { 20.0 } else { -20.0 }).abs())
            .fold(0.0, f64::max);
        limit("NE570", model, "output swing, peak (V)", 15.0, peak, figure("swing"), 7.5);
    }
}
