//! `Circuit<Wdf>` on the very decks ngspice simulates (`spice/*.cir`):
//! each deck runs through the wave digital method — trees of series and
//! parallel adaptors under one R-type root, the devices a multi-port
//! nonlinear root — as a `stave_test::reference::Case` at the budget
//! `Circuit<Dk>` meets on the same deck (`dk_matches_ngspice.rs`,
//! `transistors_match_ngspice.rs`). Then WDF and DK are held to each
//! other sample for sample on every deck (`agreement`): both discretise
//! the same equations by the bilinear transform, so they differ only by
//! rounding and the Newton tolerance. Every solve's health is printed and
//! no solve may hit its cap, at the decks' rates and at 48 kHz driven hot.

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::methods::wdf::{NonlinearRoot, Wdf};
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Node};
use stave_dsp::circuit::Circuit;
use stave_test::reference::{Case, Metric, Reference};

fn deck_text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    std::fs::read_to_string(&path).unwrap()
}

fn deck(name: &str) -> Netlist {
    Netlist::parse_spice(&deck_text(name)).unwrap()
}

fn at(netlist: &Netlist, name: &str) -> Node {
    netlist.node(name).unwrap_or_else(|| panic!("no node {name}"))
}

/// The deck's own stimulus at `oversample` × 48 kHz through `Circuit<Wdf>`,
/// read at the reference's instants (sample 0 the operating point).
fn transient(name: &str, netlist: &Netlist, oversample: u32) -> (Vec<f64>, Health) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Wdf>::new(netlist).unwrap();
    circuit.prepare(rate as f32);
    println!("  {name}: {:?}", circuit.structure());
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

fn healthy(name: &str, health: &Health) {
    println!(
        "  WDF solve: {:.2} iterations/solve, {} of {} capped, {} continuations",
        health.newton.mean_iterations(),
        health.newton.capped,
        health.newton.solves,
        health.continuations
    );
    assert_eq!(health.newton.capped, 0, "{name} hit its iteration cap");
}

/// The deck (or `netlist`, built from it) against ngspice at `budget`.
fn holds(name: &str, netlist: &Netlist, oversample: u32, budget: f64, about_operating_point: bool) {
    let (run, health) = transient(name, netlist, oversample);
    let mut case = Case::ngspice(name)
        .named(format!("{name}/wdf {oversample}x"))
        .budget(Metric::NormalizedRms, budget);
    if about_operating_point {
        case = case.about_operating_point();
    }
    case.compare(&run).assert();
    healthy(name, &health);
}

/// Every transient deck `Circuit<Dk>` meets ngspice on, at DK's rate and
/// budget: `(deck, oversample, budget, about the operating point)`.
const TRANSIENTS: &[(&str, u32, f64, bool)] = &[
    ("rc_step", 16, 1e-3, false),
    ("coupler", 1, 2e-6, false),
    ("diode_clipper", 4, 5e-4, false),
    ("opamp_clipper", 16, 5e-5, false),
    ("common_cathode", 4, 1e-5, true),
    ("common_cathode_grid_current", 4, 2e-5, true),
    ("power_supply", 4, 2e-5, false),
    ("push_pull", 1, 2e-6, false),
    ("ce_stage_static", 4, 1e-5, false),
    ("fuzz_face_static", 4, 4e-5, false),
    ("rangemaster_static", 4, 1e-3, false),
    ("jfet_booster_static", 4, 1.5e-5, false),
    ("mosfet_booster_static", 4, 1e-6, false),
    ("tube_screamer", 16, 6e-5, false),
    ("ota_vca", 4, 1e-5, false),
];

#[test]
fn transient_decks_match_ngspice() {
    for &(name, oversample, budget, about) in TRANSIENTS {
        holds(name, &deck(name), oversample, budget, about);
    }
}

/// DK's budget (`Triode::static_transfer`'s): 5e-7 about the bias point.
#[test]
fn triode_stage_static_transfer_matches_ngspice() {
    let (run, health) = transient("triode_stage", &deck("triode_stage"), 1);
    let reference = Reference::ngspice("triode_stage");
    let bias = reference
        .abscissa()
        .iter()
        .position(|&t| -10.0 + 1_000.0 * t >= -1.5 - 1e-9)
        .unwrap();
    let plate = reference.values()[bias];
    Case::new("triode_stage/wdf", reference)
        .budget(Metric::NormalizedRms, 5e-7)
        .about(plate)
        .compare(&run)
        .assert();
    healthy("triode_stage", &health);
}

/// The Tube Screamer with stave's JRC4558 macromodel element in place of
/// the deck's behavioural op-amp: its gain node, leak and buffer absorbed
/// in the R-type root, its compensation capacitor a leaf port, its gain
/// stage a port of the nonlinear root, beside the clipping diodes.
#[test]
fn tube_screamer_with_a_jrc4558_element_matches_ngspice() {
    use stave_dsp::circuit::devices::opamp::{Amp, Macromodel};
    let mut netlist = deck("tube_screamer");
    for e in ["bg", "cc", "eo"] {
        netlist.remove(e).unwrap();
    }
    let kind = ElementKind::OpAmp {
        plus: at(&netlist, "in"),
        minus: at(&netlist, "fb"),
        output: at(&netlist, "out"),
        amp: Amp::Macromodel(Macromodel::JRC4558_TABLE),
        supply: (0.0, 9.0),
    };
    netlist.add("u1", kind).unwrap();
    holds("tube_screamer", &netlist, 16, 6e-5, false);
}

/// The TS808 clipper with the ideal op-amp element (a nullor absorbed in
/// the R-type root) against the deck's controlled-source form.
#[test]
fn opamp_clipper_with_an_ideal_op_amp_element_matches_ngspice() {
    use stave_dsp::circuit::devices::opamp::{Amp, Ideal};
    let mut netlist = Netlist::parse_spice(
        "TS808 clipper, an ideal op-amp element
         V1 in 0 DC 0 SIN(0 0.5 1k)
         R1 fb g 4.7k
         C1 g 0 47n
         RF out fb 51k
         CF out fb 51p
         D1 out fb DI
         D2 fb out DI
         .model DI D(Is=2.52n N=1.752)
         .control
         wrdata out v(out)
         .endc",
    )
    .unwrap();
    let kind = ElementKind::OpAmp {
        plus: at(&netlist, "in"),
        minus: at(&netlist, "fb"),
        output: at(&netlist, "out"),
        amp: Amp::Ideal(Ideal),
        supply: (-15.0, 15.0),
    };
    netlist.add("u1", kind).unwrap();
    holds("opamp_clipper", &netlist, 16, 5e-5, false);
}

/// A linear deck's response against ngspice's AC analysis: the running
/// filter's own state space, read by stepping it, prewarped — exact for
/// a linear circuit. Returns the circuit for further checks.
fn ac(name: &str, budget_db: f64) -> Circuit<Wdf> {
    let circuit = Circuit::<Wdf>::new(&deck(name)).unwrap();
    println!("  {name}: {:?}", circuit.structure());
    Case::ngspice(name)
        .named(format!("{name}/wdf"))
        .budget(Metric::MaxDb, budget_db)
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
    circuit
}

#[test]
fn rc_lowpass_matches_ngspice() {
    assert!(ac("rc_lowpass", 1e-10).structure().is_tree());
}

/// The Yeh–Smith tone stack as a true R-type WDF: no series–parallel
/// form, so the trees meet at an R-type root of several ports. Against
/// ngspice's AC analysis at three pot settings (DK's budget, 1e-10 dB)
/// and against Yeh & Smith's closed form (`PassiveToneStack`).
#[test]
fn tone_stack_matches_ngspice_and_yeh_smith() {
    use stave_dsp::{PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents};
    for (name, controls) in [("tone_stack_duncan_fender_mid", (0.5, 0.5, 0.5)), ("tone_stack_duncan_fender_bright", (0.9, 0.1, 0.7))] {
        let circuit = ac(name, 1e-10);
        let s = circuit.structure();
        assert!(!s.is_tree() && s.ports >= 4, "{name}: {s:?}");
        let mut stack = PassiveToneStack::new(PassiveToneStackComponents {
            treble_capacitance_farads: 250e-12,
            bass_capacitance_farads: 100e-9,
            midrange_capacitance_farads: 47e-9,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: 1e6,
            midrange_potentiometer_ohms: 25e3,
            slope_resistance_ohms: 100e3,
        });
        stack.set_controls(PassiveToneControlPositions::new(controls.0, controls.1, controls.2));
        let worst = (0..60)
            .map(|k| {
                let hz = 20.0 * 10f64.powf(f64::from(k) / 20.0);
                let ours = circuit.analog_response(hz).unwrap().magnitude();
                (20.0 * (ours / stack.circuit_magnitude_at(hz)).log10()).abs()
            })
            .fold(0.0, f64::max);
        println!("  {name}: Yeh–Smith closed form vs WDF, largest difference {worst:.3e} dB");
        assert!(worst < 1e-10, "{name}: {worst}");
    }
    assert!(!ac("tone_stack_duncan_fender_dark", 1e-10).structure().is_tree());
}

/// A bridged-T notch with a load (a bridge: no series–parallel form)
/// against ngspice's AC analysis, down through the notch.
#[test]
fn bridged_t_matches_ngspice() {
    let circuit = ac("bridged_t", 1e-10);
    assert!(!circuit.structure().is_tree());
}

/// The Scattering Iterative Method in place of the Newton root, on the
/// decks whose devices are all diodes: each diode its own wave port and
/// scalar solve, swept through the R-type root to convergence. The same
/// budgets; the health counts sweeps per sample.
#[test]
fn scattering_iterative_method_matches_ngspice() {
    for (name, oversample, budget) in [("diode_clipper", 4, 5e-4), ("opamp_clipper", 16, 5e-5)] {
        let reference = Reference::ngspice(name);
        let rate = 48_000.0 * f64::from(oversample);
        let mut circuit = Circuit::<Wdf>::new(&deck(name)).unwrap();
        circuit.set_nonlinear_root(NonlinearRoot::Scattering).unwrap();
        circuit.prepare(rate as f32);
        let (mut n, mut out) = (0u64, circuit.output());
        let run: Vec<f64> = reference
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
        Case::ngspice(name)
            .named(format!("{name}/wdf {oversample}x, scattering iterative method"))
            .budget(Metric::NormalizedRms, budget)
            .compare(&run)
            .assert();
        healthy(name, &circuit.health());
    }
    // A transistor is no one-port: refused.
    let mut fuzz = Circuit::<Wdf>::new(&deck("fuzz_face_static")).unwrap();
    assert!(fuzz.set_nonlinear_root(NonlinearRoot::Scattering).is_err());
}

mod agreement {
    //! `Circuit<Wdf>` against `Circuit<Dk>` on every deck above, sample
    //! for sample at the deck's rate: the same bilinear discretisation,
    //! so the two differ by rounding and the Newton tolerance (1e-9 V).

    use super::*;

    fn largest(name: &str, netlist: &Netlist, oversample: u32) -> f64 {
        let rate = 48_000.0 * f64::from(oversample);
        let frames = (Reference::ngspice(name).abscissa().last().unwrap() * rate) as usize;
        let mut wdf = Circuit::<Wdf>::new(netlist).unwrap();
        let mut dk = Circuit::<Dk>::new(netlist).unwrap();
        wdf.prepare(rate as f32);
        dk.prepare(rate as f32);
        let (mut worst, mut peak) = ((wdf.output() - dk.output()).abs(), dk.output().abs());
        for _ in 0..frames {
            let (a, b) = (wdf.step(0.0), dk.step(0.0));
            worst = worst.max((a - b).abs());
            peak = peak.max(b.abs());
        }
        healthy(name, &wdf.health());
        worst / peak.max(1e-30)
    }

    #[test]
    fn wdf_and_dk_agree_on_every_transient_deck() {
        for &(name, oversample, _, _) in TRANSIENTS {
            // WDF solves devices statically (DSP.md R11): DK on the same.
            let worst = largest(name, &deck(name).without_charge(), oversample);
            println!("{name}: WDF vs DK, largest difference {worst:.3e} of peak");
            assert!(worst < 1e-8, "{name}: {worst:e}");
        }
    }

    #[test]
    fn wdf_and_dk_agree_on_the_linear_responses() {
        for name in ["rc_lowpass", "tone_stack_duncan_fender_mid", "tone_stack_duncan_fender_bright", "tone_stack_duncan_fender_dark", "bridged_t"] {
            let wdf = Circuit::<Wdf>::new(&deck(name)).unwrap();
            let dk = Circuit::<Dk>::new(&deck(name)).unwrap();
            let worst = (0..60)
                .map(|k| {
                    let hz = 20.0 * 10f64.powf(f64::from(k) / 20.0);
                    let (a, b) = (wdf.analog_response(hz).unwrap(), dk.analog_response(hz).unwrap());
                    (a.re - b.re).hypot(a.im - b.im) / b.magnitude()
                })
                .fold(0.0, f64::max);
            println!("{name}: WDF vs DK response, largest relative difference {worst:.3e}");
            assert!(worst < 1e-11, "{name}: {worst:e}");
        }
    }
}

mod health {
    //! Solver health at the shipping rate: 48 kHz, no oversampling, a hot
    //! signal (two tones, 110 Hz and 2.75 kHz) far past each circuit's
    //! clipping point. No solve may hit its cap.

    use super::*;

    fn hot(n: usize, volts: f64) -> f64 {
        let t = n as f64 / 48e3;
        volts * ((std::f64::consts::TAU * 110.0 * t).sin() + 0.5 * (std::f64::consts::TAU * 2_750.0 * t).sin())
    }

    fn run(name: &str, netlist: &Netlist, source: &str, offset: f64, volts: f64, root: NonlinearRoot) {
        let mut c = Circuit::<Wdf>::new(netlist).unwrap();
        c.set_nonlinear_root(root).unwrap();
        c.drive(source).unwrap();
        c.prepare(48_000.0);
        let mut peak = 0.0f64;
        for n in 0..48_000 {
            let y = c.step(offset + hot(n, volts));
            assert!(y.is_finite(), "{name}: not finite at {n}");
            peak = peak.max(y.abs());
        }
        println!("{name} at 48 kHz, {volts} V hot: peak {peak:.3} V");
        healthy(name, &c.health());
    }

    #[test]
    fn diode_clipper_at_48k_hot() {
        let netlist = Netlist::parse_spice(&deck_text("diode_clipper").replace("V1 in 0 SIN(0 1 1k)", "V1 in 0 0")).unwrap();
        run("diode_clipper", &netlist, "v1", 0.0, 10.0, NonlinearRoot::Newton);
        run("diode_clipper (scattering)", &netlist, "v1", 0.0, 10.0, NonlinearRoot::Scattering);
    }

    #[test]
    fn tube_screamer_at_48k_hot() {
        let netlist = Netlist::parse_spice(&deck_text("tube_screamer").replace("V1 in 0 SIN(4.5 0.3 1k)", "V1 in 0 4.5")).unwrap();
        run("tube_screamer", &netlist, "v1", 4.5, 2.0, NonlinearRoot::Newton);
    }

    #[test]
    fn fuzz_face_at_48k_hot() {
        let text = deck_text("fuzz_face_static");
        let line = text.lines().find(|l| l.to_ascii_lowercase().starts_with("vin")).expect("fuzz_face: no VIN");
        let netlist = Netlist::parse_spice(&text.replace(line, "VIN in 0 0")).unwrap();
        run("fuzz_face", &netlist, "vin", 0.0, 1.0, NonlinearRoot::Newton);
    }

    #[test]
    fn common_cathode_at_48k_hot() {
        let netlist = Netlist::parse_spice(&deck_text("common_cathode").replace("V1 g 0 SIN(0 0.5 1k)", "V1 g 0 0")).unwrap();
        run("common_cathode", &netlist, "v1", 0.0, 5.0, NonlinearRoot::Newton);
    }
}
