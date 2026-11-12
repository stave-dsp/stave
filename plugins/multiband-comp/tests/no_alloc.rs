//! The no-alloc check (colocated): the full process loop under an
//! allocation-panicking guard while params sweep per block.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_multiband_comp_vst3::MultibandComp>(
        "multiband",
        &[
            "lo threshold",
            "mid ratio",
            "hi makeup",
            "lo drive",
            "mid sat mix",
            "lo attack",
            "hi release",
            "mid knee",
            "low/mid",
            "hi mix",
            "lo style",
            "mid solo",
        ],
        &NoAllocConfig::default(),
    );
}
