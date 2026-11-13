use super::*;
use crate::circuit::devices::opamp::{Amp, Ideal};

fn parse(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap()
}

/// Run WDF and DK side by side on `netlist`, `V1` driven by `input`
/// (or the deck's own stimulus when `drive` is false); the largest
/// difference relative to the largest output.
fn against_dk(netlist: &Netlist, drive: bool, rate: f32, frames: usize, input: impl Fn(usize) -> f64) -> f64 {
    let mut wdf = Wdf::new(netlist).unwrap();
    let mut dk = Dk::new(netlist).unwrap();
    if drive {
        wdf.drive("v1").unwrap();
        dk.drive("v1").unwrap();
    }
    wdf.prepare(rate);
    dk.prepare(rate);
    assert!((wdf.output() - dk.output()).abs() < 1e-9, "operating point");
    let (mut worst, mut peak) = (0.0f64, 0.0f64);
    for n in 0..frames {
        let x = input(n);
        let (a, b) = (wdf.step(x), dk.step(x));
        worst = worst.max((a - b).abs());
        peak = peak.max(b.abs());
    }
    assert_eq!(wdf.stats().capped, 0);
    worst / peak.max(1e-30)
}

const RC: &str = "* RC\nV1 in 0 SIN(0 1 1k)\nR1 in out 1k\nC1 out 0 100n\n.tran 20.8333333u 5m\n.control\nwrdata f v(out)\n.endc";

#[test]
fn an_rc_lowpass_is_one_tree_and_agrees_with_dk() {
    let netlist = parse(RC);
    let wdf = Wdf::new(&netlist).unwrap();
    let s = wdf.structure();
    assert!(s.is_tree(), "{s:?}");
    // V1 + R1 in series, then in parallel with C1 (turned round).
    assert_eq!((s.series, s.parallel, s.leaves, s.absorbed), (1, 1, 3, 0), "{s:?}");
    let e = against_dk(&netlist, false, 48_000.0, 4_800, |_| 0.0);
    assert!(e < 1e-12, "{e:e}");
}

const TONE_STACK: &str = "* tone stack\nVI i 0 DC 0 AC 1\nC1 i t 250p\nRT1 t o 125000\nRT2 o b 125000\nR4 i n 100000\n\
    C2 n b 100n\nRB b j 500000\nRM1 j w 12500\nRM2 w 0 12500\nC3 n w 47n\n.ac dec 20 20 20k\n\
    .control\nwrdata f vm(o)\n.endc";

/// The tone stack has no series–parallel form: an R-type root of
/// several ports, whose measured response is DK's.
#[test]
fn the_tone_stack_needs_an_r_type_root() {
    let netlist = parse(TONE_STACK);
    let wdf = Wdf::new(&netlist).unwrap();
    let s = wdf.structure();
    assert!(!s.is_tree() && s.ports >= 4 && s.stateful_ports == 3, "{s:?}");
    let dk = Dk::new(&netlist).unwrap();
    for hz in [20.0, 200.0, 1e3, 5e3, 20e3] {
        let (a, b) = (wdf.analog_response(hz).unwrap(), dk.analog_response(hz).unwrap());
        let e = (a.magnitude() / b.magnitude() - 1.0).abs();
        assert!(e < 1e-11, "{hz} Hz: {e:e}");
        assert!((a.phase() - b.phase()).abs() < 1e-9, "{hz} Hz phase");
    }
}

const CLIPPER: &str = "clipper\nV1 in 0 0\nR1 in out 2.2k\nC1 out 0 10n\nD1 out 0 DI\nD2 0 out DI\n\
    .model DI D(Is=2.52n N=1.752)\n.control\nwrdata f v(out)\n.endc";

/// The diode clipper: a tree facing a one-port nonlinear root, driven
/// hot at 48 kHz, wave for wave with DK.
#[test]
fn the_diode_clipper_agrees_with_dk_driven_hot() {
    let netlist = parse(CLIPPER);
    let wdf = Wdf::new(&netlist).unwrap();
    let s = wdf.structure();
    assert!(s.is_tree() && s.nonlinear_ports == 1, "{s:?}");
    let hot = |n: usize| 8.0 * (std::f64::consts::TAU * 1_234.5 * n as f64 / 48e3).sin();
    let e = against_dk(&netlist, true, 48_000.0, 9_600, hot);
    assert!(e < 1e-8, "{e:e}");
}

/// An inductor and `UIC`: a charged LC tank through a resistor rings as
/// DK's does.
#[test]
fn an_rlc_tank_from_initial_conditions_agrees_with_dk() {
    let netlist = parse(
        "* RLC\nC1 a 0 1u IC=1\nL1 a b 10m\nR1 b 0 10\n.tran 20.8333333u 5m UIC\n.control\nwrdata f v(a)\n.endc",
    );
    let e = against_dk(&netlist, false, 48_000.0, 4_800, |_| 0.0);
    assert!(e < 1e-11, "{e:e}");
}

/// An inverting amplifier with the ideal op-amp element absorbed into
/// the root, and a capacitor across its feedback resistor.
#[test]
fn an_absorbed_op_amp_agrees_with_dk() {
    let mut netlist = parse("* inverting\nV1 in 0 0\nR1 in m 10k\nRF m out 100k\nCF m out 1n\nRL out 0 10k\n.control\nwrdata f v(out)\n.endc");
    let (plus, minus, output) = (Node::GROUND, netlist.node("m").unwrap(), netlist.node("out").unwrap());
    netlist
        .add(
            "u1",
            ElementKind::OpAmp {
                plus,
                minus,
                output,
                amp: Amp::Ideal(Ideal),
                supply: (-15.0, 15.0),
            },
        )
        .unwrap();
    let wdf = Wdf::new(&netlist).unwrap();
    assert_eq!(wdf.structure().absorbed, 1);
    let x = |n: usize| (std::f64::consts::TAU * 3e3 * n as f64 / 48e3).sin();
    let e = against_dk(&netlist, true, 48_000.0, 4_800, x);
    assert!(e < 1e-12, "{e:e}");
}

/// What a wave port cannot be is refused by name.
#[test]
fn a_negative_resistance_is_refused() {
    let netlist = parse("* bad\nV1 in 0 1\nR1 in out -1k\nR2 out 0 1k\n.control\nwrdata f v(out)\n.endc");
    match Wdf::new(&netlist) {
        Err(NetlistError::Unsupported { element, reason }) => {
            assert_eq!(element, "r1");
            assert!(reason.contains("wave port"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

/// The scattering iterative method on the diode clipper: each diode its
/// own wave port and scalar solve, swept through the root — the same
/// waveform as the Newton root, driven hot.
#[test]
fn the_scattering_iterative_method_agrees_with_the_newton_root() {
    let netlist = parse(CLIPPER);
    let (mut sim, mut newton) = (Wdf::new(&netlist).unwrap(), Wdf::new(&netlist).unwrap());
    sim.set_nonlinear_root(NonlinearRoot::Scattering).unwrap();
    for c in [&mut sim, &mut newton] {
        c.drive("v1").unwrap();
        c.prepare(48_000.0);
    }
    let mut worst = 0.0f64;
    for n in 0..9_600 {
        let x = 8.0 * (std::f64::consts::TAU * 1_234.5 * n as f64 / 48e3).sin();
        worst = worst.max((sim.step(x) - newton.step(x)).abs());
    }
    let h = sim.health();
    assert_eq!(h.newton.capped, 0, "{h:?}");
    assert!(worst < 1e-8, "{worst:e}");
}

/// A multi-port device cannot be a scattering port: refused, naming the
/// root to use instead.
#[test]
fn the_scattering_iterative_method_refuses_multi_port_devices() {
    let netlist = parse("* B\nV1 in 0 0\nR1 in out 1k\nB1 out 0 I = {1m*tanh(V(out))}\n.control\nwrdata f v(out)\n.endc");
    let mut wdf = Wdf::new(&netlist).unwrap();
    match wdf.set_nonlinear_root(NonlinearRoot::Scattering) {
        Err(NetlistError::Unsupported { reason, .. }) => assert!(reason.contains("Newton"), "{reason}"),
        other => panic!("{other:?}"),
    }
}
