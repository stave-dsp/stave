//! Loom model of the processor's claim token, run on the shipping type:
//! under `cfg(loom)` [`ClaimCell`]'s token and cell are loom's, so the
//! model checks `stave_core::runtime::ClaimCell` itself — the cell every
//! `PluginCell` is. The invariant: at most one holder at a time, and a
//! successor observes every write the predecessor made before release.
//!
//! Run with: RUSTFLAGS="--cfg loom" cargo test -p stave-core --test loom --release
#![cfg(loom)]

use loom::sync::Arc;
use loom::thread;
use stave_core::runtime::ClaimCell;

/// "process": a read-modify-write that must never interleave with
/// another holder's.
fn bump(cell: &ClaimCell<u64>) -> bool {
    match cell.try_claim() {
        Some(mut held) => {
            let v = *held;
            *held = v + 1;
            true
        }
        None => false,
    }
}

/// Check: the audio thread (process) and the main thread (setActive)
/// racing for the processor — never both inside, and whoever enters
/// second sees the first's writes (no torn or stale state across the
/// handoff).
#[test]
fn claim_is_mutually_exclusive_and_ordered() {
    loom::model(|| {
        let cell = Arc::new(ClaimCell::new(0u64));
        let t = {
            let cell = Arc::clone(&cell);
            thread::spawn(move || bump(&cell))
        };
        let main_won = bump(&cell);
        let thread_won = t.join().unwrap();

        // Every claimer that got in incremented exactly once; a refused
        // claimer simply didn't run (the wrapper returns an error code).
        let expected = u64::from(main_won) + u64::from(thread_won);
        // Re-claim from this thread: must succeed now and observe the
        // full history (Acquire pairs with both Releases).
        let held = cell.try_claim().expect("cell free after all guards dropped");
        assert_eq!(*held, expected, "lost or duplicated increment");
    });
}
