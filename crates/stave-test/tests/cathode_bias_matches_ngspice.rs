//! Memory in tube stages against ngspice (DSP.md R15): the bias a loud
//! passage leaves behind, read where it lives. Each `spice/*_burst.cir`
//! drives a deck with a 50 ms burst at 200 Hz (from 20 ms) and then
//! silence, and probes the memory node: the cathode of the cathode-biased
//! triode stage and of the cathode-biased push-pull pair, and the grid
//! side of the coupling cap that blocks (`grid_blocking`).
//!
//! Each is held two ways. Its waveform, the bias building under the burst
//! and recovering after, by normalised RMS; and its 1/e recovery time —
//! from the end of the burst, how long the bias shift (the probe's mean
//! over each 5 ms cycle, from its value at rest) takes to fall to 1/e
//! of its size in the first silent cycle — as a characteristic of its
//! own, relative to ngspice's. `Circuit<Dk>` runs every deck; the
//! tabulated `Triode` and `PushPull` run theirs under `Bias::Cathode`,
//! where their one-pole cathode against the circuit's is the finding.
//! The R17 matrix holds the same decks' plate outputs, with the burst and
//! its recovery at T0 (`characterise.rs`).

#[allow(dead_code)]
mod circuits;

use std::path::PathBuf;

use circuits::{PUSH_PULL_CATHODE_FARADS, TRIODE_CATHODE_FARADS};
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist};
use stave_dsp::circuit::topologies::triode_stage::{Bias, LevelPolicy};
use stave_dsp::circuit::Circuit;
use stave_dsp::{PushPull, Stage, Triode};
use stave_measure::suite::analysis::Recovery;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The burst's cycle: 200 Hz.
const CYCLE: f64 = 5e-3;
/// The burst: from half an output step past 20 ms to half a step past
/// 70 ms (the decks' note says why).
const START: f64 = 20e-3 + 10.4167e-6;
const END: f64 = 70e-3 + 10.4167e-6;

/// The deck's input at `t`: the sum of its sources between the input
/// node and ground (the burst's two series sines).
fn stimulus(deck: &Netlist) -> impl Fn(f64) -> f64 {
    let sources: Vec<_> = deck
        .elements()
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::VoltageSource { source, .. } if source.signal.is_some() => Some(source.clone()),
            _ => None,
        })
        .collect();
    move |t| sources.iter().map(|s| s.value_at(t)).sum()
}

/// The 1/e recovery time of `run` (on the reference's 48 kHz instants):
/// the mean of each cycle from the burst's end, against the mean of the
/// cycle before it began.
fn recovery(run: &[f64], abscissa: &[f64]) -> f64 {
    let mean = |from: f64| {
        let inside: Vec<f64> = abscissa
            .iter()
            .zip(run)
            .filter(|(t, _)| **t >= from - 1e-9 && **t < from + CYCLE - 1e-9)
            .map(|(_, v)| *v)
            .collect();
        inside.iter().sum::<f64>() / inside.len() as f64
    };
    let base = mean(START - CYCLE);
    let cycles = ((abscissa[abscissa.len() - 1] - END) / CYCLE).floor() as usize;
    let r = Recovery {
        dc: (0..cycles).map(|j| mean(END + j as f64 * CYCLE)).collect(),
        probe_db: vec![0.0; cycles],
    };
    r.time_constant(base, 0, CYCLE, 0.0).unwrap()
}

/// `Circuit<Dk>` on the deck at `oversample` × 48 kHz on the deck's own
/// stimulus, read at the reference's instants.
fn circuit_run(name: &str, oversample: u32) -> Vec<f64> {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut c = Circuit::<Dk>::new(&deck(name)).unwrap();
    c.prepare(rate as f32);
    let (mut n, mut out) = (0u64, c.output());
    let run = reference
        .abscissa()
        .iter()
        .map(|&t| {
            while n < (t * rate).round() as u64 {
                out = c.step(0.0);
                n += 1;
            }
            out
        })
        .collect();
    assert_eq!(c.health().newton.capped, 0, "{name}: a solve hit its cap");
    run
}

/// A hand stage's memory node at `oversample` × 48 kHz on the deck's
/// stimulus: `tick` takes the input and returns the node's volts.
fn hand_run(name: &str, oversample: u32, mut tick: impl FnMut(f64) -> f64) -> Vec<f64> {
    let reference = Reference::ngspice(name);
    let input = stimulus(&deck(name));
    let rate = 48_000.0 * f64::from(oversample);
    let (mut n, mut out) = (0u64, tick(0.0));
    reference
        .abscissa()
        .iter()
        .map(|&t| {
            while n < (t * rate).round() as u64 {
                n += 1;
                out = tick(input(n as f64 / rate));
            }
            out
        })
        .collect()
}

/// The run against the reference's waveform, and its recovery time
/// against the reference's within `tau` (relative).
fn judge(name: &str, label: &str, run: &[f64], budget: f64, tau: f64) -> Outcome {
    let reference = Reference::ngspice(name);
    let (tr, tm) = (recovery(reference.values(), reference.abscissa()), recovery(run, reference.abscissa()));
    let e = (tm / tr - 1.0).abs();
    println!("{name}, {label}: 1/e recovery {:.3} ms vs ngspice's {:.3} ms ({e:.2e} relative, budget {tau:e})", tm * 1e3, tr * 1e3);
    assert!(e <= tau, "{name}, {label}: recovery {:.3} ms vs {:.3} ms, {e:.3e} relative over {tau:e}", tm * 1e3, tr * 1e3);
    Case::ngspice(name)
        .named(format!("{name}, {label}"))
        .budget(Metric::NormalizedRms, budget)
        .compare(run)
}

/// The decks on `Circuit<Dk>`: the bias trajectory at 4× and 8×, its
/// error falling with the step, and the recovery time. Budgets: the 8×
/// error three times over, rounded up to 1-2-5 (measured 4× → 8×: the
/// triode's cathode 7.07e-8 → 1.83e-8, recovery 1.6e-9 relative; the
/// pair's 1.54e-7 → 3.28e-8, 2.1e-8; the coupling cap's grid side
/// 3.09e-7 → 1.61e-7, 2.5e-9). The blocking deck halves rather than
/// quarters: its worst points are the grid's conduction onsets on the
/// burst's first peaks (2.6e-6 → 2.0e-6 V), where ngspice's own 1 µs
/// step is the error left.
#[test]
fn bursts_on_circuit_dk_match_ngspice() {
    for (name, budget, tau, ratio) in [
        ("triode_stage_cathode_burst", 1e-7, 1e-8, 3.0),
        ("push_pull_cathode_burst", 1e-7, 5e-8, 3.0),
        ("grid_blocking_burst", 5e-7, 1e-8, 1.8),
    ] {
        let coarse = judge(name, "dk 4x", &circuit_run(name, 4), budget, tau);
        let fine = judge(name, "dk 8x", &circuit_run(name, 8), budget, tau);
        println!("{coarse}\n{fine}");
        assert_converges(&coarse, &fine, ratio);
        fine.assert();
    }
}

/// The tabulated `Triode` under `Bias::Cathode`: its cathode (`vk0` plus
/// its drift) against the circuit's. Both rest at 1.500 V (the stage's Rk
/// from the plate's load line seen from the cathode); measured 8.48e-5
/// normalised RMS (the worst 0.43 mV, at the burst's end) and a 1/e
/// recovery of 17.84 ms against 17.75 (+0.53%): the one-pole cathode
/// holds the circuit's memory to within a percent. Before the bias was
/// corrected the stage rested 8 mV hot (5.4e-3). Budgets three times
/// over, rounded up.
#[test]
fn triode_cathode_bias_matches_ngspice() {
    let name = "triode_stage_cathode_burst";
    let mut t = Triode::new();
    t.set_level_policy(LevelPolicy::Physical { reference_drive: 1.0 });
    t.set_grid_drive(1.0);
    t.set_bias(Bias::Cathode { bypass_farads: TRIODE_CATHODE_FARADS });
    t.prepare(4.0 * 48_000.0);
    let rest = -t.frame_params().0;
    let run = hand_run(name, 4, |x| {
        t.tick(x as f32);
        rest + t.cathode_shift()
    });
    judge(name, "Triode, Bias::Cathode, 4x", &run, 5e-4, 2e-2).assert();
}

/// `PushPull` under `Bias::Cathode`: its shared cathode against the
/// circuit's. Both rest at 3.500 V (the pair's Rk from the plates' load
/// lines seen from the cathode; 3.458 V in the circuit before); measured
/// 4.58e-3 normalised RMS (the worst 51 mV, 8.82 V against 8.77 V at the
/// burst's end) and a 1/e recovery of 19.04 ms against 18.93 (+0.58%).
/// Budgets three times over, rounded up.
#[test]
fn push_pull_cathode_bias_matches_ngspice() {
    let name = "push_pull_cathode_burst";
    let mut p = PushPull::new();
    p.set_bias(Bias::Cathode { bypass_farads: PUSH_PULL_CATHODE_FARADS });
    p.prepare(4.0 * 48_000.0);
    let run = hand_run(name, 4, |x| {
        p.tick(x as f32);
        3.5 + p.cathode_shift()
    });
    judge(name, "PushPull, Bias::Cathode, 4x", &run, 2e-2, 2e-2).assert();
}
