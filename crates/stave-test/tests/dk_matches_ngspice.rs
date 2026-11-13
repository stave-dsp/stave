//! `Circuit<Dk>` on the very decks ngspice simulates (`spice/*.cir`):
//! each deck — its own stimulus included — runs through the nodal DK
//! method, as a `stave_test::reference::Case` at a budget no looser than
//! the hand-derived topology's on the same deck (`matches_ngspice.rs`).
//! Then the hand-derived topologies and `Circuit<Dk>` are held to each
//! other on identical input (`mod agreement`), so a topology that drifts
//! from its schematic shows even where ngspice is not run. Every
//! solve's health is printed and no solve may hit its cap.

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::Circuit;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

fn deck_text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    std::fs::read_to_string(&path).unwrap()
}

fn deck(name: &str) -> Netlist {
    Netlist::parse_spice(&deck_text(name)).unwrap()
}

/// The deck's own stimulus at `oversample` × 48 kHz, read at the
/// reference's instants (sample n is time n/rate; sample 0 is the
/// operating point, or the initial conditions under `UIC`).
fn transient(name: &str, oversample: u32) -> (Vec<f64>, Health) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
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

fn transient_case(name: &str, oversample: u32, case: Case) -> Outcome {
    let (run, health) = transient(name, oversample);
    let outcome = case.named(format!("{name}/dk {oversample}x")).compare(&run);
    healthy(name, &health);
    outcome
}

/// No hand topology: the oracle's own RC step, through DK at 16×.
#[test]
fn rc_step_matches_ngspice() {
    let case = Case::ngspice("rc_step").budget(Metric::NormalizedRms, 1e-3);
    transient_case("rc_step", 16, case).assert();
}

#[test]
fn rc_lowpass_matches_ngspice() {
    let circuit = Circuit::<Dk>::new(&deck("rc_lowpass")).unwrap();
    Case::ngspice("rc_lowpass")
        .named("rc_lowpass/dk")
        .budget(Metric::MaxDb, 1e-10)
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
}

/// Hand budget (`Coupler`): 2e-6 at 48 kHz.
#[test]
fn coupler_matches_ngspice() {
    let case = Case::ngspice("coupler").budget(Metric::NormalizedRms, 2e-6);
    transient_case("coupler", 1, case).assert();
}

/// Hand budget (WDF clipper): 5e-4 at 4×. DK keeps the card's 0.568 Ω
/// series resistance the WDF leaves out; the junction capacitance and
/// transit time stay unmodelled (`Netlist::unmodelled`).
#[test]
fn diode_clipper_matches_ngspice() {
    let case = Case::ngspice("diode_clipper").budget(Metric::NormalizedRms, 5e-4);
    transient_case("diode_clipper", 4, case).assert();
}

/// Hand budget: 5e-5 at 16×, and the error falls as h² (three times or
/// more at 32×).
#[test]
fn opamp_clipper_matches_ngspice() {
    let case = || Case::ngspice("opamp_clipper").budget(Metric::NormalizedRms, 5e-5);
    let coarse = transient_case("opamp_clipper", 16, case());
    let fine = transient_case("opamp_clipper", 32, case());
    assert_converges(&coarse, &fine, 3.0);
    coarse.assert();
}

/// Hand budget (`CommonCathode`): 1e-5 at 4× about the operating point,
/// which must agree to 1 µV. The Koren law is the deck's B-source
/// expression, solved as written.
#[test]
fn common_cathode_matches_ngspice() {
    let (run, health) = transient("common_cathode", 4);
    let case = Case::ngspice("common_cathode")
        .named("common_cathode/dk 4x")
        .budget(Metric::NormalizedRms, 1e-5)
        .about_operating_point();
    let operating_point = case.reference().values()[0];
    assert!((run[0] - operating_point).abs() < 1e-6, "{} vs {operating_point}", run[0]);
    case.compare(&run).assert();
    healthy("common_cathode", &health);
}

/// Hand budget: 2e-5 at 4× (Dempwolf grid current through 33 kΩ).
#[test]
fn common_cathode_grid_current_matches_ngspice() {
    let (run, health) = transient("common_cathode_grid_current", 4);
    let case = Case::ngspice("common_cathode_grid_current")
        .named("common_cathode_grid_current/dk 4x")
        .budget(Metric::NormalizedRms, 2e-5)
        .about_operating_point();
    let operating_point = case.reference().values()[0];
    assert!((run[0] - operating_point).abs() < 1e-6, "{} vs {operating_point}", run[0]);
    case.compare(&run).assert();
    healthy("common_cathode_grid_current", &health);
}

/// Hand budget (`PowerSupply`): 2e-5 at 4×, from switch-on (`UIC`).
#[test]
fn power_supply_matches_ngspice() {
    let case = Case::ngspice("power_supply").budget(Metric::NormalizedRms, 2e-5);
    transient_case("power_supply", 4, case).assert();
}

/// Hand budget (`PassiveToneStack`'s closed form): 1e-10 dB against
/// ngspice's AC analysis, at three pot settings. DK's response is the
/// analog one by the bilinear prewarp, exact for a linear circuit.
#[test]
fn tone_stack_matches_ngspice() {
    for name in ["tone_stack_mid", "tone_stack_bright", "tone_stack_dark"] {
        let circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
        Case::ngspice(name)
            .named(format!("{name}/dk"))
            .budget(Metric::MaxDb, 1e-10)
            .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
            .assert();
    }
}

/// Hand budget (`Triode::static_transfer`): 5e-7 about the bias point —
/// after the hand stage's fitted level scale; DK reads absolute plate
/// volts and needs no fit.
#[test]
fn triode_stage_static_transfer_matches_ngspice() {
    let (run, health) = transient("triode_stage", 1);
    let reference = Reference::ngspice("triode_stage");
    // The grid ramps −10 V → +10 V over 20 ms; the 12AX7's bias is −1.5 V.
    let bias = reference
        .abscissa()
        .iter()
        .position(|&t| -10.0 + 1_000.0 * t >= -1.5 - 1e-9)
        .unwrap();
    let plate = reference.values()[bias];
    Case::new("triode_stage/dk", reference)
        .budget(Metric::NormalizedRms, 5e-7)
        .about(plate)
        .compare(&run)
        .assert();
    healthy("triode_stage", &health);
}

/// Hand budget (`PushPull::static_transfer`): 2e-6, again after a fit
/// the DK comparison does without.
#[test]
fn push_pull_static_transfer_matches_ngspice() {
    let case = Case::ngspice("push_pull").budget(Metric::NormalizedRms, 2e-6);
    transient_case("push_pull", 1, case).assert();
}

/// E6: every diode part's I–V curve — a ramp through 1 kΩ, forward
/// conduction and, where the card has `BV`, reverse breakdown — through
/// `Circuit<Dk>` against ngspice on the same card; and each part constant
/// is its card as the parser reads it. Budgets about three times the
/// measured error (6.5e-8 1N4148, 6.4e-9 1N34A, 1.8–2.0e-7 the LEDs,
/// 1.1e-8 BZX84C6V2L, 2.4e-8 1N750; ngspice 47 fixtures).
#[test]
fn diode_iv_curves_match_ngspice() {
    use stave_dsp::circuit::netlist::ElementKind;
    use stave_dsp::Shockley;
    for (name, part, budget) in [
        ("diode_iv_1n4148", Shockley::N4148, 2e-7),
        ("diode_iv_1n34a", Shockley::N34A, 2e-8),
        ("diode_iv_led_red", Shockley::LED_RED, 5e-7),
        ("diode_iv_led_green", Shockley::LED_GREEN, 5e-7),
        ("diode_iv_led_blue", Shockley::LED_BLUE, 5e-7),
        ("diode_iv_bzx84c6v2l", Shockley::BZX84C6V2L, 5e-8),
        ("diode_iv_1n750", Shockley::N750, 1e-7),
    ] {
        let netlist = deck(name);
        let ElementKind::Diode { diode, .. } = netlist.element("d1").unwrap().kind else {
            panic!("{name}: no diode D1");
        };
        // To the last bit or two: `10U` reads as 10 × 1e-6.
        let same = |a: f64, b: f64| (a - b).abs() <= 1e-15 * a.abs().max(b.abs());
        let card = |d: Shockley| {
            let b = d.breakdown.map_or([0.0; 3], |b| [b.volts, b.amps, b.emission]);
            [d.saturation_amps, d.emission, d.thermal_volts, d.series_ohms, b[0], b[1], b[2]]
        };
        let (ours, theirs) = (card(part), card(diode));
        assert!(
            ours.iter().zip(&theirs).all(|(a, b)| same(*a, *b)),
            "{name}: the part constant is not the deck's card: {part:?} vs {diode:?}"
        );
        assert!(netlist.unmodelled.is_empty(), "{name}: {:?}", netlist.unmodelled);
        let case = Case::ngspice(name).budget(Metric::NormalizedRms, budget);
        transient_case(name, 1, case).assert();
    }
}

mod agreement {
    //! The hand-derived topologies against `Circuit<Dk>` on the same
    //! netlist and the same hot input, at the rate each ships at. Both
    //! discretise the same equations by the trapezoidal rule, so they
    //! agree to the Newton tolerance and the hand stage's `f32` output.

    use super::*;
    use stave_dsp::{
        CommonCathode, CommonCathodeConfig, Coupler, DiodeClipper, DiodeClipperConfig, Koren,
        NewtonSettings, OpAmpClipper, OpAmpClipperConfig, PassiveToneControlPositions,
        PassiveToneStack, PassiveToneStackComponents, PowerSupply, PowerSupplyConfig, Shockley,
        Stage,
    };

    fn driven(text: &str, source: &str, rate: f64) -> Circuit<Dk> {
        let mut c = Circuit::<Dk>::new(&Netlist::parse_spice(text).unwrap()).unwrap();
        c.drive(source).unwrap();
        c.prepare(rate as f32);
        c
    }

    fn hot(n: usize, rate: f64, volts: f64) -> f64 {
        let t = n as f64 / rate;
        volts
            * ((std::f64::consts::TAU * 110.0 * t).sin()
                + 0.5 * (std::f64::consts::TAU * 2_750.0 * t).sin())
    }

    /// Run both on `frames` hot samples; the largest difference.
    fn largest(
        frames: usize,
        mut dk: impl FnMut(usize) -> f64,
        mut hand: impl FnMut(usize) -> f64,
    ) -> f64 {
        (0..frames)
            .map(|n| (dk(n) - hand(n)).abs())
            .fold(0.0, f64::max)
    }

    fn agree(name: &str, worst: f64, budget: f64, dk: &Circuit<Dk>) {
        println!("{name}: hand vs DK, largest difference {worst:.3e} (budget {budget:e})");
        assert!(worst <= budget, "{name}: {worst:e} > {budget:e}");
        healthy(name, &dk.health());
    }

    #[test]
    fn coupler() {
        let mut dk = driven(
            "coupler\nV1 i 0 0\nC1 i o 22n\nR1 o 0 1meg\n.control\nwrdata f v(o)\n.endc",
            "v1",
            48e3,
        );
        let mut hand = Coupler::new((1.0 / (std::f64::consts::TAU * 22e-9 * 1e6)) as f32);
        hand.prepare(48_000.0);
        let x = |n| hot(n, 48e3, 1.0) as f32;
        let w = largest(
            48_000,
            |n| f64::from(dk.tick(x(n))),
            |n| f64::from(hand.tick(x(n))),
        );
        agree("coupler", w, 1e-6, &dk);
    }

    #[test]
    fn diode_clipper() {
        let rate = 48e3;
        let mut dk = driven(
            "clipper\nV1 in 0 0\nR1 in out 2.2k\nC1 out 0 10n\nD1 out 0 DI\nD2 0 out DI\n\
             .model DI D(Is=2.52n N=1.752)\n.control\nwrdata f v(out)\n.endc",
            "v1",
            rate,
        );
        let mut hand = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::N4148,
            solver: NewtonSettings::DEFAULT,
        });
        hand.prepare(rate as f32);
        let x = |n| hot(n, rate, 4.0) as f32;
        let w = largest(
            48_000,
            |n| f64::from(dk.tick(x(n))),
            |n| f64::from(hand.tick(x(n))),
        );
        agree("diode clipper (WDF)", w, 1e-6, &dk);
    }

    #[test]
    fn opamp_clipper() {
        let rate = 48e3;
        let mut dk = driven(
            "clipper\nV1 in 0 0\nE1 inm 0 in 0 1\nR1 inm s 4.7k\nVS s g 0\nC1 g 0 47n\n\
             E2 fb 0 in 0 1\nF1 0 out VS 1\nRF out fb 551k\nCF out fb 51p\nD1 out fb DI\n\
             D2 fb out DI\n.model DI D(Is=2.52n N=1.752)\n.control\nwrdata f v(out)\n.endc",
            "v1",
            rate,
        );
        let mut hand = OpAmpClipper::new(OpAmpClipperConfig {
            feedback_ohms: 551e3,
            ..OpAmpClipperConfig::TS808
        });
        hand.prepare(rate as f32);
        let w = largest(
            48_000,
            |n| dk.tick_f64(hot(n, rate, 1.0)),
            |n| hand.tick_f64(hot(n, rate, 1.0)),
        );
        agree("op-amp clipper", w, 1e-9, &dk);
    }

    #[test]
    fn common_cathode() {
        let rate = 192e3;
        let text = deck_text("common_cathode").replace("V1 g 0 SIN(0 0.5 1k)", "V1 g 0 0");
        let mut dk = driven(&text, "v1", rate);
        let mut hand = CommonCathode::new(CommonCathodeConfig {
            tube: Koren::T12AX7,
            source_ohms: 0.0,
            supply_volts: 300.0,
            plate_load_ohms: 100e3,
            cathode_ohms: 1.5e3,
            bypass_farads: 22e-6,
            newton: Default::default(),
        });
        hand.prepare(rate as f32);
        assert!((dk.output() - hand.plate_volts()).abs() < 1e-6);
        let x = |n| hot(n, rate, 2.0) as f32;
        // The hand stage returns f32 volts near 200 V: its rounding is ~1e-5 V.
        let w = largest(
            19_200,
            |n| f64::from(dk.tick(x(n))),
            |n| f64::from(hand.tick(x(n))),
        );
        agree("common cathode", w, 1e-4, &dk);
    }

    #[test]
    fn power_supply() {
        let rate = 48e3;
        let mut dk = Circuit::<Dk>::new(&deck("power_supply")).unwrap();
        dk.drive("il").unwrap();
        dk.prepare(rate as f32);
        let mut hand = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
        hand.prepare(rate as f32);
        let load = |n: usize| 0.05 + 0.04 * (std::f64::consts::TAU * 7.0 * n as f64 / rate).sin();
        let w = largest(24_000, |n| dk.tick_f64(load(n)), |n| hand.tick_f64(load(n)));
        agree("power supply", w, 1e-8, &dk);
    }

    #[test]
    fn tone_stack() {
        let mut stack = PassiveToneStack::new(PassiveToneStackComponents {
            treble_capacitance_farads: 250e-12,
            bass_capacitance_farads: 100e-9,
            midrange_capacitance_farads: 47e-9,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: 1e6,
            midrange_potentiometer_ohms: 25e3,
            slope_resistance_ohms: 100e3,
        });
        stack.set_controls(PassiveToneControlPositions::new(0.9, 0.1, 0.7));
        let dk = Circuit::<Dk>::new(&deck("tone_stack_bright")).unwrap();
        let w = (0..60)
            .map(|k| {
                let hz = 20.0 * 10f64.powf(f64::from(k) / 20.0);
                let ours = dk.analog_response(hz).unwrap().magnitude();
                (20.0 * (ours / stack.circuit_magnitude_at(hz)).log10()).abs()
            })
            .fold(0.0, f64::max);
        println!("tone stack: closed form vs DK, largest difference {w:.3e} dB");
        assert!(w < 1e-10, "{w}");
    }
}
