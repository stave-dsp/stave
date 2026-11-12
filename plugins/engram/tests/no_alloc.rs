//! The no-alloc check: the full process loop under the guard while
//! params sweep. Grow the wiggle list with every param that
//! rebuilds tables or switches structure — not just the easy trims.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<stave_engram_vst3::Engram>(
        "engram",
        &["output", "size", "decay", "tone", "recall", "focus", "deja", "match", "texture", "forget", "plasticity", "mix", "hold", "space", "space x", "space y", "steer", "drift"],
        &NoAllocConfig::default(),
    );
}
