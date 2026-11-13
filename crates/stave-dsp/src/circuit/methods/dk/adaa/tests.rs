use super::super::Dk;
use super::Mean;
use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::netlist::Netlist;
use crate::circuit::topologies::diode_clipper::{DiodeClipper, DiodeClipperConfig};
use crate::kernel::node::Stage;

fn deck(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap()
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

fn adaa(text: &str) -> Dk {
    let mut dk = Dk::new(&deck(text)).unwrap();
    dk.drive("v1").unwrap();
    dk.set_antialiasing(Antialiasing::Adaa);
    dk
}

/// For a linear circuit, ADAA is Holters' eq. (13): the response the
/// method reports is the one a bin-centred tone measures.
#[test]
fn a_linear_circuit_has_holters_response() {
    let mut dk = adaa(
        "rc
V1 in 0 0
R1 in out 1k
C1 out 0 22n
.control
wrdata out v(out)
.endc",
    );
    dk.prepare(48_000.0);
    let n = 4_800;
    for bin in [30, 300, 900, 1_500] {
        let w = std::f64::consts::TAU * bin as f64 / n as f64;
        dk.reset();
        for i in 0..4 * n {
            dk.step((w * i as f64).sin());
        }
        let (mut re, mut im) = (0.0, 0.0);
        for i in 4 * n..5 * n {
            let y = dk.step((w * i as f64).sin());
            re += y * (w * i as f64).sin();
            im += y * (w * i as f64).cos();
        }
        // y = |H|·sin(wi + φ): re = |H|cos φ·n/2, im = |H|sin φ·n/2.
        let (re, im) = (2.0 * re / n as f64, 2.0 * im / n as f64);
        let h = dk.frequency_response(bin as f64 * 10.0).unwrap();
        assert!((re - h.re).abs() < 1e-9 && (im - h.im).abs() < 1e-9, "bin {bin}: ({re}, {im}) vs {h:?}");
    }
}

/// The DK closed form and the wave-digital root are one derivation: on
/// the same clipper (no series resistance) they agree to the solves'
/// tolerance.
#[test]
fn the_closed_form_is_the_wave_digital_root() {
    let mut dk = adaa(CLIPPER);
    assert_eq!(dk.adaa_mean(), Some(Mean::ClosedForm));
    dk.prepare(96_000.0);
    let mut wdf = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::N4148,
        solver: crate::circuit::topologies::diode_clipper::PairSolver::default(),
    });
    wdf.set_antialiasing(Antialiasing::Adaa);
    wdf.prepare(96_000.0);
    let mut worst: f64 = 0.0;
    for i in 0..9_600 {
        let x = 6.0 * (i as f64 * 0.071).sin() + 2.0 * (i as f64 * 0.53).sin();
        let a = dk.step(x);
        let b = f64::from(wdf.tick(x as f32));
        worst = worst.max((a - b).abs());
    }
    assert!(worst < 1e-5, "{worst}");
}

/// A circuit at rest stays there with ADAA on, in either form (two
/// diodes behind their own series resistances are two controls, so
/// quadrature): the ill-conditioned `Δp = 0` is no special case.
#[test]
fn a_circuit_at_rest_stays_at_rest() {
    let biased = "divider
VB b 0 300
V1 in 0 0
R0 in p 1meg
R1 b p 100k
R2 p 0 100k
C1 p 0 1u
D1 p 0 DI
D2 p 0 DI
.model DI D(Is=1e-14 N=1 RS=10)
.control
wrdata out v(p)
.endc";
    for (text, mean) in [
        (CLIPPER, Mean::ClosedForm),
        (biased, Mean::Quadrature { nodes: 3 }),
    ] {
        let mut dk = adaa(text);
        assert_eq!(dk.adaa_mean(), Some(mean));
        let rest = dk.output();
        for _ in 0..1_000 {
            assert!((dk.step(0.0) - rest).abs() < 1e-9, "{mean:?}");
        }
        assert_eq!(dk.health().newton.capped, 0);
    }
}

/// A slow signal is barely changed: ADAA is the plain circuit half a
/// sample late, to second order in the step.
#[test]
fn a_slow_signal_is_the_plain_circuit_half_a_sample_late() {
    let mut plain = Dk::new(&deck(CLIPPER)).unwrap();
    plain.drive("v1").unwrap();
    plain.prepare(48_000.0);
    let mut dk = adaa(CLIPPER);
    let w = std::f64::consts::TAU * 50.0 / 48_000.0;
    let mut worst: f64 = 0.0;
    for i in 0..4_800 {
        let a = dk.step(3.0 * (w * i as f64).sin());
        let b = plain.step(3.0 * (w * (i as f64 - 0.5)).sin());
        if i > 100 {
            worst = worst.max((a - b).abs());
        }
    }
    assert!(worst < 2e-3, "{worst}");
}
