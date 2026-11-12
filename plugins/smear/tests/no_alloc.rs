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
    check_no_alloc::<stave_smear_vst3::Smear>(
        "smear",
        &[
            "seed", "smear", "bloom", "decay", "flux", "shimmer", "drive", "motion", "depth",
            "tone", "mix",
        ],
        &NoAllocConfig::default(),
    );
}
