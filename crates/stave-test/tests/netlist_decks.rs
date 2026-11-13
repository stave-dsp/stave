//! One netlist feeds ngspice and stave: every deck in `spice/` reads
//! into a `stave_dsp::circuit::netlist::Netlist`, with the probe its
//! `wrdata` writes and the analysis its reference was made by.

use std::path::PathBuf;

use stave_dsp::circuit::netlist::{ElementKind, Netlist};

fn decks() -> Vec<(String, Netlist)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("spice");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "cir") {
            let text = std::fs::read_to_string(&path).unwrap();
            let netlist =
                Netlist::parse_spice(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            out.push((name, netlist));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_deck_reads_with_its_probe_and_analysis() {
    let decks = decks();
    assert!(decks.len() >= 13, "{} decks", decks.len());
    for (name, netlist) in &decks {
        assert!(netlist.probe.is_some(), "{name}: no probe");
        assert!(
            netlist.tran.is_some() != netlist.ac.is_some(),
            "{name}: one analysis"
        );
        assert!(!netlist.elements().is_empty(), "{name}");
    }
}

#[test]
fn the_decks_elements_read_as_written() {
    let decks = decks();
    let find = |name: &str| &decks.iter().find(|(n, _)| n == name).unwrap().1;
    let supply = find("power_supply");
    assert!(supply.tran.unwrap().uic);
    assert!(matches!(
        supply.element("L1").unwrap().kind,
        ElementKind::Inductor {
            henries: 10.0,
            initial_amps: Some(0.0),
            ..
        }
    ));
    let clipper = find("opamp_clipper");
    assert!(matches!(
        clipper.element("F1").unwrap().kind,
        ElementKind::CurrentControlledCurrent { gain: 1.0, .. }
    ));
    let cathode = find("common_cathode_grid_current");
    assert!(matches!(
        cathode.element("BP").unwrap().kind,
        ElementKind::BehaviouralCurrent { .. }
    ));
    // What the WDF clipper and DK leave out of the 1N4148 card, on record.
    assert_eq!(find("diode_clipper").unmodelled.len(), 4);
}
