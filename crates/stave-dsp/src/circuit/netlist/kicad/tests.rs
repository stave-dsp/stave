use super::*;
use crate::circuit::netlist::{ElementKind, Node, Signal};

#[test]
fn values_read_kicad_notation() {
    for (word, x) in [
        ("10k", 10e3),
        ("4k7", 4.7e3),
        ("2R2", 2.2),
        ("10R", 10.0),
        ("4n7", 4.7e-9),
        ("100n", 100e-9),
        ("1M", 1e6),
        ("1m", 1e-3),
        ("3e-4k", 0.3),
        ("10uF", 10e-6),
        ("10µ", 10e-6),
        ("1kΩ", 1e3),
        ("-9", -9.0),
        ("2.2k", 2.2e3),
    ] {
        let got = value(word).unwrap();
        assert!((got - x).abs() <= 1e-15 * x.abs(), "{word}: {got}");
    }
    for bad in ["k", "", "1k2k3", "abc"] {
        assert!(value(bad).is_err(), "{bad}");
    }
    // The scale applied as the SPICE reader applies it: the same f64.
    for word in ["2.2k", "20.8333333u", "4p", "0.568", "1.752", "2.52n"] {
        assert_eq!(value(word).unwrap(), crate::circuit::netlist::spice::value(word).unwrap(), "{word}");
    }
}

const RC: &str = r#"(export (version "E")
  (design (source "/somewhere/rc.kicad_sch") (tool "Eeschema 8.0"))
  (components
    (comp (ref "V1") (value "SIN")
      (property (name "Sim.Device") (value "V"))
      (property (name "Sim.Type") (value "SIN"))
      (property (name "Sim.Params") (value "dc=0 ampl=1 f=1k ac=1"))
      (property (name "Sim.Pins") (value "1=+ 2=-")))
    (comp (ref "R1") (value "4k7"))
    (comp (ref "C1") (value "100n")
      (fields (field (name "Footprint") "C_0805")))
    (comp (ref "TP1") (value "TestPoint")
      (property (name "Sim.Enable") (value "0"))))
  (nets
    (net (code "1") (name "/in") (node (ref "V1") (pin "1")) (node (ref "R1") (pin "1")))
    (net (code "2") (name "Net-(C1-Pad1)") (node (ref "R1") (pin "2")) (node (ref "C1") (pin "1")) (node (ref "TP1") (pin "1")))
    (net (code "3") (name "GND") (node (ref "C1") (pin "2")) (node (ref "V1") (pin "2")))))"#;

#[test]
fn an_rc_reads_with_inferred_passives_and_a_sine() {
    let n = Import::new(RC)
        .directives(".tran 10u 1m\n.control\nwrdata out v(net-_c1-pad1_)\n.endc")
        .netlist()
        .unwrap();
    assert_eq!(n.title, "KiCad netlist: rc.kicad_sch");
    assert_eq!(n.elements().len(), 3, "the excluded test point is left out");
    let out = n.node("net-_c1-pad1_").unwrap();
    assert!(matches!(n.element("r1").unwrap().kind, ElementKind::Resistor { ohms, b, .. } if ohms == 4.7e3 && b == out));
    let ElementKind::VoltageSource { source, .. } = &n.element("v1").unwrap().kind else {
        panic!("v1 is a voltage source")
    };
    assert_eq!(source.ac, 1.0);
    assert!(matches!(source.signal, Some(Signal::Sine { hz, amplitude, .. }) if hz == 1e3 && amplitude == 1.0));
    assert_eq!(n.probe.unwrap().plus, out);
    assert!(n.tran.is_some());
}

fn one_part(comp: &str, nets: &str) -> String {
    format!(r#"(export (version "E") (components {comp}) (nets {nets}))"#)
}

const TWO_PINS: &str = r#"(net (code "1") (name "a") (node (ref "U1") (pin "1"))) (net (code "2") (name "b") (node (ref "U1") (pin "2")))"#;

#[test]
fn unsupported_parts_are_named() {
    for (comp, says) in [
        (r#"(comp (ref "U1") (value "TL072"))"#, "no simulation model"),
        (
            r#"(comp (ref "U1") (value "x") (property (name "Sim.Device") (value "SUBCKT")))"#,
            "subcircuit",
        ),
        (
            r#"(comp (ref "U1") (value "x") (property (name "Sim.Device") (value "NPN")) (property (name "Sim.Type") (value "VBIC")))"#,
            "VBIC",
        ),
        (
            r#"(comp (ref "U1") (value "x") (property (name "Sim.Device") (value "V")) (property (name "Sim.Type") (value "PULSE")))"#,
            "PULSE",
        ),
        (
            r#"(comp (ref "U1") (value "x") (property (name "Sim.Device") (value "D")) (property (name "Sim.Library") (value "parts.lib")) (property (name "Sim.Name") (value "1N4148")))"#,
            "parts.lib was not given",
        ),
    ] {
        let e = Netlist::parse_kicad(&one_part(comp, TWO_PINS)).unwrap_err();
        let NetlistError::Unsupported { element, reason } = &e else {
            panic!("{e}")
        };
        assert_eq!(element, "U1");
        assert!(reason.contains(says), "{reason}");
    }
}

#[test]
fn a_bad_value_names_its_part() {
    let comp = r#"(comp (ref "R7") (value "lots"))"#;
    let nets = r#"(net (code "1") (name "a") (node (ref "R7") (pin "1"))) (net (code "2") (name "b") (node (ref "R7") (pin "2")))"#;
    let e = Netlist::parse_kicad(&one_part(comp, nets)).unwrap_err();
    assert!(matches!(&e, NetlistError::Unsupported { element, .. } if element == "R7"), "{e}");
}

#[test]
fn library_cards_take_overrides_and_pins_map() {
    let comp = r#"(comp (ref "D1") (value "1N4148")
        (property (name "Sim.Device") (value "D"))
        (property (name "Sim.Library") (value "${KIPRJMOD}/models/diodes.lib"))
        (property (name "Sim.Name") (value "1N4148"))
        (property (name "Sim.Params") (value "n=2"))
        (property (name "Sim.Pins") (value "1=K 2=A")))"#;
    let nets = r#"(net (code "1") (name "/k") (node (ref "D1") (pin "1"))) (net (code "2") (name "/a") (node (ref "D1") (pin "2")))"#;
    let lib = "* diodes\n.model 1N4148 D(Is=2.52n N=1.752\n+ Rs=0.568)\n";
    let n = Import::new(&one_part(comp, nets)).library("diodes.lib", lib).netlist().unwrap();
    let ElementKind::Diode { anode, cathode, diode } = n.element("d1").unwrap().kind else {
        panic!("d1 is a diode")
    };
    assert_eq!((anode, cathode), (n.node("a").unwrap(), n.node("k").unwrap()));
    assert_eq!(diode.emission, 2.0, "Sim.Params over the card");
    assert_eq!(diode.series_ohms, 0.568, "the card's continuation read");
}

#[test]
fn a_pot_becomes_two_legs_and_a_variable() {
    let comp = r#"(comp (ref "RV1") (value "100k")
        (property (name "Sim.Device") (value "R"))
        (property (name "Sim.Type") (value "POT"))
        (property (name "Sim.Params") (value "r=100k pos=0.25"))
        (property (name "Sim.Pins") (value "1=r0 2=wiper 3=r1")))"#;
    let nets = r#"(net (code "1") (name "GND") (node (ref "RV1") (pin "1"))) (net (code "2") (name "/w") (node (ref "RV1") (pin "2"))) (net (code "3") (name "/top") (node (ref "RV1") (pin "3")))"#;
    let n = Netlist::parse_kicad(&one_part(comp, nets)).unwrap();
    assert!(matches!(n.element("rv1_a").unwrap().kind, ElementKind::Resistor { ohms, .. } if ohms == 25e3));
    assert!(matches!(n.element("rv1_b").unwrap().kind, ElementKind::Resistor { ohms, .. } if ohms == 75e3));
    assert_eq!(n.variables().len(), 1);
}

#[test]
fn kicad_5_fields_read() {
    let comp = r#"(comp (ref "V2") (value "DC 10")
        (fields (field (name "Spice_Primitive") "V") (field (name "Spice_Node_Sequence") "2 1")))"#;
    let nets = r#"(net (code "1") (name "GND") (node (ref "V2") (pin "1"))) (net (code "2") (name "VDD") (node (ref "V2") (pin "2")))"#;
    let n = Netlist::parse_kicad(&one_part(comp, nets)).unwrap();
    let ElementKind::VoltageSource { a, b, source } = &n.element("v2").unwrap().kind else {
        panic!("v2 is a voltage source")
    };
    assert_eq!((*a, *b), (n.node("vdd").unwrap(), Node::GROUND));
    assert_eq!(source.dc, 10.0);
}

#[test]
fn malformed_files_say_where() {
    assert!(matches!(Netlist::parse_kicad("(export (components"), Err(NetlistError::Syntax { line: 1, .. })));
    assert!(matches!(Netlist::parse_kicad("(design)"), Err(NetlistError::Syntax { .. })));
    assert!(matches!(Netlist::parse_kicad("(export \"open"), Err(NetlistError::Syntax { .. })));
}
