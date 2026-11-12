//! The VST3 event and parameter-change paths, steered by coverage: the
//! simulated host (stave-host `plugin::sim`) reads every choice — call
//! order, block sizes, buffer shapes, parameter points, state bytes —
//! from the fuzzer's tape, and checks every law the seeded sweep does
//! against Tilt, hosted in process through the full ABI.
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_host::plugin::sim::{simulate_tape, SimConfig};

fuzz_target!(|tape: &[u8]| {
    let config = SimConfig {
        steps: (tape.len() / 8).clamp(1, 64),
        illegal_per_16: 4,
    };
    if let Err(failure) = simulate_tape(stave_host::entry_points!(stave_tilt_vst3), tape, config) {
        panic!("{failure}");
    }
});
