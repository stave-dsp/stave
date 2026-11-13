//! KiCad netlist import (DSP.md G1): a KiCad netlist of a deck in
//! `spice/` reads to the deck's circuit — the same elements on the same
//! nodes, by name — and runs to the deck's output. The fixtures in
//! `kicad/` are written for these tests in KiCad 8's export format; the
//! libraries they name hold the decks' own model cards.
//!
//! The output is held at 1e-12 of peak against the deck with its parts
//! in the KiCad netlist's order, where both number their nodes alike: it
//! is met bit for bit. The diode clipper's parts are in the deck's own
//! order; the Fuzz Face's in KiCad's (by reference), whose renumbering
//! moves the DK solution by rounding (5e-12 of peak, reported).

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::kicad::Import;
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::Circuit;

fn read(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The deck's analyses, options and probe: what a schematic carries as
/// text directives, not as parts.
fn directives(deck: &str) -> String {
    let (mut out, mut keep, mut control) = (String::new(), false, false);
    for line in deck.lines().skip(1) {
        let t = line.trim().to_ascii_lowercase();
        if t.starts_with('+') {
            if keep {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        control |= t.starts_with(".control");
        keep = control || (t.starts_with('.') && !t.starts_with(".model") && !t.starts_with(".param"));
        control &= !t.starts_with(".endc");
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn import(name: &str) -> (String, Netlist, Netlist) {
    let deck_text = read(&format!("spice/{name}.cir"));
    let deck = Netlist::parse_spice(&deck_text).unwrap();
    let kicad = Import::new(&read(&format!("kicad/{name}.net")))
        .library("models.lib", &read("kicad/models.lib"))
        .directives(&directives(&deck_text))
        .netlist()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    (deck_text, deck, kicad)
}

/// Each element as text, its nodes by name: one circuit, however its
/// nodes are numbered.
fn by_name(n: &Netlist) -> Vec<(String, String)> {
    let names: Vec<&str> = n.nodes().map(|node| n.name(node)).collect();
    let mut out: Vec<(String, String)> = n
        .elements()
        .iter()
        .map(|e| {
            let text = format!("{:?}", e.kind);
            let (mut named, mut rest) = (String::new(), text.as_str());
            while let Some(at) = rest.find("Node(") {
                let close = at + rest[at..].find(')').unwrap();
                let index: usize = rest[at + 5..close].parse().unwrap();
                named.push_str(&rest[..at]);
                named.push_str(&format!("Node({})", names[index]));
                rest = &rest[close + 1..];
            }
            named.push_str(rest);
            (e.name.clone(), named)
        })
        .collect();
    out.sort();
    out
}

/// The deck with its element lines in `order` (everything else as
/// written): the same circuit, its nodes numbered in another order.
fn reordered(deck: &str, order: &[String]) -> String {
    let mut lines = deck.lines();
    let mut out = format!("{}\n", lines.next().unwrap_or(""));
    let (mut elements, mut rest, mut control) = (Vec::new(), String::new(), false);
    for line in lines {
        let t = line.trim();
        control |= t.to_ascii_lowercase().starts_with(".control");
        let element = !control && !t.is_empty() && !t.starts_with(['*', '.', '+']);
        control &= !t.to_ascii_lowercase().starts_with(".endc");
        if element {
            let name = t.split_whitespace().next().unwrap().to_ascii_lowercase();
            elements.push((name, line));
        } else {
            rest.push_str(line);
            rest.push('\n');
        }
    }
    for name in order {
        let (_, line) = elements.iter().find(|(n, _)| n == name).unwrap();
        out.push_str(line);
        out.push('\n');
    }
    out + &rest
}

/// The deck's transient at 96 kHz, its own stimulus driving it.
fn run(netlist: &Netlist) -> Vec<f64> {
    let rate = 96_000.0;
    let stop = netlist.tran.unwrap().stop_seconds;
    let mut circuit = Circuit::<Dk>::new(netlist).unwrap();
    circuit.prepare(rate as f32);
    (0..(stop * rate) as usize).map(|_| circuit.step(0.0)).collect()
}

/// The worst difference between two runs, as a fraction of the first's
/// peak.
fn difference(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    let peak = a.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    assert!(peak > 0.01, "the deck makes a signal: {peak}");
    a.iter().zip(b).fold(0.0f64, |m, (x, y)| m.max((x - y).abs())) / peak
}

/// The KiCad netlist reads to the deck's circuit: the same elements on
/// the same nodes by name, the same analysis, temperature and probe;
/// and, numbered as the deck numbers it when the parts are listed in the
/// KiCad netlist's order, the same elements exactly and the same output
/// bit for bit (0 against the budget of 1e-12 of peak).
fn assert_round_trip(name: &str) -> (Vec<f64>, Vec<f64>) {
    let (deck_text, deck, kicad) = import(name);
    assert_eq!(by_name(&kicad), by_name(&deck), "{name}: the same circuit");
    assert_eq!(kicad.tran, deck.tran);
    assert_eq!(kicad.temperature_celsius, deck.temperature_celsius);
    let probe = |n: &Netlist| n.probe.map(|p| (n.name(p.plus).to_string(), n.name(p.minus).to_string()));
    assert_eq!(probe(&kicad), probe(&deck));
    let order: Vec<String> = kicad.elements().iter().map(|e| e.name.clone()).collect();
    let same_order = Netlist::parse_spice(&reordered(&deck_text, &order)).unwrap();
    assert_eq!(kicad.elements(), same_order.elements(), "{name}: numbered alike");
    let (a, b) = (run(&same_order), run(&kicad));
    let worst = difference(&a, &b);
    println!("{name}: {} samples, worst difference {worst:.3e} of peak (budget 1e-12)", a.len());
    assert!(worst <= 1e-12, "{name}: {worst:.3e} of peak (budget 1e-12)");
    assert_eq!(a, b, "{name}: bit for bit");
    (run(&deck), b)
}

#[test]
fn diode_clipper_round_trips() {
    // Listed in the deck's own order: the deck itself, bit for bit.
    let (deck, kicad) = assert_round_trip("diode_clipper");
    assert_eq!(deck, kicad);
}

#[test]
fn fuzz_face_round_trips() {
    // Listed in KiCad's order (by reference). Against the deck in its own
    // order the run differs by the solver's rounding under a renumbering,
    // not by the import: reported, held to 1e-10 of peak.
    let (deck, kicad) = assert_round_trip("fuzz_face");
    let renumbered = difference(&deck, &kicad);
    println!("fuzz_face: renumbered against the deck's order {renumbered:.3e} of peak (budget 1e-10)");
    assert!(renumbered <= 1e-10, "{renumbered:.3e}");
}
