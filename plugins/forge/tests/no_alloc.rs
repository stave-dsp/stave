//! The no-alloc check (colocated): the full process loop under an
//! allocation-panicking guard while params sweep per block.
//! Continuous AND structural axes: stack sweeps all four topologies
//! (Curve↔RC↔Bypass kind switches); character/edge/bias force full
//! curve-table rebuilds every block.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_forge_vst3::Forge>(
        "forge",
        &[
            "drive",
            "tone",
            "depth",
            "grit",
            "iron",
            "sub mix",
            "sub mod",
            "width",
            "mix",
            "edge",
            "character",
            "stack",
            "bias",
        ],
        &NoAllocConfig::default(),
    );
}
