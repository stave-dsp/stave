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

/// Loom model of the convolver workers' hand-off ring
/// (`stave_core::workers::handoff`, the shipping type): the audio thread
/// swaps three blocks into a two-slot ring while a worker drains it.
/// Every block arrives once, whole and in order, the sender never writes
/// a slot the receiver holds (loom checks every cell access), and every
/// value the sender gets back is one it can reuse (a fresh slot or a
/// released block).
#[test]
fn handoff_delivers_in_order_without_tearing() {
    use stave_core::workers::handoff;
    loom::model(|| {
        // A block: two words that must never be seen half-written.
        let (mut tx, mut rx) = handoff(vec![[0u32; 2]; 2]);
        let worker = thread::spawn(move || {
            let mut seen = Vec::new();
            while seen.len() < 3 {
                if let Some(block) = rx.peek(|b| *b) {
                    assert_eq!(block[0], block[1], "torn block");
                    seen.push(block[0]);
                    rx.release();
                } else {
                    thread::yield_now();
                }
            }
            seen
        });
        let mut returned = Vec::new();
        for v in 1..=3u32 {
            let mut block = [v, v];
            while !tx.send_swap(&mut block) {
                thread::yield_now();
            }
            returned.push(block[0]);
        }
        assert_eq!(worker.join().unwrap(), [1, 2, 3]);
        // The first two sends got the fresh slots; the third a released
        // block (the first).
        assert_eq!(returned, [0, 0, 1]);
    });
}
