//! The linear decks in both domains (DSP.md R3): the tone stacks, the
//! Big Muff's tone stack, the bridged-T and the CE-2's BBD filters were
//! held only to ngspice's AC analysis, the coupler only to its
//! transient. Each `<deck>_tran.cir` is the AC deck from rest under
//! 0.5 V at 220 Hz plus 0.5 V at 3.1 kHz, so the start-up and the steady
//! state read against ngspice's transient; `coupler_ac.cir` is the
//! coupler's response from 1 Hz. `Circuit<Dk>` on each deck, its error
//! falling near the trapezoidal rule's h² from 4× to 8×.

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::Circuit;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The deck at `oversample` × 48 kHz on its own stimulus, read at the
/// reference's instants.
fn run(name: &str, oversample: u32, budget: f64) -> Outcome {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut c = Circuit::<Dk>::new(&deck(name)).unwrap();
    c.prepare(rate as f32);
    let (mut n, mut out) = (0u64, c.output());
    let run: Vec<f64> = reference
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
    Case::ngspice(name)
        .named(format!("{name}/dk {oversample}x"))
        .budget(Metric::NormalizedRms, budget)
        .compare(&run)
}

/// Each linear deck's transient: the 8× error three times over, rounded
/// up to 1-2-5 (measured 4× → 8×: 5F6-A 2.66e-4 → 7.55e-5, AB763
/// 4.56e-4 → 1.20e-4, 2203 1.33e-4 → 4.55e-5, Duncan mid 4.07e-4 →
/// 1.10e-4, bright 4.35e-4 → 1.16e-4, dark 3.20e-4 → 8.92e-5, Big Muff
/// 2.46e-4 → 7.12e-5, bridged-T 4.80e-4 → 1.15e-4, BBD input 6.28e-4 →
/// 1.41e-4, output 7.89e-4 → 1.77e-4). The worst point of each is the
/// first millisecond, where the 3.1 kHz sine starts at full slope.
const TRANSIENTS: [(&str, f64); 10] = [
    ("tone_stack_fender_5f6a_tran", 5e-4),
    ("tone_stack_fender_ab763_tran", 5e-4),
    ("tone_stack_marshall_2203_tran", 2e-4),
    ("tone_stack_duncan_fender_mid_tran", 5e-4),
    ("tone_stack_duncan_fender_bright_tran", 5e-4),
    ("tone_stack_duncan_fender_dark_tran", 5e-4),
    ("big_muff_tone_stack_tran", 5e-4),
    ("bridged_t_tran", 5e-4),
    ("bbd_input_filter_tran", 5e-4),
    ("bbd_output_filter_tran", 1e-3),
];

#[test]
fn linear_transients_match_ngspice() {
    let runs: Vec<(Outcome, Outcome)> = TRANSIENTS
        .iter()
        .map(|&(name, budget)| (run(name, 4, budget), run(name, 8, budget)))
        .collect();
    for (coarse, fine) in &runs {
        println!("{coarse}\n{fine}\n  ratio {:.2}", coarse.error / fine.error);
    }
    // At least 2.5× as the step halves (h² gives 4; the 2203's start-up
    // reads 2.93, its 470 pF treble cap's onset nearer the reference's
    // own 1 µs step).
    for (coarse, fine) in runs {
        assert_converges(&coarse, &fine, 2.5);
        fine.assert();
    }
}

/// The interstage coupler's small-signal response from 1 Hz (its corner
/// 7.234 Hz) against ngspice's AC analysis: `Circuit<Dk>`'s analog
/// response, exact for a linear circuit.
#[test]
fn coupler_response_matches_ngspice() {
    let circuit = Circuit::<Dk>::new(&deck("coupler_ac")).unwrap();
    Case::ngspice("coupler_ac")
        .named("coupler_ac/dk")
        .budget(Metric::MaxDb, 2e-11) // 4.76e-12 dB
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
}
