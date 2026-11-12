//! The no-alloc check under the NOTE STORM (the check churns notes past
//! the pool size for instruments — the steal path runs under the
//! guard) while the envelope + shape sweep rebuilds voicing.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_chime_vst3::Chime>(
        "chime",
        &[
            "attack", "decay", "sustain", "release", "shape", "output", "lfo rate", "src 1",
            "tgt 1", "depth 1", "src 2", "tgt 2", "depth 2",
        ],
        &NoAllocConfig::default(),
    );
}
