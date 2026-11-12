//! Loom model of the `PluginCell` claim-token protocol.
//!
//! Models the atomic choreography of `stave_core::wrapper::PluginCell`
//! (CAS-claim → exclusive access → Release on drop) rather than the
//! generic production type; keep in sync with `wrapper.rs`. The invariant:
//! at most one holder at a time, and a successor observes every write
//! the predecessor made before release.
//!
//! Run with: RUSTFLAGS="--cfg loom" cargo test -p stave-core --test loom --release
#![cfg(loom)]

use loom::cell::UnsafeCell;
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::Arc;
use loom::thread;

struct Cell {
    claimed: AtomicBool,
    state: UnsafeCell<u64>,
}

unsafe impl Sync for Cell {}
unsafe impl Send for Cell {}

impl Cell {
    fn new() -> Self {
        Self {
            claimed: AtomicBool::new(false),
            state: UnsafeCell::new(0),
        }
    }

    /// `PluginCell::claim` shape.
    fn claim(&self) -> bool {
        self.claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// `ProcessorGuard::drop` shape.
    fn release(&self) {
        self.claimed.store(false, Ordering::Release);
    }
}

/// Check: audio thread (process) and main thread (setActive) racing for
/// the processor — never both inside, and whoever enters second sees the
/// first's writes (no torn/stale state across the handoff).
#[test]
fn claim_is_mutually_exclusive_and_ordered() {
    loom::model(|| {
        let cell = Arc::new(Cell::new());

        let t = {
            let cell = Arc::clone(&cell);
            thread::spawn(move || {
                if cell.claim() {
                    // "process": read-modify-write, must never interleave.
                    cell.state.with_mut(|p| unsafe {
                        let v = *p;
                        *p = v + 1;
                    });
                    cell.release();
                    true
                } else {
                    false
                }
            })
        };

        let main_won = if cell.claim() {
            cell.state.with_mut(|p| unsafe {
                let v = *p;
                *p = v + 1;
            });
            cell.release();
            true
        } else {
            false
        };
        let thread_won = t.join().unwrap();

        // Every claimer that got in incremented exactly once; a refused
        // claimer simply didn't run (the wrapper returns an error code).
        let expected = u64::from(main_won) + u64::from(thread_won);
        // Re-claim from this thread: must succeed now and observe the
        // full history (Acquire pairs with both Releases).
        assert!(cell.claim(), "cell free after all guards dropped");
        cell.state.with(|p| unsafe {
            assert_eq!(*p, expected, "lost or duplicated increment");
        });
        cell.release();
    });
}
