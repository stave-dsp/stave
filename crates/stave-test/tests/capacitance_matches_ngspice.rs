//! Device capacitances (DSP.md R11) against ngspice on the full cards:
//! each junction's `C(v)` itself, every family's small-signal response
//! (magnitude and phase), and transients where the charge decides the
//! waveform — a common-emitter stage's Miller roll-off, a 12AX7's Miller
//! input, a diode clipper's recovery at 10 kHz. Every transient case also
//! runs with the netlist's charge taken out, to show the reference is one
//! the capacitances move by many budgets. Every solve's health is printed
//! and no solve may hit its cap.

use std::path::PathBuf;

use stave_dsp::circuit::devices::bjt::Bjt;
use stave_dsp::circuit::devices::capacitance::Depletion;
use stave_dsp::circuit::devices::jfet::Jfet;
use stave_dsp::circuit::devices::{pentode, triode};
use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Node};
use stave_dsp::circuit::Circuit;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn element<'n>(netlist: &'n Netlist, name: &str) -> &'n ElementKind {
    &netlist.element(name).unwrap_or_else(|| panic!("no {name}")).kind
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

/// `netlist` with every device's stored charge taken out: the static
/// circuit the decks were before R11.
fn without_charge(netlist: &Netlist) -> Netlist {
    let mut out = netlist.clone();
    let names: Vec<String> = netlist.elements().iter().map(|e| e.name.clone()).collect();
    for name in names {
        let mut kind = out.element(&name).unwrap().kind.clone();
        match &mut kind {
            ElementKind::Diode { diode, .. } => {
                diode.capacitance.zero_bias_farads = 0.0;
                diode.transit_seconds = 0.0;
            }
            ElementKind::Bjt { bjt, .. } => {
                for d in [
                    &mut bjt.emitter_capacitance,
                    &mut bjt.collector_capacitance,
                    &mut bjt.substrate_capacitance,
                ] {
                    d.zero_bias_farads = 0.0;
                }
                bjt.forward_transit.seconds = 0.0;
                bjt.reverse_transit_seconds = 0.0;
            }
            ElementKind::Jfet { jfet, .. } => {
                jfet.gate_source_capacitance.zero_bias_farads = 0.0;
                jfet.gate_drain_capacitance.zero_bias_farads = 0.0;
            }
            ElementKind::Mosfet { mosfet, .. } => {
                mosfet.gate_source_overlap_farads = 0.0;
                mosfet.gate_drain_overlap_farads = 0.0;
                mosfet.gate_bulk_overlap_farads = 0.0;
                mosfet.bulk_drain_capacitance.zero_bias_farads = 0.0;
                mosfet.bulk_source_capacitance.zero_bias_farads = 0.0;
            }
            ElementKind::Triode { capacitances, .. } => *capacitances = triode::Interelectrode::NONE,
            ElementKind::Pentode { capacitances, .. } => *capacitances = pentode::Interelectrode::NONE,
            ElementKind::Capacitor { .. } if name.starts_with("cg") || name.starts_with("cp") => {
                out.remove(&name);
                continue;
            }
            _ => continue,
        }
        out.remove(&name);
        out.add(&name, kind).unwrap();
    }
    out
}

/// The deck's own stimulus at `oversample` × 48 kHz, read at the
/// reference's instants.
fn transient(name: &str, netlist: &Netlist, oversample: u32) -> (Vec<f64>, Health) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Dk>::new(netlist).unwrap();
    circuit.prepare(rate as f32);
    let (mut n, mut out) = (0u64, circuit.output());
    let run = reference
        .abscissa()
        .iter()
        .map(|&t| {
            while n < (t * rate).round() as u64 {
                out = circuit.step(0.0);
                n += 1;
            }
            out
        })
        .collect();
    (run, circuit.health())
}

fn case(name: &str, netlist: &Netlist, oversample: u32, budget: f64, label: &str) -> Outcome {
    let (run, health) = transient(name, netlist, oversample);
    let outcome = Case::ngspice(name)
        .named(format!("{name}/dk {oversample}x{label}"))
        .budget(Metric::NormalizedRms, budget)
        .compare(&run);
    healthy(name, &health);
    outcome
}

/// The deck at `oversample`× and twice that: the first run meets
/// `budget` and the error falls as h²; then the same netlist without its
/// charge, which must miss the reference by `margin` budgets or more.
fn converging_with_charge(name: &str, netlist: &Netlist, oversample: u32, budget: f64, margin: f64) {
    let coarse = case(name, netlist, oversample, budget, "");
    let fine = case(name, netlist, 2 * oversample, budget, "");
    assert_converges(&coarse, &fine, 3.0);
    let coarse = coarse.assert();
    charge_matters(name, netlist, oversample, budget, margin, coarse);
}

/// The netlist with its charge taken out, against the same reference:
/// its error must exceed `margin` budgets.
fn charge_matters(name: &str, netlist: &Netlist, oversample: u32, budget: f64, margin: f64, with: f64) {
    let (run, _) = transient(name, &without_charge(netlist), oversample);
    let without = Case::ngspice(name)
        .named(format!("{name}/dk {oversample}x, no charge"))
        .budget(Metric::NormalizedRms, f64::INFINITY)
        .compare(&run)
        .assert();
    println!("  {name}: normalized RMS {with:.3e} with the charge, {without:.3e} without");
    assert!(without > margin * budget, "{name}: the charge moves the waveform only {without:e}");
}

// ------------------------------------------------------------------ C(v)

/// The ramp the `junction_cv_*` decks put across the junction: `(v(t),
/// dv/dt)`, 10 µs from `start` to `end`; at t = 0 the operating point,
/// where nothing moves yet.
fn ramp(t: f64, start: f64, end: f64) -> (f64, f64) {
    let slope = (end - start) / 10e-6;
    (start + slope * t, if t > 0.0 { slope } else { 0.0 })
}

/// `C(v)` as each deck measures it — the current `C(v)·dv/dt + I(v)` of a
/// fast ramp, at 1 V per µA — from the device law alone, pointwise.
/// ngspice integrates the same charge with its own steps, so what is left
/// is its integration error.
#[test]
fn junction_capacitance_matches_ngspice() {
    let scale = 1e6;
    // ngspice's gmin across every junction (a convergence aid, 1 pS, not
    // in the device models): 10 pA at −10 V, beside microamps of charging.
    const GMIN: f64 = 1e-12;
    // 1N4148: depletion, then the transit time's diffusion charge.
    let n = deck("junction_cv_1n4148");
    let ElementKind::Diode { diode, .. } = element(&n, "d1") else { panic!() };
    let junction = diode.junction();
    Case::ngspice("junction_cv_1n4148")
        .budget(Metric::NormalizedRms, 1e-5)
        .pointwise(|t| {
            let (v, k) = ramp(t, -10.0, 0.3);
            let (i, g) = junction.current_and_conductance(v);
            let (_, c) = junction.charge(v, i, g);
            scale * (i + GMIN * v + c * k)
        })
        .assert();

    // 2N3904's base–collector junction: vbe = 0, vbc = −v(collector).
    let n = deck("junction_cv_2n3904");
    let ElementKind::Bjt { bjt, .. } = element(&n, "q1") else { panic!() };
    Case::ngspice("junction_cv_2n3904")
        .budget(Metric::NormalizedRms, 1e-5)
        .pointwise(|t| {
            let (vc, k) = ramp(t, 10.0, -0.3);
            let c = bjt.currents(0.0, -vc);
            let q = bjt.charges(0.0, -vc);
            // Into the collector: its current, and the junction's charge
            // leaving as vbc rises.
            scale * (c.collector + GMIN * vc + q.base_collector_per_vbc * k)
        })
        .assert();

    // J201: both gate junctions, the drain and source at ground.
    let n = deck("junction_cv_j201");
    let ElementKind::Jfet { jfet, .. } = element(&n, "j1") else { panic!() };
    Case::ngspice("junction_cv_j201")
        .budget(Metric::NormalizedRms, 1e-5)
        .pointwise(|t| {
            let (v, k) = ramp(t, -10.0, 0.3);
            let i = jfet.currents(v, v).gate;
            let [(_, cgs), (_, cgd)] = jfet.gate_charges(v, v);
            scale * (i + 2.0 * GMIN * v + (cgs + cgd) * k)
        })
        .assert();

    // VN10KM: both bulk junctions and the gate–bulk overlap.
    let n = deck("junction_cv_vn10km");
    let ElementKind::Mosfet { mosfet, .. } = element(&n, "m1") else { panic!() };
    Case::ngspice("junction_cv_vn10km")
        .budget(Metric::NormalizedRms, 1e-5)
        .pointwise(|t| {
            let (v, k) = ramp(t, -10.0, 0.3);
            let [(_, cbd), (_, cbs)] = mosfet.bulk_charges(v, v);
            scale * (2.0 * GMIN * v + (cbd + cbs + mosfet.gate_bulk_overlap_farads) * k)
        })
        .assert();

    // The law the decks exercise, at a point by hand: 4 pF at zero bias,
    // 4 pF · 11^−0.4 at −10 V.
    let law = Depletion {
        zero_bias_farads: 4e-12,
        grading: 0.4,
        ..Depletion::NONE
    };
    assert!((law.charge(-10.0).1 - 4e-12 * 11f64.powf(-0.4)).abs() < 1e-24);
}

// -------------------------------------------------------- small signal

/// `circuit`'s analog response against the deck's `.ac`: magnitude (dB)
/// and phase (degrees, each model value taken on the branch nearest the
/// reference's).
fn response(name: &str, circuit: &Circuit<Dk>, label: &str, db: f64, degrees: f64) {
    Case::ngspice(name)
        .named(format!("{name}/dk{label} magnitude"))
        .budget(Metric::MaxDb, db)
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
    let phase = Reference::ngspice(name).column(2);
    let table: Vec<(f64, f64)> = phase.abscissa().iter().copied().zip(phase.values().iter().copied()).collect();
    Case::new(name, phase)
        .named(format!("{name}/dk{label} phase"))
        .map_reference("degrees", f64::to_degrees)
        .budget(Metric::MaxAbs, degrees)
        .pointwise(|hz| {
            let reference = table.iter().find(|(f, _)| *f == hz).map_or(0.0, |p| p.1).to_degrees();
            let model = circuit.analog_response(hz).unwrap().phase().to_degrees();
            model - 360.0 * ((model - reference) / 360.0).round()
        })
        .assert();
}

/// Diode, BJT, JFET, MOSFET: each deck's small-signal response, its
/// junction capacitances linearised at the operating point.
#[test]
fn small_signal_responses_match_ngspice() {
    // The J201 deck's mid-band gain sits 1.5e-6 (relative) from ngspice's
    // at every frequency — its operating point, not its capacitances —
    // hence its looser magnitude budget.
    for (name, db, degrees) in [
        ("diode_ac", 1e-5, 1e-4),
        ("ce_stage_ac", 1e-5, 1e-4),
        ("jfet_booster_ac", 3e-5, 3e-4),
        ("mosfet_booster_ac", 1e-5, 1e-4),
    ] {
        let circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
        response(name, &circuit, "", db, degrees);
    }
}

/// The 12AX7 stage: the deck's B-source law with its capacitors, then
/// stave's triode element carrying `Interelectrode::T12AX7` in their
/// place.
#[test]
fn triode_small_signal_response_matches_ngspice() {
    let name = "common_cathode_ac";
    response(name, &Circuit::<Dk>::new(&deck(name)).unwrap(), "", 1e-5, 1e-4);
    let devices = with_triode(name);
    response(name, &Circuit::<Dk>::new(&devices).unwrap(), ", stave triode", 1e-5, 1e-4);
}

/// The EL34 stage: the deck, then stave's pentode element with
/// `Interelectrode::EL34`.
#[test]
fn pentode_small_signal_response_matches_ngspice() {
    use stave_dsp::circuit::devices::pentode::koren::Koren;
    let name = "pentode_stage_ac";
    response(name, &Circuit::<Dk>::new(&deck(name)).unwrap(), "", 1e-5, 1e-4);
    let mut n = deck(name);
    for e in ["bp", "bs", "cgk", "cgp", "cpk"] {
        n.remove(e).unwrap();
    }
    let kind = ElementKind::Pentode {
        plate: n.node("p").unwrap(),
        screen: n.node("s").unwrap(),
        grid: n.node("g").unwrap(),
        cathode: Node::GROUND,
        tube: Koren::EL34,
        capacitances: pentode::Interelectrode::EL34,
    };
    n.add("x1", kind).unwrap();
    response(name, &Circuit::<Dk>::new(&n).unwrap(), ", stave pentode", 1e-5, 1e-4);
}

/// A common-cathode deck with stave's 12AX7 (Koren, with its datasheet
/// capacitances) in place of the B sources and capacitors.
fn with_triode(name: &str) -> Netlist {
    use stave_dsp::circuit::devices::triode::{koren::Koren, Tube};
    let mut n = deck(name);
    for e in ["be", "re", "bi", "cgk", "cgp", "cpk"] {
        n.remove(e).unwrap();
    }
    let kind = ElementKind::Triode {
        plate: n.node("p").unwrap(),
        grid: n.node("g").unwrap(),
        cathode: n.node("k").unwrap(),
        tube: Tube::Koren(Koren::T12AX7),
        capacitances: triode::Interelectrode::T12AX7,
    };
    n.add("t1", kind).unwrap();
    n
}

// ------------------------------------------------------------ transients

/// The CE stage at gain ~200 from 47k, 50 mV at 20 kHz: the Miller pole
/// near 90 kHz takes about a decibel and ten degrees off the output.
#[test]
fn ce_stage_miller_roll_off_matches_ngspice() {
    converging_with_charge("ce_stage_miller", &deck("ce_stage_miller"), 16, 5e-4, 10.0);
}

/// The 12AX7 stage at 10 kHz through 68k into its Miller capacitance:
/// the deck, then stave's triode with its datasheet capacitances.
#[test]
fn triode_miller_input_matches_ngspice() {
    let name = "common_cathode_miller";
    let netlist = deck(name);
    converging_with_charge(name, &netlist, 8, 2e-4, 10.0);
    let devices = with_triode(name);
    let outcome = case(name, &devices, 8, 2e-4, ", stave triode").assert();
    charge_matters(name, &devices, 8, 2e-4, 10.0, outcome);
}

/// The 1N4148 clipper at 10 kHz with no shunt capacitor: the junctions'
/// recovery on every edge.
#[test]
fn diode_clipper_at_10_khz_matches_ngspice() {
    converging_with_charge("diode_clipper_hf", &deck("diode_clipper_hf"), 16, 1e-3, 3.0);
}
