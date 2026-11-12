//! The no-alloc check: the full process loop under the guard while
//! the tone knob sweeps (it rebuilds coefficients — the hard case).

// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_tilt_vst3::Tilt>("tilt", &["Tilt", "Output"], &NoAllocConfig::default());
}
