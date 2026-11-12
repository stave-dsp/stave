//! The no-alloc check: the full process loop under the guard while
//! every param sweeps — including MODE (a program switch wipes the
//! core mid-block), DIVE (the clock ratio moves, so the bridge runs
//! 0..n core ticks per frame) and SYNC.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_saga_vst3::Saga>(
        "saga",
        &[
            "output", "mode", "feedback", "regen", "mod", "mix", "x", "time", "tone", "width",
            "mod rate", "mod shape", "shift", "ramp", "dive", "sync",
        ],
        &NoAllocConfig::default(),
    );
}
