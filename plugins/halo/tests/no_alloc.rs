// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static ALLOCATION_GUARD: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_halo_vst3::Halo>(
        "halo",
        &[
            "output", "gain", "bass", "mid", "treble", "headroom", "bloom", "reserve", "response",
        ],
        &NoAllocConfig::default(),
    );
}
