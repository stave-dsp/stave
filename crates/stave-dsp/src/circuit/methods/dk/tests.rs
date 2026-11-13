use super::*;
use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::NewtonSettings;
use crate::circuit::netlist::Netlist;
use crate::circuit::topologies::coupler::Coupler;
use crate::circuit::topologies::diode_clipper::{DiodeClipper, DiodeClipperConfig};
use crate::kernel::node::Stage;

fn deck(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap()
}

/// The coupler is the trapezoidal RC high-pass written out by hand; DK
/// derives the same recursion from the netlist, so they agree to rounding.
#[test]
fn a_linear_netlist_is_the_hand_written_trapezoid() {
    let netlist = deck(
        "coupler
V1 i 0 0
C1 i o 22n
R1 o 0 1meg
.control
wrdata out v(o)
.endc",
    );
    let mut dk = Dk::new(&netlist).unwrap();
    dk.drive("v1").unwrap();
    dk.prepare(48_000.0);
    let corner = 1.0 / (std::f64::consts::TAU * 22e-9 * 1e6);
    let mut hand = Coupler::new(corner as f32);
    hand.prepare(48_000.0);
    for n in 0..2_000 {
        let x = (n as f32 * 0.01).sin();
        let (a, b) = (dk.step(f64::from(x)), f64::from(hand.tick(x)));
        assert!((a - b).abs() < 1e-6, "{n}: {a} vs {b}");
    }
    assert_eq!(dk.dimensions(), (3, 1, 0, 0));
}

/// The trapezoidal rule is the bilinear transform: prewarped, the
/// discrete response is the analog one exactly.
#[test]
fn the_analog_response_of_a_linear_netlist_is_exact() {
    let netlist = deck(
        "rc
V1 in 0 DC 0 AC 1
R1 in out 1k
C1 out 0 100n
.control
wrdata out vm(out)
.endc",
    );
    let dk = Dk::new(&netlist).unwrap();
    for hz in [10.0, 1_591.5, 10_000.0, 23_000.0] {
        let h = dk.analog_response(hz).unwrap();
        let wrc = std::f64::consts::TAU * hz * 1e3 * 100e-9;
        let exact = 1.0 / (1.0 + wrc * wrc).sqrt();
        assert!((h.magnitude() - exact).abs() < 1e-12, "{hz}: {h:?}");
        assert!((h.phase() + wrc.atan()).abs() < 1e-12, "{hz}: {h:?}");
    }
    assert!(dk.frequency_response(10.0).is_some());
}

const CLIPPER: &str = "clipper
V1 in 0 0
R1 in out 2.2k
C1 out 0 10n
D1 out 0 DI
D2 0 out DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(out)
.endc";

/// The WDF clipper and DK solve the same discretised circuit (the
/// bilinear capacitor is the trapezoidal one), so they agree to the
/// Newton tolerance; the pair shares one controlling voltage.
#[test]
fn the_diode_clipper_agrees_with_its_wave_digital_form() {
    let mut dk = Dk::new(&deck(CLIPPER)).unwrap();
    dk.drive("v1").unwrap();
    dk.prepare(192_000.0);
    assert_eq!(dk.dimensions().2, 1, "one control for the pair");
    let mut wdf = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::D1N4148,
        solver: NewtonSettings::DEFAULT,
    });
    wdf.prepare(192_000.0);
    let mut worst = 0.0f64;
    for n in 0..4_000 {
        let x = 4.0 * (std::f64::consts::TAU * 1_000.0 * n as f64 / 192_000.0).sin();
        let a = dk.step(x);
        let b = f64::from(wdf.tick(x as f32));
        worst = worst.max((a - b).abs());
    }
    assert!(worst < 1e-6, "{worst}");
    assert_eq!(dk.stats().capped, 0);
}

/// The operating point holds: with no input, nothing moves.
#[test]
fn a_biased_circuit_rests_at_its_operating_point() {
    let netlist = deck(
        "divider with a cap
VB b 0 300
R1 b p 100k
R2 p 0 100k
C1 p 0 1u
D1 p 0 DI
.model DI D(Is=1e-14 N=1)
.control
wrdata out v(p)
.endc",
    );
    let mut dk = Dk::new(&netlist).unwrap();
    let rest = dk.output();
    assert!((0.5..0.9).contains(&rest), "{rest}");
    for _ in 0..1_000 {
        assert!((dk.step(0.0) - rest).abs() < 1e-9);
    }
}

#[test]
fn a_floating_node_is_an_error_not_a_guess() {
    let netlist = deck(
        "loop of sources
V1 a 0 1
V2 a 0 2
R1 a 0 1
.control
wrdata out v(a)
.endc",
    );
    assert!(matches!(
        Dk::new(&netlist).unwrap_err(),
        NetlistError::Singular { .. }
    ));
    let no_probe = deck("t\nV1 a 0 1\nR1 a 0 1\n");
    assert_eq!(Dk::new(&no_probe).unwrap_err(), NetlistError::NoProbe);
}
