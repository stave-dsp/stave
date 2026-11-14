//! N8: every solver's health reaches telemetry. Each topology — the
//! hand-derived ones, and every deck through `Circuit<Dk>` and
//! `Circuit<Wdf>` — runs a hot signal at the host rate in 64-sample
//! blocks, publishing each block's account through stave-core's
//! `SolverTaps`, the path a plugin uses; then the telemetry block, read
//! as a tool reads it, must show solves on every tap and not one cap hit
//! over the whole run.

use std::path::PathBuf;

use stave_core::solver_taps::{SolverTapIds, SolverTaps};
use stave_core::telemetry::Telemetry;
use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::methods::newton::NewtonStats;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::{Circuit, Method};
use stave_dsp::{
    CommonCathode, CommonCathodeConfig, Dempwolf, DiodeClipper, DiodeClipperConfig, OpAmpClipper,
    OpAmpClipperConfig, PairSolver, PowerSupply, PowerSupplyConfig, Shockley, Stage,
};
use stave_dsp::units::Db;
use stave_dsp::units::db;

const RATE: f64 = 48_000.0;
const BLOCK: usize = 64;
const SAMPLES: usize = 12_000;

fn hot(n: usize, volts: f64) -> f64 {
    let t = n as f64 / RATE;
    volts * ((std::f64::consts::TAU * 110.0 * t).sin() + 0.5 * (std::f64::consts::TAU * 2_750.0 * t).sin())
}

/// What the taps showed over the run: (largest mean iterations, largest
/// cap count in a block, blocks published), as a tool reads them.
fn read(telemetry: &Telemetry, name: &str) {
    let ids = SolverTapIds::DEFAULT;
    let taps = telemetry.taps();
    let slot = |id: stave_core::telemetry::TapId| {
        taps.iter()
            .find(|t| t.name == id.name())
            .unwrap_or_else(|| panic!("{name}: no {} tap", id.name()))
    };
    let (iterations, capped, peak) = (slot(ids.iterations), slot(ids.capped), slot(ids.peak_iterations));
    let residual = slot(ids.residual);
    println!(
        "  {name:<34} {:>4} blocks, iterations/sample ≤ {:.2}, peak {:>2}, capped ≤ {}, residual ≤ {:.1e}",
        iterations.hits,
        iterations.max,
        peak.max,
        capped.max,
        if residual.hits > 0 { residual.max } else { f64::NAN }
    );
    assert!(iterations.hits > 0 && iterations.max > 0.0, "{name}: no solves published");
    assert_eq!(capped.hits, iterations.hits, "{name}: every block publishes its cap count");
    assert_eq!(capped.max, 0.0, "{name}: a solve ended at its cap");
}

/// A hand topology, its cumulative counts windowed per block.
fn hand(name: &str, mut tick: impl FnMut(usize), mut stats: impl FnMut() -> NewtonStats) {
    let telemetry = Telemetry::new();
    let taps = SolverTaps::new(&telemetry, SolverTapIds::DEFAULT);
    let mut before = stats();
    for start in (0..SAMPLES).step_by(BLOCK) {
        for n in start..start + BLOCK {
            tick(n);
        }
        let now = stats();
        let mut window = now;
        window.solves -= before.solves;
        window.iterations -= before.iterations;
        window.capped -= before.capped;
        taps.publish(&Health::from(window));
        before = now;
    }
    read(&telemetry, name);
}

fn deck(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    std::fs::read_to_string(path).unwrap()
}

/// A netlist circuit, each block's `take_health` published.
fn circuit<M: Method>(name: &str, netlist: &Netlist, source: &str, input: impl Fn(usize) -> f64)
where
    Circuit<M>: Drivable,
{
    let telemetry = Telemetry::new();
    let taps = SolverTaps::new(&telemetry, SolverTapIds::DEFAULT);
    let mut c = Circuit::<M>::new(netlist).unwrap();
    c.drive_named(source);
    c.prepare(RATE as f32);
    for start in (0..SAMPLES).step_by(BLOCK) {
        for n in start..start + BLOCK {
            assert!(c.tick_f64(input(n)).is_finite(), "{name}: sample {n}");
        }
        taps.publish(&c.window());
    }
    read(&telemetry, name);
}

trait Drivable {
    fn drive_named(&mut self, source: &str);
    fn window(&mut self) -> Health;
}

impl Drivable for Circuit<Dk> {
    fn drive_named(&mut self, source: &str) {
        self.drive(source).unwrap();
    }
    fn window(&mut self) -> Health {
        self.take_health()
    }
}

impl Drivable for Circuit<Wdf> {
    fn drive_named(&mut self, source: &str) {
        self.drive(source).unwrap();
    }
    fn window(&mut self) -> Health {
        self.take_health()
    }
}

#[test]
fn hand_topologies_publish_no_cap_hits() {
    let mut clipper = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::N4148,
        solver: PairSolver::default(),
    });
    clipper.prepare(RATE as f32);
    let clipper = std::cell::RefCell::new(clipper);
    hand(
        "diode clipper",
        |n| {
            clipper.borrow_mut().tick(hot(n, 4.0) as f32);
        },
        || clipper.borrow().stats(),
    );

    let opamp = std::cell::RefCell::new(OpAmpClipper::new(OpAmpClipperConfig {
        feedback_ohms: 551e3,
        ..OpAmpClipperConfig::TS808
    }));
    opamp.borrow_mut().prepare(RATE as f32);
    hand(
        "op-amp clipper",
        |n| {
            opamp.borrow_mut().tick_f64(hot(n, 1.0));
        },
        || opamp.borrow().stats(),
    );

    let cathode = std::cell::RefCell::new(CommonCathode::new(CommonCathodeConfig {
        tube: Dempwolf::RSD_1,
        source_ohms: 33e3,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    }));
    cathode.borrow_mut().prepare(RATE as f32);
    hand(
        "common cathode",
        |n| {
            cathode.borrow_mut().tick(hot(n, 8.0) as f32);
        },
        || cathode.borrow().stats(),
    );

    let supply = std::cell::RefCell::new(PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE));
    supply.borrow_mut().prepare(RATE as f32);
    hand(
        "power supply",
        |n| {
            let load = 0.05 + 0.04 * (std::f64::consts::TAU * 7.0 * n as f64 / RATE).sin();
            supply.borrow_mut().tick_f64(load);
        },
        || supply.borrow().stats(),
    );

    for (label, pentode) in [("push-pull feedback", false), ("EL34 push-pull feedback", true)] {
        let mut pair = stave_dsp::PushPull::new();
        if pentode {
            pair.set_pentode(stave_dsp::circuit::topologies::push_pull::PentodePair::EL34);
        }
        pair.prepare(RATE as f32);
        pair.set_drive(Db::new(if pentode { 36.0 } else { 18.0 }));
        pair.set_feedback(db(12.0));
        let pair = std::cell::RefCell::new(pair);
        hand(
            label,
            |n| {
                pair.borrow_mut().tick(hot(n, 1.0) as f32);
            },
            || pair.borrow().stats(),
        );
    }
}

/// A deck to drive: its name, netlist text, driven source and signal.
type Deck = (&'static str, String, &'static str, Box<dyn Fn(usize) -> f64>);

/// Every deck the solver conformance drives hot, with its source and
/// signal.
fn decks() -> Vec<Deck> {
    vec![
        (
            "diode clipper",
            deck("diode_clipper").replace("V1 in 0 SIN(0 1 1k)", "V1 in 0 0"),
            "v1",
            Box::new(|n| hot(n, 4.0)),
        ),
        (
            "op-amp clipper",
            deck("opamp_clipper")
                .replace("V1 in 0 DC 0 SIN(0 0.5 1k)", "V1 in 0 0")
                .replace("RF out fb 51k", "RF out fb 551k"),
            "v1",
            Box::new(|n| hot(n, 1.0)),
        ),
        (
            "common cathode",
            deck("common_cathode_grid_current").replace("V1 in 0 SIN(0 3 1k)", "V1 in 0 0"),
            "v1",
            Box::new(|n| hot(n, 8.0)),
        ),
        ("Tube Screamer", deck("tube_screamer"), "v1", Box::new(|n| 4.5 + hot(n, 1.0))),
        ("Tube Screamer, drive 0.9", deck("tube_screamer_drive_90"), "v1", Box::new(|n| 4.5 + hot(n, 1.0))),
        ("op-amp slew", deck("opamp_slew"), "v1", Box::new(|n| hot(n, 3.0))),
        ("OTA VCA", deck("ota_vca"), "vin", Box::new(|n| hot(n, 20.0))),
        ("EL34 push-pull", deck("push_pull_pentode"), "vx", Box::new(|n| hot(n, 36.0))),
        (
            "power supply",
            deck("power_supply"),
            "il",
            Box::new(|n| 0.05 + 0.04 * (std::f64::consts::TAU * 7.0 * n as f64 / RATE).sin()),
        ),
        ("fuzz face", deck("fuzz_face"), "vin", Box::new(|n| hot(n, 2.0))),
        ("rangemaster", deck("rangemaster"), "vin", Box::new(|n| hot(n, 3.0))),
        ("CE stage", deck("ce_stage"), "vin", Box::new(|n| hot(n, 2.0))),
        ("JFET booster", deck("jfet_booster"), "vin", Box::new(|n| hot(n, 4.0))),
        ("MOSFET booster", deck("mosfet_booster"), "vin", Box::new(|n| hot(n, 8.0))),
    ]
}

#[test]
fn every_deck_on_dk_publishes_no_cap_hits() {
    for (name, text, source, input) in decks() {
        let netlist = Netlist::parse_spice(&text).unwrap();
        circuit::<Dk>(&format!("{name} (DK)"), &netlist, source, input);
    }
}

#[test]
fn every_deck_on_wdf_publishes_no_cap_hits() {
    for (name, text, source, input) in decks() {
        let netlist = Netlist::parse_spice(&text).unwrap();
        circuit::<Wdf>(&format!("{name} (WDF)"), &netlist, source, input);
    }
}
