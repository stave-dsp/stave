//! The no-alloc check under the NOTE STORM while the whole voicing
//! surface sweeps (27 knobs — every block re-applies 4 envelopes ×
//! 16 voices: the apply path itself is under the guard).

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_opal_vst3::Opal>(
        "opal",
        &[
            "algo",
            "feedback",
            "vel mod",
            "ratio 1",
            "level 1",
            "attack 1",
            "decay 1",
            "sustain 1",
            "release 1",
            "ratio 2",
            "level 2",
            "attack 2",
            "decay 2",
            "sustain 2",
            "release 2",
            "ratio 3",
            "level 3",
            "attack 3",
            "decay 3",
            "sustain 3",
            "release 3",
            "ratio 4",
            "level 4",
            "attack 4",
            "decay 4",
            "sustain 4",
            "release 4",
            "lfo rate",
            "src 1",
            "tgt 1",
            "depth 1",
            "src 2",
            "tgt 2",
            "depth 2",
            "output",
        ],
        &NoAllocConfig::default(),
    );
}
