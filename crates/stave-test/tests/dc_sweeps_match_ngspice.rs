//! DC transfer sweeps (DSP.md R3): every deck's operating point held to
//! ngspice's `.dc` analysis as one value moves — the input where it is
//! DC-coupled (the static transfer), the supply, a bias source, or a bias
//! pot or trim (a resistor). `spice/dc_sweeps.list` names each sweep and
//! its probes; `scripts/ngspice-simulate.py` writes
//! `spice/dc_sweeps.ngspice.txt`; here `Circuit<Dk>` solves its
//! operating point at every swept value (the deck rebuilt with that
//! value, every other source at rest as the script holds it) and each
//! probe is compared absolutely, against a budget in volts: the measured
//! error three times over, rounded up to 1-2-5 (at least 1 nV). The tube
//! decks' 2e-5 V on 100–300 V plates is ngspice's own reltol (1e-7 of
//! the plate); every sweep together takes about a second, so all are T0.

use std::collections::BTreeMap;
use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Probe, Source};
use stave_dsp::circuit::Circuit;

fn spice(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("spice").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One line of `dc_sweeps.list`.
#[derive(Clone, Debug)]
struct Sweep {
    id: String,
    deck: String,
    element: String,
    holds: Vec<(String, f64)>,
    probes: Vec<String>,
}

fn sweeps() -> Vec<Sweep> {
    spice("dc_sweeps.list")
        .lines()
        .filter(|l| l.starts_with("sweep "))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let mut i = 7;
            let mut holds = Vec::new();
            while w[i] == "hold" {
                let (name, value) = w[i + 1].split_once('=').unwrap();
                holds.push((name.to_ascii_lowercase(), value.parse().unwrap()));
                i += 2;
            }
            assert_eq!(w[i], "probe", "{l}");
            Sweep {
                id: w[1].into(),
                deck: w[2].into(),
                element: w[3].to_ascii_lowercase(),
                holds,
                probes: w[i + 1..].iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect()
}

/// ngspice's points per sweep: `[value, probe volts…]`.
fn reference() -> BTreeMap<String, Vec<Vec<f64>>> {
    let mut out: BTreeMap<String, Vec<Vec<f64>>> = BTreeMap::new();
    let mut current = None;
    for line in spice("dc_sweeps.ngspice.txt").lines() {
        if let Some(rest) = line.strip_prefix("sweep ") {
            let id = rest.split_whitespace().next().unwrap().to_string();
            out.insert(id.clone(), Vec::new());
            current = Some(id);
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            let row = line.split_whitespace().map(|x| x.parse().unwrap()).collect();
            out.get_mut(current.as_ref().unwrap()).unwrap().push(row);
        }
    }
    out
}

/// `v(a)` or `v(a,b)` on `netlist`.
fn probe(netlist: &Netlist, vector: &str) -> Probe {
    let inner = vector
        .strip_prefix("v(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or_else(|| panic!("probe {vector}: expected v(a) or v(a,b)"));
    let node = |n: &str| netlist.node(n).unwrap_or_else(|| panic!("no node {n}"));
    match inner.split_once(',') {
        Some((a, b)) => Probe {
            plus: node(a),
            minus: node(b),
        },
        None => Probe::node(node(inner)),
    }
}

/// The deck at one swept value, every other source at rest (its value
/// at t = 0, as DK's operating point and the script's `.dc` both take
/// it) or held.
fn at(deck: &Netlist, sweep: &Sweep, value: f64) -> Netlist {
    let mut n = deck.clone();
    let names: Vec<String> = n.elements().iter().map(|e| e.name.clone()).collect();
    for name in names {
        let element = n.element_mut(&name).unwrap();
        let swept = name == sweep.element;
        match &mut element.kind {
            ElementKind::Resistor { ohms, .. } if swept => *ohms = value,
            ElementKind::VoltageSource { source, .. } | ElementKind::CurrentSource { source, .. } => {
                let held = sweep.holds.iter().find(|(h, _)| *h == name).map(|h| h.1);
                let dc = if swept { value } else { held.unwrap_or_else(|| source.value_at(0.0)) };
                *source = Source::dc(dc);
            }
            other if swept => panic!("{}: cannot sweep {other:?}", sweep.id),
            _ => {}
        }
    }
    n
}

/// The worst |error| of each probe over the sweep, volts, and where.
fn measure(sweep: &Sweep, points: &[Vec<f64>]) -> Vec<(f64, String)> {
    let deck = Netlist::parse_spice(&spice(&format!("{}.cir", sweep.deck))).unwrap();
    assert!(deck.element(&sweep.element).is_some(), "{}: no {}", sweep.id, sweep.element);
    let mut worst = vec![(0.0f64, String::new()); sweep.probes.len()];
    for row in points {
        let netlist = at(&deck, sweep, row[0]);
        let circuit = Circuit::<Dk>::new(&netlist)
            .unwrap_or_else(|e| panic!("{} at {}: no operating point: {e:?}", sweep.id, row[0]));
        for (k, vector) in sweep.probes.iter().enumerate() {
            let mut c = circuit.clone();
            c.set_probe(probe(&netlist, vector));
            let (got, want) = (c.output(), row[k + 1]);
            let e = (got - want).abs();
            if e > worst[k].0 || e.is_nan() {
                worst[k] = (e, format!("{vector} at {:.6e}: {want:.9} vs {got:.9} V", row[0]));
            }
        }
    }
    worst
}

/// Each sweep's budget, volts (the worst probe's measured error beside
/// it).
const BUDGETS: &[(&str, f64)] = &[
    ("diode_clipper/input", 2e-6), // 3.76e-7
    ("opamp_clipper/input", 1e-9), // 0
    ("common_cathode/grid", 1e-4), // 1.81e-5
    ("common_cathode_grid_current/grid", 1e-4), // 2.41e-5
    ("triode_stage_cathode/grid", 1e-4), // 2.08e-5
    ("push_pull_cathode/swing", 1e-4), // 2.73e-5
    ("ota_vca/input", 1e-9), // 4.99e-13
    ("diode_ac/bias", 5e-6), // 1.01e-6
    ("common_cathode/supply", 5e-6), // 6.69e-7
    ("triode_stage/supply", 1e-8), // 1.84e-9
    ("triode_stage_cathode/supply", 5e-6), // 9.39e-7
    ("push_pull/supply", 1e-9), // 9.92e-11
    ("push_pull_cathode/supply", 1e-6), // 1.89e-7
    ("push_pull_pentode/supply", 1e-9), // 5.45e-11
    ("grid_blocking/supply", 2e-5), // 3.68e-6
    ("fuzz_face/supply", 1e-6), // 2.36e-7
    ("rangemaster/supply", 5e-7), // 1.02e-7
    ("ce_stage/supply", 5e-6), // 9.22e-7
    ("jfet_booster/supply", 5e-9), // 7.21e-10
    ("mosfet_booster/supply", 5e-7), // 1.02e-7
    ("big_muff_rams_head/supply", 5e-6), // 8.87e-7
    ("boss_ds1/supply", 5e-6), // 1.11e-6
    ("proco_rat/bias", 1e-9), // 2.26e-12
    ("tube_screamer_ts808/supply", 1e-6), // 2.01e-7
    ("mxr_distortion_plus/supply", 1e-9), // 4.46e-13
    ("marshall_jcm800_2203/supply", 5e-8), // 1.52e-8
    ("tube_screamer/reference", 5e-5), // 8.57e-6
    ("pentode_stage_ac/bias", 5e-8), // 8.4e-9
    ("fuzz_face/trim", 2e-6), // 4.16e-7
    ("rangemaster/bias", 5e-7), // 9.87e-8
    ("ce_stage/bias", 5e-6), // 9.06e-7
    ("jfet_booster/bias", 1e-9), // 1.09e-11
    ("mosfet_booster/bias", 5e-7), // 1.32e-7
    ("ota_vca/control", 1e-9), // 6.52e-11
    ("boss_ce2_filters/bias", 1e-6), // 2.15e-7
    ("klon_centaur/bias", 2e-9), // 5.06e-10
    ("mxr_phase90/bias", 2e-6), // 6.07e-7
    ("transistor_ladder/tail", 1e-6), // 2.05e-7
    ("diode_ladder/tail", 5e-6), // 8.21e-7
    ("moog_ladder_feedback/tail", 1e-6), // 2.01e-7
];

fn run(select: impl Fn(&Sweep) -> bool) {
    let reference = reference();
    let chosen: Vec<Sweep> = sweeps().into_iter().filter(|s| select(s)).collect();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = chosen
            .iter()
            .map(|s| {
                let points = reference.get(&s.id).unwrap_or_else(|| {
                    panic!("{}: not in dc_sweeps.ngspice.txt (run scripts/ngspice-simulate.py)", s.id)
                });
                scope.spawn(move || {
                    let started = std::time::Instant::now();
                    (measure(s, points), points.len(), started.elapsed().as_secs_f64())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut failed = Vec::new();
    for (s, (worst, points, seconds)) in chosen.iter().zip(results) {
        let budget = BUDGETS
            .iter()
            .find(|b| b.0 == s.id)
            .unwrap_or_else(|| panic!("{}: no budget", s.id))
            .1;
        let top = worst.iter().map(|w| w.0).fold(0.0, f64::max);
        let verdict = if top <= budget { "ok" } else { "OVER" };
        println!(
            "{verdict:<4} {:<34} {points:>3} points {seconds:>6.2} s  worst {top:.2e} V (budget {budget:.0e})",
            s.id
        );
        for (w, at) in &worst {
            println!("       {w:.2e} V  {at}");
        }
        if top.is_nan() || top > budget {
            failed.push(s.id.clone());
        }
    }
    assert!(failed.is_empty(), "over budget: {failed:?}");
}

/// Every sweep in the list has a reference block and a budget, and every
/// budget a sweep.
#[test]
fn every_sweep_is_simulated_and_budgeted() {
    let reference = reference();
    let list = sweeps();
    for s in &list {
        assert!(reference.contains_key(&s.id), "{}: not simulated", s.id);
        assert!(BUDGETS.iter().any(|b| b.0 == s.id), "{}: no budget", s.id);
    }
    for (id, _) in BUDGETS {
        assert!(list.iter().any(|s| s.id == *id), "{id}: budgeted, not listed");
    }
}

#[test]
fn dc_sweeps_match_ngspice() {
    run(|_| true);
}
