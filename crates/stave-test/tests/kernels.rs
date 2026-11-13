//! The nonlinear kernels as an analysis (DSP.md M6, C12) held to the
//! characterisation's own harmonic measurements (R17): the harmonic
//! levels the kernels read at a sweep's level against the coherent tones
//! of the same level.
//!
//! - **The model's own table.** `Circuit<Dk>` at 48 kHz, swept at −20 dB
//!   re its knee, against its fingerprint's 48 kHz table (the 1 kHz tone
//!   at −20 dB): the same model, rate and level, so the difference is the
//!   method's — sweep against steady state, the separation's windows.
//! - **ngspice.** `Circuit<Dk>` at 4× 48 kHz against ngspice's tones at
//!   100 Hz, 330 Hz, 1 kHz and 3 kHz, −20 dB re knee: the method's error
//!   plus the solver's.
//!
//! Harmonics below [`FLOOR_DBC`] in the reference are not compared (the
//! diode clipper's H2 is its deck's asymmetry, near the floor). The
//! Volterra diagonals' weak-signal reading is printed beside them: where
//! it parts from the measurement, the circuit is not weakly nonlinear at
//! −20 dB re knee (the clippers). Budgets are the measured errors with a
//! margin of two to three (a floor of 0.02 dB). Each test runs on
//! libtest's own thread.
//!
//! Measured (H2, H3 above −100 dBc): against its own 48 kHz table at
//! −20 and 0 dB re knee, the common cathode 0.020 dB, the diode clipper
//! and the Tube Screamer stage 0.005 dB or less; against ngspice at −20
//! and −3 dB re knee from 100 Hz to 3 kHz, 0.078, 0.044 and 0.053 dB.
//! FINDING: the diode clipper is not weakly nonlinear at −3 dB re knee —
//! the Volterra diagonals from −20 and −3 dB under-read its H3 there by
//! 13–15 dB, where the common cathode's agree to 0.1 dB.

use stave_measure::suite::analysis::Harmonics;
use stave_test::characterise::kernels::{measure, KernelPlan, KernelSet};
use stave_test::characterise::plan::{Kind, HARMONICS};
use stave_test::characterise::{deck, Characterise, Model, Plan, Table, Target};

/// Harmonics this far below the fundamental in the reference are not compared.
const FLOOR_DBC: f64 = -100.0;

/// The kernels of `name`'s deck on `Circuit<Dk>` at `rate`, swept at
/// `levels` dB re knee.
fn kernels(name: &str, rate: f64, levels: &[f64]) -> (Plan, KernelSet) {
    let plan = Characterise::ngspice(name).plan().clone();
    // From 5 Hz (kernels from 50 Hz; the lowest tone's H2 is at 200 Hz):
    // a quarter of the default's length, which matters at 4x.
    let k = KernelPlan {
        levels_db: levels.to_vec(),
        rate,
        low_hz: 5.0,
        seconds: 1.5,
        ..KernelPlan::default()
    };
    let set = measure(&Model::Circuit(deck(name)), &plan, &k).expect("kernels");
    (plan, set)
}

/// The tone `id`'s fundamental (Hz) and H2, H3 (dBc) in `table`.
fn tone(plan: &Plan, table: &Table, id: &str) -> (f64, [Option<f64>; 2]) {
    let run = plan.run(id).unwrap_or_else(|| panic!("no run {id}"));
    let Kind::Tone { bin, .. } = run.kind else { panic!("{id} is not a tone") };
    let m = table.runs.get(id).unwrap_or_else(|| panic!("{id} not in the table"));
    let h = Harmonics::of(&m.periods[0], bin, HARMONICS);
    (run.period().hz(bin), [h.dbc(2), h.dbc(3)])
}

/// The worst |kernels − reference| over H2, H3 of each tone, dB, the
/// kernels from the sweep at `level` dB re knee; prints the comparison.
fn compare(what: &str, plan: &Plan, set: &KernelSet, level: f64, reference: &Table, tones: &[&str]) -> f64 {
    let k = set.at(level).unwrap();
    let v = set.volterra.as_ref();
    let amplitude = k.amplitude();
    let mut worst = 0.0f64;
    for id in tones {
        let (f0, refs) = tone(plan, reference, id);
        for (i, r) in refs.iter().enumerate() {
            let n = i + 2;
            let got = k.harmonic_dbc(n, f0).unwrap();
            let weak = v.and_then(|v| v.harmonic_dbc(n, f0, amplitude));
            let Some(r) = r.filter(|&r| r > FLOOR_DBC) else {
                println!("  {what} {id} H{n}: kernels {got:.2} dBc, reference below {FLOOR_DBC} dBc (not compared)");
                continue;
            };
            println!(
                "  {what} {id} H{n}: kernels {got:.2} dBc, reference {r:.2} ({:+.3} dB); Volterra weak-signal {}",
                got - r,
                weak.map_or("—".into(), |w| format!("{w:.2}"))
            );
            worst = worst.max((got - r).abs());
        }
    }
    worst
}

/// `name` held to its own 48 kHz table and to ngspice: `(own, ngspice)`
/// budgets in dB.
fn agree(name: &str, own_db: f64, ngspice_db: f64) {
    println!("{name}:");
    let (plan, set) = kernels(name, 48_000.0, &[-20.0, 0.0]);
    let own = Target::fingerprint(name).load().unwrap();
    let e_own = compare("48 kHz model", &plan, &set, -20.0, &own, &["tone/1k/-20"])
        .max(compare("48 kHz model", &plan, &set, 0.0, &own, &["tone/1k/+0"]));
    let (plan, set) = kernels(name, 192_000.0, &[-20.0, -3.0]);
    let spice = Target::ngspice(name).load().unwrap();
    let e_spice = compare("4x vs ngspice", &plan, &set, -20.0, &spice, &["tone/100/-20", "tone/330/-20", "tone/1k/-20", "tone/3k/-20"])
        .max(compare("4x vs ngspice", &plan, &set, -3.0, &spice, &["tone/100/-3", "tone/330/-3", "tone/1k/-3", "tone/3k/-3"]));
    println!("  worst: {e_own:.3} dB against its own table (budget {own_db}), {e_spice:.3} dB against ngspice (budget {ngspice_db})");
    assert!(e_own <= own_db && e_spice <= ngspice_db, "{name}: kernels and tones disagree");
}

#[test]
fn common_cathode() {
    agree("common_cathode", 0.06, 0.25);
}

#[test]
fn diode_clipper() {
    agree("diode_clipper", 0.02, 0.15);
}

#[test]
fn tube_screamer() {
    agree("tube_screamer", 0.02, 0.15);
}
