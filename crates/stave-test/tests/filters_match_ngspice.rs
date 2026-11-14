//! The ladder filters (DSP.md S1): each circuit — the transistor ladder
//! and the diode ladder, netlists in `stave_dsp::circuit::topologies` —
//! against ngspice on the very deck (`spice/*.cir`) through
//! `Circuit<Dk>`, transient and small-signal; then each fast structural
//! model (`primitives::filters::ladder::{Huovilainen, Stinchcombe}`)
//! against the same ngspice run, so what the structure leaves out of the
//! circuit is a number with a budget.

use std::path::PathBuf;

use stave_dsp::circuit::devices::diode::THERMAL_VOLTS_27C;
use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::topologies::diode_ladder::DiodeLadder;
use stave_dsp::circuit::topologies::transistor_ladder::TransistorLadder;
use stave_dsp::circuit::Circuit;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::units::hz;
use stave_dsp::Stage;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

/// The decks' stimulus: 40 mV at 300 Hz.
fn sine(t: f64) -> f64 {
    0.04 * (std::f64::consts::TAU * 300.0 * t).sin()
}

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// A model run at `oversample` × 48 kHz, read at the reference's
/// instants: `start` at t = 0, then `step(input at the new time)`.
fn run_against(
    reference: &Reference,
    oversample: u32,
    start: f64,
    mut step: impl FnMut(f64) -> f64,
) -> Vec<f64> {
    let rate = 48_000.0 * f64::from(oversample);
    let (mut n, mut out) = (0u64, start);
    reference
        .abscissa()
        .iter()
        .map(|&t| {
            while n < (t * rate).round() as u64 {
                n += 1;
                out = step(sine(n as f64 / rate));
            }
            out
        })
        .collect()
}

fn healthy(name: &str, health: &Health) {
    println!(
        "  DK solve: {:.2} iterations/solve, {} of {} capped, {} continuations",
        health.newton.mean_iterations(),
        health.newton.capped,
        health.newton.solves,
        health.continuations
    );
    assert_eq!(health.newton.capped, 0, "{name} hit its iteration cap");
}

/// The deck as ngspice runs it (its own source), through `Circuit<Dk>`.
fn deck_case(name: &str, oversample: u32, budget: f64) -> Outcome {
    let reference = Reference::ngspice(name);
    let mut circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
    circuit.prepare(48_000.0 * oversample as f32);
    let start = circuit.output();
    let run = run_against(&reference, oversample, start, |_| circuit.step(0.0));
    let outcome = Case::ngspice(name)
        .named(format!("{name}/dk {oversample}x"))
        .budget(Metric::NormalizedRms, budget)
        .compare(&run);
    healthy(name, &circuit.health());
    outcome
}

/// The topology's netlist, its `vin` driven by the deck's sine.
fn topology_case(name: &str, netlist: &Netlist, oversample: u32, budget: f64) -> Outcome {
    let reference = Reference::ngspice(name);
    let mut circuit = Circuit::<Dk>::new(netlist).unwrap();
    circuit.drive("vin").unwrap();
    circuit.prepare(48_000.0 * oversample as f32);
    let start = circuit.output();
    let run = run_against(&reference, oversample, start, |x| circuit.step(x));
    let outcome = Case::ngspice(name)
        .named(format!("{name}/topology dk {oversample}x"))
        .budget(Metric::NormalizedRms, budget)
        .compare(&run);
    healthy(name, &circuit.health());
    outcome
}

/// The netlist's small-signal response through `Circuit<Dk>` against
/// ngspice's AC analysis.
fn ac_case(name: &str, netlist: &Netlist, budget_db: f64) {
    let circuit = Circuit::<Dk>::new(netlist).unwrap();
    Case::ngspice(name)
        .named(format!("{name}/dk"))
        .budget(Metric::MaxDb, budget_db)
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
}

/// Where the netlist's loop (resonance 0) reaches −180°: the resonance at
/// which it self-oscillates (one over the gain there) and its pitch (that
/// frequency), from `Circuit<Dk>`'s small-signal response.
fn onset_and_pitch(netlist: &Netlist) -> (f64, f64) {
    let circuit = Circuit::<Dk>::new(netlist).unwrap();
    let at = |hz: f64| circuit.analog_response(hz).unwrap();
    // A lowpass of positive DC gain is negative real at −180°: its
    // imaginary part turns from negative to positive there.
    let (mut lo, mut hi) = (100.0f64, 100.0f64);
    while at(hi).im < 0.0 {
        lo = hi;
        hi *= 1.01;
        assert!(hi < 1e5, "no −180° crossing");
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if at(mid).im < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let pitch = 0.5 * (lo + hi);
    (1.0 / at(pitch).magnitude(), pitch)
}

/// A structural model on a deck's drive (the input in units of `2·Vt`)
/// against ngspice, at 4 × 48 kHz.
fn structure_case(
    name: &str,
    label: &str,
    budget: f64,
    mut tick: impl FnMut(f64) -> f64,
) -> Outcome {
    let two_vt = 2.0 * THERMAL_VOLTS_27C;
    let reference = Reference::ngspice(name);
    let run = run_against(&reference, 4, 0.0, |x| two_vt * tick(x / two_vt));
    Case::ngspice(name)
        .named(format!("{name}/{label} 4x"))
        .budget(Metric::NormalizedRms, budget)
        .compare(&run)
}

// ------------------------------------------------------ the transistor ladder

/// Ten 2N3904s, resonance 3, 40 mV at 300 Hz: the deck, and the topology
/// driven by the same sine, through `Circuit<Dk>` at 4× and 8×; the error
/// falls as h², so what is left at 4× is the trapezoidal rule's.
/// Measured 6.7e-5 at 4×.
#[test]
fn transistor_ladder_matches_ngspice() {
    let coarse = deck_case("transistor_ladder", 4, 2e-4);
    let fine = deck_case("transistor_ladder", 8, 2e-4);
    assert_converges(&coarse, &fine, 3.0);
    coarse.assert();
    let netlist = TransistorLadder {
        resonance: 3.0,
        ..TransistorLadder::DEFAULT
    }
    .netlist();
    topology_case("transistor_ladder", &netlist, 4, 2e-4).assert();
}

/// The ladder without resonance, small-signal, against ngspice's AC
/// analysis (1.4e-5 dB at worst, at 100 kHz where the response is 1e-8);
/// and where it self-oscillates: near the ideal's resonance 4, 3 % below
/// its ideal pitch — the 2N3904's base current at 50 µA (its card's
/// `Ise` leakage brings β near 90) loses about 1 % of the current a rung.
#[test]
fn transistor_ladder_ac_matches_ngspice() {
    let ladder = TransistorLadder::DEFAULT;
    ac_case("transistor_ladder_ac", &ladder.netlist(), 3e-5);
    let (onset, pitch) = onset_and_pitch(&ladder.netlist());
    let ideal = ladder.ideal_cutoff().get() as f64;
    println!("transistor ladder: onset at resonance {onset:.4} (ideal 4), pitch {pitch:.2} Hz (ideal {ideal:.2})");
    assert!((onset / 4.0 - 1.0).abs() < 0.01, "onset {onset}");
    assert!((pitch / ideal - 1.0).abs() < 0.04, "pitch {pitch} Hz");
}

/// Huovilainen's model on the deck's drive against ngspice's
/// ten-transistor ladder, resonance 3 of 4: at the ideal cutoff
/// `I/(8π·Vt·C)` — what assuming ideal transistors costs, mostly the
/// circuit's 3 % lower pitch — and tuned to the circuit's own
/// small-signal pitch and onset, which leaves the structure's own error.
#[test]
fn huovilainen_matches_the_transistor_ladder() {
    let ladder = TransistorLadder::DEFAULT;
    let mut ideal = Huovilainen::new(hz(ladder.ideal_cutoff().get() as f64 as f32), 0.75);
    ideal.prepare(4.0 * 48_000.0);
    structure_case(
        "transistor_ladder",
        "huovilainen ideal",
        HUOVILAINEN_IDEAL,
        |x| ideal.tick_f64(x),
    )
    .assert();
    let (onset, pitch) = onset_and_pitch(&ladder.netlist());
    let mut tuned = Huovilainen::new(hz(pitch as f32), (3.0 / onset) as f32);
    tuned.prepare(4.0 * 48_000.0);
    structure_case(
        "transistor_ladder",
        "huovilainen tuned",
        HUOVILAINEN_TUNED,
        |x| tuned.tick_f64(x),
    )
    .assert();
    assert_eq!(ideal.stats().capped + tuned.stats().capped, 0);
}
const HUOVILAINEN_IDEAL: f64 = 4e-2;
const HUOVILAINEN_TUNED: f64 = 1e-2;

// ---------------------------------------------------------- the diode ladder

/// Two driven and two top 2N3904s, six junctions, resonance 10, 40 mV at
/// 300 Hz: the deck and the topology through `Circuit<Dk>` (2.6e-5 at 4×).
#[test]
fn diode_ladder_matches_ngspice() {
    let coarse = deck_case("diode_ladder", 4, 2e-4);
    let fine = deck_case("diode_ladder", 8, 2e-4);
    assert_converges(&coarse, &fine, 3.0);
    coarse.assert();
    let netlist = DiodeLadder {
        resonance: 10.0,
        ..DiodeLadder::DEFAULT
    }
    .netlist();
    topology_case("diode_ladder", &netlist, 4, 2e-4).assert();
}

/// The diode ladder small-signal against ngspice's AC analysis; it
/// self-oscillates at Stinchcombe's resonance 17, its pitch near `√2`
/// times the rungs' corner (only the driven pair's base current is lost).
#[test]
fn diode_ladder_ac_matches_ngspice() {
    let ladder = DiodeLadder::DEFAULT;
    ac_case("diode_ladder_ac", &ladder.netlist(), 3e-5);
    let (onset, pitch) = onset_and_pitch(&ladder.netlist());
    let ideal = ladder.ideal_pitch().get() as f64;
    println!("diode ladder: onset at resonance {onset:.4} (ideal 17), pitch {pitch:.2} Hz (ideal {ideal:.2})");
    assert!((onset / 17.0 - 1.0).abs() < 0.01, "onset {onset}");
    assert!((pitch / ideal - 1.0).abs() < 0.02, "pitch {pitch} Hz");
}

/// Stinchcombe's model on the deck's drive against ngspice's diode
/// ladder, resonance 10 of 17: at the ideal pitch `√2·I/(8π·Vt·C)`, and
/// tuned to the circuit's own pitch and onset.
#[test]
fn stinchcombe_matches_the_diode_ladder() {
    let ladder = DiodeLadder::DEFAULT;
    let mut ideal = Stinchcombe::new(hz(ladder.ideal_pitch().get() as f64 as f32), 10.0 / 17.0);
    ideal.prepare(4.0 * 48_000.0);
    structure_case(
        "diode_ladder",
        "stinchcombe ideal",
        STINCHCOMBE_IDEAL,
        |x| ideal.tick_f64(x),
    )
    .assert();
    let (onset, pitch) = onset_and_pitch(&ladder.netlist());
    let mut tuned = Stinchcombe::new(hz(pitch as f32), (10.0 / onset) as f32);
    tuned.prepare(4.0 * 48_000.0);
    structure_case(
        "diode_ladder",
        "stinchcombe tuned",
        STINCHCOMBE_TUNED,
        |x| tuned.tick_f64(x),
    )
    .assert();
    assert_eq!(ideal.stats().capped + tuned.stats().capped, 0);
}
const STINCHCOMBE_IDEAL: f64 = 8e-3;
const STINCHCOMBE_TUNED: f64 = 2e-4;
