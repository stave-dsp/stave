//! The no-alloc check: the full process loop under the guard while
//! every param sweeps — including MACHINE (a switch wipes the core
//! mid-block), HOLD (the clock ratio moves, so the bridge runs 0..n
//! core ticks per frame), MEDIA/WARP (the wear and wobble re-voice)
//! and SYNC.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_heirloom_vst3::Heirloom>(
        "heirloom",
        &[
            "output", "time", "repeats", "mix", "wobble", "rpm", "age", "machine", "warp",
            "media", "hold", "sync",
        ],
        &NoAllocConfig::default(),
    );
}
