//! The no-alloc check: the full process loop under the guard while
//! the trim sweeps.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_eddy_vst3::Eddy>(
        "eddy",
        &[
            "output",
            "time",
            "feedback",
            "dry",
            "wet",
            "rate",
            "depth",
            "motion",
            "spread",
            "flavor",
            "algo",
            "circuit",
            "heat",
            "sag",
            "inertia",
            "shift",
            "grain",
            "splice",
            "orbit",
            "diffusion",
            "diffuse",
            "density",
            "wander",
            "bend",
            "smear",
            "spray",
            "motes",
            "size",
            "weave",
            "hold",
            "bleed",
        ],
        &NoAllocConfig::default(),
    );
}
