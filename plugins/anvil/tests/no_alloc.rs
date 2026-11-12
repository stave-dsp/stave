//! The no-alloc check: the full process loop under the guard while
//! params sweep. Grow the wiggle list with every param that
//! rebuilds tables or switches structure — not just the easy trims.

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static ALLOCATION_GUARD: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_anvil_vst3::Anvil>(
        "anvil",
        &[
            "output", "gain", "tight", "bass", "mid", "treble", "presence", "depth", "focus",
            "squash", "sub", "edge",
        ],
        &NoAllocConfig::default(),
    );
}
