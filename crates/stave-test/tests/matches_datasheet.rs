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

/// The model value across `x ± ux` nearest the reading `y`.
fn nearest(x: f64, y: f64, ux: f64, model: &impl Fn(f64) -> f64) -> f64 {
    (0..=8)
        .map(|k| model(x + ux * (f64::from(k) / 4.0 - 1.0)))
        .min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs()))
        .expect("nine samples")
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

/// The root of an increasing `f` in `[lo, hi]` by bisection, to 1e-12.
fn bisect(mut lo: f64, mut hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    0.5 * (lo + hi)
}

// ----------------------------------------------------------- transistors

mod bjt {
    use stave_dsp::circuit::devices::bjt::{Bjt, GummelPoon};

    use super::bisect;

    /// Collector and base current magnitudes at intrinsic junction
    /// magnitudes (`vbe`, `vce`), N-sense for either polarity.
    fn intrinsic(q: &GummelPoon, vbe: f64, vce: f64) -> (f64, f64) {
        let s = q.polarity.sign();
        let c = q.currents(s * vbe, s * (vbe - vce));
        (s * c.collector, s * c.base)
    }

    /// The intrinsic `vbe` that draws base current `ib` at intrinsic `vce`.
    fn vbe_for_base(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        bisect(-0.5, 1.5, |vbe| intrinsic(q, vbe, vce).1 - ib)
    }

    /// The operating point with base current |Ib| forced at terminal
    /// |Vce| (a curve tracer): `(intrinsic vce, intrinsic vbe, |Ic|)`,
    /// the card's ohmic emitter and collector resistances included.
    fn operating(q: &GummelPoon, ib: f64, vce: f64) -> (f64, f64, f64) {
        let at = |vce_int: f64| {
            let vbe = vbe_for_base(q, ib, vce_int);
            (vbe, intrinsic(q, vbe, vce_int).0)
        };
        let terminal = |vce_int: f64| {
            let ic = at(vce_int).1;
            vce_int + ic * q.collector_ohms + (ic + ib) * q.emitter_ohms
        };
        let vce_int = bisect(-0.5, vce, |v| terminal(v) - vce);
        let (vbe, ic) = at(vce_int);
        (vce_int, vbe, ic)
    }

    /// |Ic| at terminal |Vce| with |Ib| forced: the output family.
    pub fn collector_at(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        operating(q, ib, vce).2
    }

    /// Terminal |VBE| with |Ib| forced at terminal |Vce|: the junction's
    /// plus the base and emitter resistances' drops.
    pub fn terminal_vbe(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        let (_, vbe, ic) = operating(q, ib, vce);
        vbe + ib * q.base_ohms + (ic + ib) * q.emitter_ohms
    }

    /// The terminal |Vce| at which |Ib| draws |Ic| (the saturation
    /// region); 5 V when it cannot below that.
    pub fn vce_for(q: &GummelPoon, ib: f64, ic: f64) -> f64 {
        bisect(0.0, 5.0, |vce| collector_at(q, ib, vce) - ic)
    }

    /// DC current gain Ic/Ib at |Ic| and terminal |Vce|.
    pub fn hfe_at(q: &GummelPoon, ic: f64, vce: f64) -> f64 {
        let ib = bisect(0.0, ic, |ib| collector_at(q, ib, vce) - ic);
        ic / ib
    }
}

/// A diode's forward voltage at `amps`: the junction's plus the series
/// resistance's drop.
fn diode_volts(d: &stave_dsp::circuit::devices::diode::Shockley, amps: f64) -> f64 {
    let junction = bisect(-1.0, 3.0, |v| d.current(v) - amps);
    junction + amps * d.series_ohms
}

// --------------------------------------------------------------- op-amps

mod opamp {
    use stave_dsp::circuit::devices::opamp::{Amp, Macromodel};
    use stave_dsp::circuit::methods::dk::Dk;
    use stave_dsp::circuit::netlist::{ElementKind, Netlist, Node, Probe, Source};

    /// A unity follower of `part` on `supply`, its input source driven.
    pub fn follower(part: Macromodel, supply: (f64, f64)) -> Dk {
        let mut n = Netlist::new();
        let (input, out) = (n.add_node("in"), n.add_node("out"));
        n.add(
            "v1",
            ElementKind::VoltageSource {
                a: input,
                b: Node::GROUND,
                source: Source::dc(0.0),
            },
        )
        .unwrap();
        n.add(
            "u1",
            ElementKind::OpAmp {
                plus: input,
                minus: out,
                output: out,
                amp: Amp::Macromodel(part),
                supply,
            },
        )
        .unwrap();
        n.probe = Some(Probe::node(out));
        let mut dk = Dk::new(&n).unwrap();
        dk.drive("v1").unwrap();
        dk
    }

    /// The follower's output at each of `times` (seconds, ascending) for
    /// an input `input(t)`, stepped at `rate` and read by linear
    /// interpolation.
    pub fn response(dk: &mut Dk, rate: f64, input: impl Fn(f64) -> f64, times: &[f64]) -> Vec<f64> {
        dk.prepare(rate as f32);
        let end = times.iter().copied().fold(0.0, f64::max);
        let steps = (end * rate).ceil() as usize + 1;
        let trace: Vec<f64> = (0..=steps)
            .map(|i| dk.step(input(i as f64 / rate)))
            .collect();
        times
            .iter()
            .map(|&t| {
                let k = t * rate;
                let i = (k.floor() as usize).min(steps - 1);
                let frac = k - i as f64;
                trace[i] + frac * (trace[i + 1] - trace[i])
            })
            .collect()
    }

    /// The largest output slope (V/s) of the follower for a step from
    /// `from` to `to` volts on `supply`, stepped at 1 GHz.
    pub fn slew(part: Macromodel, supply: (f64, f64), from: f64, to: f64) -> f64 {
        let mut dk = follower(part, supply);
        let rate = 1e9;
        // Settle at `from` (the driven source rests at 0), then step.
        let start = 1e-6 + 2.0 * from.abs() / part.slew_rate;
        let swing = (to - from).abs();
        let times: Vec<f64> = (0..=(4.0 * swing / part.slew_rate * rate) as usize)
            .map(|i| start + i as f64 / rate)
            .collect();
        let out = response(&mut dk, rate, |t| if t < start { from } else { to }, &times);
        out.windows(2)
            .map(|w| (w[1] - w[0]).abs() * rate)
            .fold(0.0, f64::max)
    }
}

// ----------------------------------------------------------------- tubes

mod tubes {
    use stave_dsp::circuit::devices::pentode::{koren::Koren as KorenPentode, Pentode};
    use stave_dsp::circuit::devices::triode::dempwolf::Dempwolf;
    use stave_dsp::circuit::devices::triode::koren::Koren;
    use stave_dsp::circuit::devices::triode::Triode;

    use super::{gap, linear, pass};

    fn triode(table: &str, tube: impl Triode) -> super::Family {
        linear(table, |c, va| tube.currents(c.parameter("vg"), va).plate)
    }

    /// Plate current at the table's screen voltage (a condition, or a
    /// curve's own).
    fn plate(table: &str, tube: KorenPentode) -> super::Family {
        let vg2 = stave_test::reference::Datasheet::load(table).condition("vg2");
        linear(table, move |c, va| {
            let vg2 = c.get("vg2").or(vg2).expect("a screen voltage");
            tube.currents(c.parameter("vg1"), vg2, va).plate
        })
    }

    fn screen(table: &str, tube: KorenPentode) -> super::Family {
        let vg2 = stave_test::reference::Datasheet::load(table).condition("vg2");
        linear(table, move |c, va| {
            let vg2 = c.get("vg2").or(vg2).expect("a screen voltage");
            tube.currents(c.parameter("vg1"), vg2, va).screen
        })
    }

    #[test]
    fn ecc83_koren() {
        gap(
            triode("ecc83_plate", Koren::T12AX7).of("12AX7/ECC83", "Koren T12AX7"),
            11.7,
        );
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
        let f = linear("el34_screen", |c, vg1| {
            KorenPentode::EL34
                .currents(vg1, c.parameter("vg2"), c.parameter("va"))
                .screen
        });
        gap(f.of("EL34", "Koren EL34 (screen)"), 53.7);
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

    #[test]
    fn kt88_plate() {
        gap(
            plate("kt88_plate", KorenPentode::KT88).of("KT88", "Koren KT88"),
            14.1,
        );
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
}

// ------------------------------------------------- transistors and diodes

mod transistors {
    use stave_dsp::circuit::devices::bjt::{EbersMoll, GummelPoon};

    use super::bjt::{collector_at, hfe_at, terminal_vbe, vce_for};
    use super::{gap, limit, linear, logarithmic, Family};

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
        // The table's hFE limits at VCE = 1 V (onsemi 2N3903/D).
        for (ic, min, max) in [
            (0.1e-3, 40.0, f64::INFINITY),
            (1e-3, 70.0, f64::INFINITY),
            (10e-3, 100.0, 300.0),
            (50e-3, 60.0, f64::INFINITY),
            (100e-3, 30.0, f64::INFINITY),
        ] {
            limit(
                "2N3904",
                "Gummel–Poon N2N3904",
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
    #[test]
    fn ac128_output() {
        let q = GummelPoon::AC128;
        let f = linear("ac128_output", |c, vce| {
            collector_at(&q, c.parameter("ib"), vce)
        });
        gap(f.of(AC128.0, AC128.1), 25.0);
    }

    /// hFE against |IC| at |VCE| = 1 V, and the table's static hFE limits
    /// at VCB = 0.
    #[test]
    fn ac128_hfe() {
        let q = GummelPoon::AC128;
        let f = linear("ac128_hfe", |c, ic| hfe_at(&q, ic, c.parameter("vce")));
        gap(f.of(AC128.0, AC128.1), 11.9);
        // VCB = 0: |VCE| = |VBE|, solved with the operating point.
        for (ic, min, max) in [
            (50e-3, 55.0, 175.0),
            (300e-3, 55.0, 175.0),
            (1.0, 45.0, 165.0),
        ] {
            let ib = super::bisect(0.0, ic, |ib| {
                let vce = super::bisect(0.0, 2.0, |v| v - terminal_vbe(&q, ib, v));
                collector_at(&q, ib, vce) - ic
            });
            limit(AC128.0, AC128.1, "hFE (VCB = 0)", ic, ic / ib, min, max);
        }
    }

    /// |IC| against |IB| at |VCE| = 1 V.
    #[test]
    fn ac128_transfer() {
        let q = GummelPoon::AC128;
        let f = linear("ac128_transfer", |c, ib| {
            collector_at(&q, ib, c.parameter("vce"))
        });
        gap(f.of(AC128.0, AC128.1), 24.3);
    }

    /// |IB| against |VBE| at |VCE| = 1 V.
    #[test]
    fn ac128_input() {
        let q = GummelPoon::AC128;
        let f = linear("ac128_input", |c, vbe| {
            let vce = c.parameter("vce");
            super::bisect(0.0, 0.1, |ib| terminal_vbe(&q, ib, vce) - vbe)
        });
        gap(f.of(AC128.0, AC128.1), 45.2);
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

    use super::{diode_volts, gap, limit, Measured, DIODE_VOLTS};

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

    use super::opamp::{follower, response, slew};
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

    /// The least-squares slope (V/s) of the points of `(t, v)` between
    /// 20% and 80% of the way from `low` to `high`, within `[from, to]`.
    fn edge_slope(t: &[f64], v: &[f64], low: f64, high: f64, from: f64, to: f64) -> f64 {
        let span = high - low;
        let inside: Vec<(f64, f64)> = t
            .iter()
            .zip(v)
            .filter(|(&t, &v)| {
                let k = (v - low) / span;
                t >= from && t <= to && (0.2..=0.8).contains(&k)
            })
            .map(|(&t, &v)| (t, v))
            .collect();
        assert!(inside.len() >= 3, "an edge needs points: {inside:?}");
        let n = inside.len() as f64;
        let (mt, mv) = inside
            .iter()
            .fold((0.0, 0.0), |(a, b), (t, v)| (a + t / n, b + v / n));
        let (num, den) = inside.iter().fold((0.0, 0.0), |(a, b), (t, v)| {
            (a + (t - mt) * (v - mv), b + (t - mt) * (t - mt))
        });
        num / den
    }

    /// The voltage-follower pulse response: the model driven by the
    /// sheet's drawn input, and each edge's slew (20–80% of the swing,
    /// least squares) read off both the same way. The drawing is
    /// idealised (straight segments, a drawn step at the edge's start),
    /// so its slopes are compared, not its points.
    fn pulse(table: &str, part: Macromodel) -> Vec<Measured> {
        let sheet = Datasheet::load(table);
        let vcc = sheet.condition("vcc").expect("the chart's supply");
        let output = &sheet.curve(&[("output", 1.0)]).reference;
        let input = &sheet.curve(&[("input", 1.0)]).reference;
        let (low, high) = (
            input.values()[0],
            input.values().iter().copied().fold(f64::MIN, f64::max),
        );
        // The input's falling edge: between its last high and first low point after the rise.
        let fall = {
            let (t, v) = (input.abscissa(), input.values());
            let i = (1..v.len())
                .find(|&i| t[i] > 0.0 && v[i] < 0.5 * (low + high))
                .expect("a falling edge");
            0.5 * (t[i - 1] + t[i])
        };
        let start = -output.abscissa()[0] + 2.0 * (high - low) / part.slew_rate;
        let mut dk = follower(part, (-vcc, vcc));
        let rate = 1e9;
        let times: Vec<f64> = output.abscissa().iter().map(|t| t + start).collect();
        let drive = |t: f64| {
            let t = t - start;
            if t < -0.5 * start {
                0.0
            } else if (0.0..fall).contains(&t) {
                high
            } else {
                low
            }
        };
        let model = response(&mut dk, rate, drive, &times);
        let t = output.abscissa();
        let ux = sheet.uncertainty_x().at(0.0);
        let uy = sheet.uncertainty_y().at(0.0);
        [("rising", 0.0, fall), ("falling", fall, f64::MAX)]
            .into_iter()
            .map(|(edge, from, to)| {
                let sheet_slope = edge_slope(t, output.values(), low, high, from, to).abs();
                let model_slope = edge_slope(t, &model, low, high, from, to).abs();
                // The slope's reading uncertainty: a point's x and y error
                // over the 20-80% edge's extent.
                let rise = 0.6 * (high - low);
                let reading = 2.0 * uy / rise + 2.0 * ux * sheet_slope / rise;
                let budget = reading + LOG_TOLERANCE;
                let error = (model_slope / sheet_slope - 1.0).abs();
                Measured {
                    what: format!(
                        "{} | Macromodel {} | {table} {edge} edge (sheet {:.2} V/µs, model {:.2} V/µs)",
                        part.name,
                        part.name,
                        sheet_slope * 1e-6,
                        model_slope * 1e-6
                    ),
                    error: 100.0 * error,
                    budget: 100.0 * budget,
                    unit: "% of value",
                }
            })
            .collect()
    }

    /// The largest undistorted swing against frequency (full-power
    /// bandwidth): the model's own slew (simulated) as SR/(π·f), capped
    /// at its own simulated output swing.
    fn swing(table: &str, part: Macromodel) -> Family {
        let vcc = Datasheet::load(table)
            .condition("vcc")
            .expect("the chart's supply");
        let supply = (-vcc, vcc);
        let sr = slew(part, supply, -5.0, 5.0);
        let rails = {
            let mut dk = follower(part, supply);
            let times = [20e-6];
            let up = response(&mut dk, 1e8, |_| 2.0 * vcc, &times)[0];
            let down = response(&mut dk, 1e8, |_| -2.0 * vcc, &times)[0];
            up - down
        };
        linear(table, move |_, f| {
            (sr / (std::f64::consts::PI * f)).min(rails)
        })
    }

    #[test]
    fn tl072() {
        let part = Macromodel::TL072;
        let [slew, gbw] = two(figures("tl072_figures", part));
        // The macromodel takes SLOS080J's 13 V/µs; this revision's table
        // prints 20 V/µs for the same TL07xC rows.
        gap(slew, 35.0);
        pass(gbw);
        // The drawn follower response rises at ~10 V/µs and falls at
        // ~12.8: the classic part slews asymmetrically, the model does not.
        let [rising, falling] = two(pulse("tl072_pulse", part));
        gap(rising, 31.8);
        pass(falling);
        // The plotted open loop's dominant pole sits above the model's
        // GBW/A0 (15 Hz): 38% low at 20 Hz.
        gap(
            open_loop("tl072_open_loop", part).of("TL072", "Macromodel TL072"),
            38.0,
        );
    }

    #[test]
    fn jrc4558() {
        let part = Macromodel::JRC4558;
        for figure in figures("jrc4558_figures", part) {
            pass(figure);
        }
        // The sheet contradicts its own table: the plot's DC gain is
        // 110 dB (the table's, and the model's, 100 dB), and the swing
        // plot's knee implies ~1.8 V/µs (the table's, and the model's,
        // 1 V/µs). The model follows the table.
        gap(
            open_loop("jrc4558_open_loop", part).of("JRC4558", "Macromodel JRC4558"),
            214.4,
        );
        gap(
            swing("jrc4558_swing", part).of("JRC4558", "Macromodel JRC4558"),
            40.8,
        );
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
