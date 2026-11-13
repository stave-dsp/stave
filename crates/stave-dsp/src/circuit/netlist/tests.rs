use super::expr::{Dual, Tape};
use super::*;
use crate::circuit::devices::triode::koren::Koren;

#[test]
fn values_read_spice_suffixes_and_ignore_units() {
    for (word, x) in [
        ("2.2k", 2.2e3),
        ("1meg", 1e6),
        ("1MEG", 1e6),
        ("20.8333333u", 20.8333333e-6),
        ("250p", 250e-12),
        ("1G", 1e9),
        ("10uF", 10e-6),
        ("1e-3", 1e-3),
        ("2.5e3k", 2.5e6),
        ("3mil", 3.0 * 25.4e-6),
        ("-4", -4.0),
        ("100", 100.0),
    ] {
        let got = spice::value(word).unwrap();
        assert!((got - x).abs() <= 1e-15 * x.abs(), "{word}: {got}");
    }
    assert!(spice::value("k").is_err());
}

const CLIPPER: &str = "* Diode clipper
V1 in 0 SIN(0 1 1k)
R1 in out 2.2k
C1 out 0 10n
D1 out 0 D1N4148
D2 0 out D1N4148
.model D1N4148 D(Is=2.52n N=1.752 Rs=0.568 Cjo=4p M=0.4 tt=20n Bv=100)
.options interp reltol=1e-6
.tran 20.8333333u 10m 0 1u
.control
set wr_singlescale
run
wrdata OUTFILE v(out)
.endc
.end
";

#[test]
fn a_deck_reads_into_elements_models_analyses_and_its_probe() {
    let n = Netlist::parse_spice(CLIPPER).unwrap();
    assert_eq!(n.title, "Diode clipper");
    assert_eq!(n.elements().len(), 5);
    let out = n.node("out").unwrap();
    assert_eq!(n.probe, Some(Probe::node(out)));
    let tran = n.tran.unwrap();
    assert!((tran.step_seconds - 20.8333333e-6).abs() < 1e-18);
    assert!(!tran.uic);
    match &n.element("D2").unwrap().kind {
        ElementKind::Diode {
            anode,
            cathode,
            diode,
        } => {
            assert_eq!((*anode, *cathode), (Node::GROUND, out));
            assert_eq!(diode.saturation_amps, 2.52e-9);
            assert_eq!(diode.emission, 1.752);
            assert_eq!(diode.series_ohms, 0.568);
            // BV given, IBV and NBV at ngspice's defaults.
            let b = diode.breakdown.unwrap();
            assert_eq!((b.volts, b.amps, b.emission), (100.0, 1e-3, 1.752));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        n.unmodelled,
        ["d1n4148: cjo", "d1n4148: m", "d1n4148: tt"]
    );
    match &n.element("v1").unwrap().kind {
        ElementKind::VoltageSource { source, .. } => {
            assert_eq!(source.dc, 0.0);
            assert!((source.value_at(0.25e-3) - 1.0).abs() < 1e-12);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn sources_take_dc_ac_and_their_transient_functions() {
    let n = Netlist::parse_spice(
        "title
VI i 0 DC 0 AC 1
V2 a 0 DC 2 SIN(0 0.5 1k)
V3 b 0 PWL(0 0 1m 1 2m 1)
I1 c 0 20m
R1 i 0 1
R2 a 0 1
R3 b 0 1
R4 c 0 1
",
    )
    .unwrap();
    let source = |name: &str| match &n.element(name).unwrap().kind {
        ElementKind::VoltageSource { source, .. } | ElementKind::CurrentSource { source, .. } => {
            source.clone()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!((source("vi").dc, source("vi").ac), (0.0, 1.0));
    assert_eq!(source("v2").dc, 2.0);
    assert!((source("v2").value_at(0.25e-3) - 0.5).abs() < 1e-12);
    assert_eq!(source("v3").value_at(0.5e-3), 0.5);
    assert_eq!(source("v3").value_at(5e-3), 1.0);
    assert_eq!(source("i1").value_at(1.0), 20e-3);
}

#[test]
fn a_behavioural_source_reads_params_and_node_voltages() {
    let n = Netlist::parse_spice(
        "* Koren plate law as ngspice spells it
.param mu=100 ex=1.4 kg1=1060 kp=600 kvb=300
BE e1 0 V = {V(p,k)/kp*ln(1+exp(kp*(1/mu+V(g,k)/sqrt(kvb+V(p,k)*V(p,k)))))}
BI p k I = {V(e1) > 0 ? 2*pow(V(e1),ex)/kg1 : 0}
R1 p 0 1
R2 g 0 1
R3 k 0 1
",
    )
    .unwrap();
    let (p, g, k, e1) = (
        n.node("p").unwrap(),
        n.node("g").unwrap(),
        n.node("k").unwrap(),
        n.node("e1").unwrap(),
    );
    let ElementKind::BehaviouralVoltage { expression: be, .. } = &n.element("be").unwrap().kind
    else {
        panic!()
    };
    let ElementKind::BehaviouralCurrent { expression: bi, .. } = &n.element("bi").unwrap().kind
    else {
        panic!()
    };
    // ln(1 + exp(…)) folds to the overflow-free softplus.
    assert!(format!("{be:?}").contains("Softplus"), "{be:?}");
    let (vg, vp, vk) = (-1.5 + 1.2, 180.0 + 1.2, 1.2);
    let volts = |node: Node| match node {
        n if n == p => vp,
        n if n == g => vg,
        n if n == k => vk,
        _ => 0.0,
    };
    let e = be.value(&volts, 0.0);
    let plate = bi.value(&|node| if node == e1 { e } else { 0.0 }, 0.0);
    let koren = Koren::T12AX7.plate(vg - vk, vp - vk).amps;
    assert!((plate - koren).abs() < 1e-12 * koren, "{plate} vs {koren}");
}

#[test]
fn a_tape_carries_exact_slopes_through_every_operation() {
    let mut n = Netlist::new();
    let (a, b) = (n.add_node("a"), n.add_node("b"));
    let mut names = |s: &str| if s == "a" { a } else { b };
    let params = [("k".to_string(), 3.0)];
    let mut scope = expr::Scope {
        params: &params,
        node: &mut names,
    };
    let text = "k*tanh(v(a))*exp(-v(b)/2) + pow(abs(v(a,b)), 1.5) - sqrt(1+v(a)^2)/max(v(b), 0.1) \
                + (v(a) > v(b) ? atan(v(a)) : ln(2+v(b)))";
    let e = Expr::parse(text, &mut scope, 1).unwrap();
    let tape = Tape::compile(&e).unwrap();
    assert_eq!(tape.controls(), [(a, Node::GROUND), (b, Node::GROUND), (a, b)]);
    let mut stack = vec![Dual::default(); tape.depth()];
    for (x, y) in [(0.3, -0.7), (-1.1, 0.4), (2.0, 1.5)] {
        let controls = [x, y, x - y];
        let d = tape.eval(&controls, 0.0, &mut stack);
        let at = |x: f64, y: f64| tape.eval(&[x, y, x - y], 0.0, &mut stack.clone()).value;
        let plain = e.value(&|node| if node == a { x } else if node == b { y } else { 0.0 }, 0.0);
        assert!((d.value - plain).abs() < 1e-12, "{x} {y}");
        // The total derivative in each node voltage, by central differences.
        let h = 1e-6;
        let dx = (at(x + h, y) - at(x - h, y)) / (2.0 * h);
        let dy = (at(x, y + h) - at(x, y - h)) / (2.0 * h);
        assert!((d.slope[0] + d.slope[2] - dx).abs() < 1e-6, "{x} {y}: {d:?} {dx}");
        assert!((d.slope[1] - d.slope[2] - dy).abs() < 1e-6, "{x} {y}: {d:?} {dy}");
    }
}

#[test]
fn errors_name_the_line_and_the_problem() {
    let unknown = Netlist::parse_spice("t\nD1 a 0 NOPE\nR1 a 0 1\n").unwrap_err();
    assert_eq!(
        unknown,
        NetlistError::UnknownModel {
            line: 2,
            model: "nope".into()
        }
    );
    let subckt = Netlist::parse_spice("t\nR1 a 0 1\nX1 a 0 opamp\n").unwrap_err();
    assert!(matches!(subckt, NetlistError::Syntax { line: 3, .. }), "{subckt:?}");
    let bad = Netlist::parse_spice("t\nR1 a 0 1k\nB1 a 0 I = {v(a) +* 2}\n").unwrap_err();
    assert!(matches!(bad, NetlistError::Syntax { line: 3, .. }), "{bad:?}");
    let twice = Netlist::parse_spice("t\nR1 a 0 1\nr1 a 0 2\n").unwrap_err();
    assert_eq!(twice, NetlistError::Duplicate { name: "r1".into() });
}

#[test]
fn continuations_and_comments_join_and_vanish() {
    let n = Netlist::parse_spice(
        "title
* a comment
V1 in 0 PWL(0 0
+ 1m 1) ; inline
R1 in 0 1k
",
    )
    .unwrap();
    match &n.element("v1").unwrap().kind {
        ElementKind::VoltageSource { source, .. } => {
            assert_eq!(source.value_at(1e-3), 1.0)
        }
        other => panic!("{other:?}"),
    }
}
