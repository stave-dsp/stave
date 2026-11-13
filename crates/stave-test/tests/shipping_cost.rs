//! The shipping-cost report (DSP.md R12, P4): what each characterised
//! circuit costs as it ships (`circuits::SHIPPING`) — `Circuit<Dk>` on
//! its deck inside `Oversampled::linear` at its default factor, the
//! half-band pairs included — in ns per host sample at 48 kHz, and that
//! as a fraction of the real-time budget (20.8 µs a sample; above 1 a
//! core cannot keep up). The stimulus is the datasheet's: LCG noise of
//! ±0.3 around the deck's operating point (±5 mA on the supply's load),
//! run through `Stage::process` in 256-sample host blocks; the fastest
//! of seven runs of a quarter second of audio each, after one to warm
//! (the least disturbed by anything else the machine runs).
//! Ignored by default; measure on the reference machine, alone:
//!
//! ```text
//! cargo test --release -p stave-test --test shipping_cost -- --ignored --nocapture
//! ```

#[allow(dead_code)]
mod circuits;

use std::time::Instant;

use circuits::{shipped_circuit, CIRCUITS, SHIPPING};
use stave_dsp::{Oversampled, Stage};
use stave_test::characterise::deck;

const HOST: f64 = 48_000.0;
const BLOCK: usize = 256;
/// A quarter second of host audio a run.
const RUN: usize = 12_288;

/// LCG noise in ±`amplitude` around `dc`.
fn noise(len: usize, dc: f64, amplitude: f64) -> Vec<f32> {
    let mut lcg: u32 = 7;
    (0..len)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            (dc + amplitude * ((lcg >> 8) as f64 / f64::from(1u32 << 23) - 1.0)) as f32
        })
        .collect()
}

#[test]
#[ignore = "R12/P4 report: run on demand, on the reference machine"]
fn shipping_cost_report() {
    let only = std::env::var("STAVE_SHIPPING_ONLY").ok();
    println!("| circuit | shipping | ns per host sample | real-time load at 48 kHz |\n|---|---|---|---|");
    for (name, s) in SHIPPING.iter().filter(|(n, _)| only.as_deref().is_none_or(|o| o.split(',').any(|x| x == *n))) {
        let circuit = CIRCUITS.iter().find(|c| c.name == *name).expect("a shipping circuit is characterised");
        let input = (circuit.input)();
        let amplitude = if circuit.supply { 0.005 } else { 0.3 };
        let c = shipped_circuit(&deck(name), &input, HOST, *s);
        let mut os = Oversampled::linear(c, s.oversample);
        os.prepare(HOST as f32);
        let x = noise(RUN, input.dc, amplitude);
        let mut buf = vec![0.0f32; BLOCK];
        let mut run = || {
            let t0 = Instant::now();
            for block in x.chunks_exact(BLOCK) {
                buf.copy_from_slice(block);
                os.process(std::hint::black_box(&mut buf));
            }
            t0.elapsed().as_secs_f64() * 1e9 / RUN as f64
        };
        run();
        let ns = (0..7).map(|_| run()).fold(f64::INFINITY, f64::min);
        println!(
            "| {name} | {}× | {ns:.0} | {:.2} |",
            s.oversample,
            ns / (1e9 / HOST)
        );
    }
}
