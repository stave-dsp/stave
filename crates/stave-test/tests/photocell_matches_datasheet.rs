//! The photocell parts against their datasheets (DSP.md E7, R13, R23),
//! and the opto compressor that uses one.
//!
//! The tables under `crates/stave-test/datasheet/` (`vtl5c3_*`,
//! `nsl32_*`) were digitised once from the figures their headers cite.
//! As in `matches_datasheet.rs`, a budget is the reading's uncertainty
//! plus a stated model tolerance:
//!
//! - **Transfer curves** (resistance against LED current, log axes):
//!   ln(R) within ln(1 + reading) + ln(1 + [`LOG_TOLERANCE`]), the model
//!   read anywhere across the current's uncertainty;
//! - **LED** forward voltage at each current: the reading plus
//!   [`DIODE_VOLTS`];
//! - **Response times** (turn-on to 63% of the final conductance,
//!   turn-off to 100 kΩ), read off the response curves by log
//!   interpolation: within [`TIME_TOLERANCE`]. A CdS cell's times vary
//!   by tens of percent between units and with the light history the
//!   sheet does not state (the VTL5C3's table and its own figure differ
//!   by 30% on the turn-on), so this budget is wide on purpose; the
//!   whole curves are reported, not held.
//!
//! A miss is a **known gap**: asserted still a miss, and no worse than
//! when recorded, so a refit fails here until promoted.

use stave_dsp::circuit::devices::photocell::{LedDriver, Najnudel, Vactrol};
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::topologies::opto_compressor::{
    opto_compressor_cell, opto_compressor_side_chain, OptoCompressor,
};
use stave_dsp::circuit::Circuit;
use stave_test::reference::{Datasheet, Uncertainty};

/// Model tolerance on a log axis, a fraction of the value.
const LOG_TOLERANCE: f64 = 0.10;
/// Model tolerance on an LED's forward voltage, volts.
const DIODE_VOLTS: f64 = 0.020;
/// Model tolerance on a response time, a fraction of the time.
const TIME_TOLERANCE: f64 = 0.35;
/// The rate the transients run at: the cell is milliseconds, so any
/// audio rate resolves it (`the_cell_converges_with_rate`).
const RATE: f64 = 96_000.0;

fn report(what: &str, error_pct: f64, budget_pct: f64, verdict: &str) {
    println!("E7 | {what} | {error_pct:.1}% | budget {budget_pct:.1}% | {verdict}");
}

/// Hold a transfer curve: worst |ln(model/reading)| across the table,
/// the model read at the reading's current ± its uncertainty.
fn transfer(table: &str, label: &str, model: impl Fn(f64) -> f64) -> (f64, f64) {
    let sheet = Datasheet::load(table);
    let Uncertainty::Relative(ux) = sheet.uncertainty_x() else {
        panic!("{table}: a log current axis");
    };
    let Uncertainty::Relative(uy) = sheet.uncertainty_y() else {
        panic!("{table}: a log resistance axis");
    };
    let curve = sheet
        .curves()
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("{table}: no curve {label}"));
    let r = &curve.reference;
    let mut worst: f64 = 0.0;
    for (&amps, &ohms) in r.abscissa().iter().zip(r.values()) {
        let e = (0..=8)
            .map(|k| model(amps * (1.0 + ux * (f64::from(k) / 4.0 - 1.0))))
            .map(|m| (m / ohms).ln().abs())
            .fold(f64::INFINITY, f64::min);
        worst = worst.max(e);
    }
    (worst, uy.ln_1p() + LOG_TOLERANCE.ln_1p())
}

fn settle(part: Najnudel, adaptation: f64) -> impl Fn(f64) -> f64 {
    move |amps| {
        let mut v = Vactrol::new(part);
        v.set_adaptation(adaptation);
        v.settle(amps)
    }
}

#[track_caller]
fn pass(what: &str, error: f64, budget: f64) {
    let (e, b) = (100.0 * error.exp_m1(), 100.0 * budget.exp_m1());
    report(what, e, b, if error <= budget { "pass" } else { "FAILED" });
    assert!(error <= budget, "{what}: {e:.1}% > {b:.1}%");
}

/// A known gap: still a miss, no worse than `recorded` (nepers).
#[track_caller]
fn known_gap(what: &str, error: f64, budget: f64, recorded: f64) {
    let (e, b) = (100.0 * error.exp_m1(), 100.0 * budget.exp_m1());
    report(what, e, b, "known gap");
    assert!(error > budget, "{what}: {e:.1}% now passes {b:.1}%: promote it to a pass");
    assert!(error <= recorded * 1.01, "{what}: {e:.1}% is worse than recorded");
}

#[test]
fn vtl5c3_matches_its_transfer_curves() {
    // Curve 1: 24 hours dark; curve 2: 24 hours at 40 mA (the model's
    // light history at 0 and 1). Curves 3 and 4 are +50 and -20 C: no
    // temperature is modelled.
    for (label, adaptation) in [("adapt=1", 0.0), ("adapt=2", 1.0)] {
        let (e, b) = transfer("vtl5c3_resistance", label, settle(Najnudel::VTL5C3, adaptation));
        pass(&format!("VTL5C3 | resistance vs LED current [{label}]"), e, b);
    }
}

#[test]
fn nsl32_matches_its_transfer_curve() {
    let (e, b) = transfer("nsl32_resistance", "t=23", settle(Najnudel::NSL32, 0.0));
    pass("NSL-32 | resistance vs LED current", e, b);
    // The table's printed points: 140 ohm typical at 1 mA (the curve
    // reads 123), 40 ohm maximum at 20 mA.
    let at = settle(Najnudel::NSL32, 0.0);
    let typical = (at(1e-3) / 140.0).ln().abs();
    known_gap("NSL-32 | 140 ohm typical at 1 mA (table; the curve reads 123)", typical, LOG_TOLERANCE.ln_1p(), 0.14);
    assert!(at(20e-3) <= 40.0, "40 ohm maximum at 20 mA: {}", at(20e-3));
}

#[test]
fn vtl5c3_led_matches_its_input_characteristic() {
    let sheet = Datasheet::load("vtl5c3_led");
    let curve = &sheet.curves()[0].reference;
    let budget = sheet.uncertainty_x().at(0.0) + DIODE_VOLTS;
    let worst = curve
        .abscissa()
        .iter()
        .zip(curve.values())
        .map(|(&v, &i)| (Najnudel::VTL5C3.led_volts(i) - v).abs())
        .fold(0.0, f64::max);
    println!("E7 | VTL5C3 | LED forward voltage | {:.1} mV | budget {:.1} mV | {}", worst * 1e3, budget * 1e3, if worst <= budget { "pass" } else { "FAILED" });
    assert!(worst <= budget);
}

/// The time `curve` (seconds, ohms) first crosses `ohms`, by log
/// interpolation between readings.
fn crossing(points: &[(f64, f64)], ohms: f64) -> f64 {
    for w in points.windows(2) {
        let ((t0, r0), (t1, r1)) = (w[0], w[1]);
        if (r0 - ohms) * (r1 - ohms) <= 0.0 {
            let f = (ohms / r0).ln() / (r1 / r0).ln();
            return t0 + f * (t1 - t0);
        }
    }
    panic!("the curve never crosses {ohms}");
}

fn points(table: &str, label: &str) -> Vec<(f64, f64)> {
    let sheet = Datasheet::load(table);
    let r = &sheet
        .curves()
        .iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("{table}: no curve {label}"))
        .reference;
    r.abscissa().iter().copied().zip(r.values().iter().copied()).collect()
}

/// The model's turn-on: dark (and `adaptation`), then `amps`, until 63%
/// of the final conductance.
fn model_turn_on(part: Najnudel, amps: f64) -> f64 {
    let final_ohms = Vactrol::new(part).settle(amps);
    let mut v = Vactrol::new(part);
    v.prepare(RATE as f32);
    let target = final_ohms / 0.63;
    (1..)
        .find(|_| v.step(amps) <= target)
        .map(|n| n as f64 / RATE)
        .expect("turns on")
}

/// The model's turn-off: at rest under `amps`, then dark, until `ohms`.
fn model_turn_off(part: Najnudel, amps: f64, ohms: f64) -> f64 {
    let mut v = Vactrol::new(part);
    v.prepare(RATE as f32);
    v.settle(amps);
    (1..)
        .find(|_| v.step(0.0) >= ohms)
        .map(|n| n as f64 / RATE)
        .expect("turns off")
}

#[test]
fn vtl5c3_matches_its_response_times() {
    let budget = TIME_TOLERANCE.ln_1p();
    for amps in [0.04, 0.01] {
        let label = format!("led={amps}");
        let on = points("vtl5c3_turn_on", &label);
        let final_ohms = on.last().expect("points").1;
        let sheet_on = crossing(&on, final_ohms / 0.63);
        let sheet_off = crossing(&points("vtl5c3_turn_off", &label), 1e5);
        let on_err = (model_turn_on(Najnudel::VTL5C3, amps) / sheet_on).ln().abs();
        let off_err = (model_turn_off(Najnudel::VTL5C3, amps, 1e5) / sheet_off).ln().abs();
        pass(&format!("VTL5C3 | turn-on to 63% at {} mA ({:.2} ms read)", amps * 1e3, sheet_on * 1e3), on_err, budget);
        pass(&format!("VTL5C3 | turn-off to 100 kohm from {} mA ({:.1} ms read)", amps * 1e3, sheet_off * 1e3), off_err, budget);
    }
    // The electrical table: turn-off to 100 kohm 35 ms maximum.
    let slowest = model_turn_off(Najnudel::VTL5C3, 0.04, 1e5);
    assert!(slowest <= 35e-3, "35 ms maximum: {slowest}");
    // The whole curves, reported: worst ln error at the readings.
    for (table, on) in [("vtl5c3_turn_on", true), ("vtl5c3_turn_off", false)] {
        for amps in [0.04, 0.01] {
            let pts = points(table, &format!("led={amps}"));
            let mut v = Vactrol::new(Najnudel::VTL5C3);
            v.prepare(RATE as f32);
            let drive = if on { amps } else {
                v.settle(amps);
                0.0
            };
            let mut n = 0usize;
            let mut worst: f64 = 0.0;
            for (t, r) in pts {
                while (n as f64) < t * RATE {
                    v.step(drive);
                    n += 1;
                }
                worst = worst.max((v.ohms() / r).ln().abs());
            }
            println!("E7 | VTL5C3 | {table} [{amps} A] whole curve | worst {:.0}% | reported", 100.0 * worst.exp_m1());
        }
    }
}

#[test]
fn nsl32_matches_its_response_times() {
    let budget = TIME_TOLERANCE.ln_1p();
    let off = (model_turn_off(Najnudel::NSL32, 0.016, 1e5) / 5e-3).ln().abs();
    let on = (model_turn_on(Najnudel::NSL32, 0.020) / 5e-3).ln().abs();
    pass("NSL-32 | decay from 16 mA to 100 kohm (5 ms printed)", off, budget);
    // No CdS cell turns off as fast as it turns on; the sheet prints 5 ms
    // for both, the fit holds the release (scripts/vactrol-fit.py).
    known_gap("NSL-32 | rise to 63% (5 ms printed, at an unstated current)", on, budget, 2.0);
}

#[test]
fn opto_compressor_netlists_solve_as_the_closed_forms() {
    // The divider: R1 = 1 k over the cell.
    let mut cell = Circuit::<Dk>::new(&opto_compressor_cell()).unwrap();
    cell.drive("vin").unwrap();
    cell.prepare(48_000.0);
    let ldr = cell.variable_resistor("rldr").unwrap();
    cell.set_smoothing(stave_dsp::circuit::Smoothing::NONE);
    cell.set_ohms(ldr, stave_dsp::units::Ohms::new(3_000.0));
    cell.settle();
    let out = cell.tick_f64(2.0);
    assert!((out - 2.0 * 3_000.0 / 4_000.0).abs() < 1e-12, "{out}");
    // The side chain at a held drive: the LED current is the closed form
    // through R2 (the capacitor charged).
    let led = Najnudel::VTL5C3.led;
    let mut side = Circuit::<Dk>::new(&opto_compressor_side_chain(led)).unwrap();
    side.drive("vout").unwrap();
    side.prepare(48_000.0);
    for volts in [1.6, 2.0, 3.0] {
        let amps = (0..48_000).map(|_| side.tick_f64(volts)).last().unwrap() / led.series_ohms;
        let closed = LedDriver::new(led, 5.0).amps(volts);
        assert!((amps / closed - 1.0).abs() < 1e-6, "{volts} V: {amps} vs {closed}");
    }
    assert_eq!(side.stats().capped, 0);
}

/// Najnudel et al.'s Fig. 9 protocol: 1 kHz at 1 V, a 10 ms burst at
/// `burst` volts, then 1 V again, at 96 kHz; the output's peak every
/// half cycle. Returns (attack, release): from the burst's start until
/// its gain reduction (dB) reaches 63% of the reduction at the burst's
/// end; from the burst's end until the output is back within 1 dB of
/// its steady 1 V.
fn fig9(burst: f64) -> (f64, f64) {
    let rate = 96_000.0;
    let half = 48usize;
    let mut comp = OptoCompressor::new(Najnudel::VTL5C3);
    stave_dsp::Stage::prepare(&mut comp, rate as f32);
    let halves = 2 * (10 + 10 + 400);
    let (on, off) = (20, 40);
    let mut gain_db = Vec::new();
    for h in 0..halves {
        let u = if (on..off).contains(&h) { burst } else { 1.0 };
        let mut peak: f64 = 0.0;
        for k in 0..half {
            let n = h * half + k;
            let x = u * (std::f64::consts::TAU * n as f64 / (2 * half) as f64).sin();
            peak = peak.max(comp.tick_f64(x).abs());
        }
        gain_db.push(20.0 * (peak / u).log10());
    }
    let seconds = |halves: usize| halves as f64 * half as f64 / rate;
    let reduced = gain_db[off - 1];
    let attack = (on..off).find(|&h| gain_db[h] <= 0.63 * reduced).unwrap_or(off) - on;
    let release = (off..halves).find(|&h| gain_db[h] > -1.0).unwrap_or(halves) - off;
    println!(
        "E7 | opto compressor | {burst} V burst: {reduced:.1} dB of gain reduction at its end, attack {:.1} ms, release {:.1} ms | reported",
        1e3 * seconds(attack),
        1e3 * seconds(release)
    );
    (seconds(attack), seconds(release))
}

#[test]
fn opto_compressor_attacks_faster_and_releases_slower_when_driven_harder() {
    // The paper (Fig. 9a-c, U0 = 3, 6, 12 V): "the higher the input
    // voltage, the shorter the attack and the longer the release".
    let runs: Vec<(f64, f64)> = [3.0, 6.0, 12.0].iter().map(|&u| fig9(u)).collect();
    for w in runs.windows(2) {
        assert!(w[1].0 <= w[0].0, "attack shortens with level: {runs:?}");
        assert!(w[1].1 > w[0].1, "release lengthens with level: {runs:?}");
    }
}
