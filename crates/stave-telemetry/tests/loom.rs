//! Loom models of the telemetry registration protocol.
//!
//! These model the PROTOCOL (the atomic choreography), not the production
//! structs — constructing a full `Telemetry` under loom would register
//! thousands of tracked atomics. The invariants proven here are exactly
//! the ones tap registration (`slot.rs`: `reserve`, `find_tap`) and
//! `InlineStr` rely on; keep the shapes in sync with
//! `stave-telemetry/src/slot.rs`. Miri runs the REAL types under
//! concurrency (`concurrent_scan_during_registration`); loom exhausts the
//! interleavings of the protocol itself.
//!
//! Run with: RUSTFLAGS="--cfg loom" cargo test -p stave-telemetry --test loom --release
#![cfg(loom)]

use loom::cell::UnsafeCell;
use loom::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use loom::sync::Arc;
use loom::thread;

/// Model of `InlineStr`: bytes guarded by a Release-published ready flag.
struct SlotName {
    ready: AtomicBool,
    bytes: UnsafeCell<[u8; 4]>,
}

impl SlotName {
    fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            bytes: UnsafeCell::new([0; 4]),
        }
    }

    /// Writer side of `InlineStr::set`.
    fn set(&self, v: [u8; 4]) {
        self.bytes.with_mut(|p| unsafe { *p = v });
        self.ready.store(true, Ordering::Release);
    }

    /// Reader side of `InlineStr::as_str`: all-or-nothing.
    fn get(&self) -> Option<[u8; 4]> {
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        Some(self.bytes.with(|p| unsafe { *p }))
    }
}

unsafe impl Sync for SlotName {}
unsafe impl Send for SlotName {}

/// Check: a reader concurrent with slot publication sees either nothing or
/// the complete name — never torn bytes. This is the InlineStr soundness
/// claim under every interleaving loom can produce.
#[test]
fn published_name_is_all_or_nothing() {
    loom::model(|| {
        let slot = Arc::new(SlotName::new());
        let writer = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || slot.set(*b"gain"))
        };
        match slot.get() {
            None => {}
            Some(v) => assert_eq!(&v, b"gain", "torn read"),
        }
        writer.join().unwrap();
        assert_eq!(slot.get(), Some(*b"gain"));
    });
}

/// Model of the `resolve_*` reservation: CAS-bump a used counter to claim
/// a slot index exclusively, then publish the slot.
///
/// Check: two concurrent registrars never double-reserve — they get
/// distinct indices, the counter ends at 2, and both slots end published
/// with their own payloads.
#[test]
fn concurrent_reservation_never_double_claims() {
    loom::model(|| {
        let used = Arc::new(AtomicU16::new(0));
        let slots = Arc::new([SlotName::new(), SlotName::new()]);

        let reserve = |used: &AtomicU16| -> Option<u16> {
            loop {
                let cur = used.load(Ordering::Relaxed);
                if cur >= 2 {
                    return None;
                }
                if used
                    .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
                {
                    return Some(cur);
                }
            }
        };

        let t = {
            let used = Arc::clone(&used);
            let slots = Arc::clone(&slots);
            thread::spawn(move || {
                let idx = reserve(&used).expect("slot available");
                slots[idx as usize].set(*b"aaaa");
                idx
            })
        };
        let idx_main = reserve(&used).expect("slot available");
        slots[idx_main as usize].set(*b"bbbb");
        let idx_thread = t.join().unwrap();

        assert_ne!(idx_main, idx_thread, "double-reserved slot");
        assert_eq!(used.load(Ordering::Acquire), 2);
        assert_eq!(slots[idx_main as usize].get(), Some(*b"bbbb"));
        assert_eq!(slots[idx_thread as usize].get(), Some(*b"aaaa"));
    });
}

/// Model of one `Messages` slot: claim by CAS to an odd sequence, write
/// the words, publish the message's even sequence; a reader keeps a read
/// only when the sequence was that message's before and after.
struct MessageSlot {
    sequence: loom::sync::atomic::AtomicU64,
    words: [loom::sync::atomic::AtomicU64; 2],
}

impl MessageSlot {
    fn push(&self, index: u64, value: u64) -> bool {
        let current = self.sequence.load(Ordering::Relaxed);
        if current % 2 == 1
            || self
                .sequence
                .compare_exchange(current, current | 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            return false;
        }
        loom::sync::atomic::fence(Ordering::Release);
        for word in &self.words {
            word.store(value, Ordering::Relaxed);
        }
        self.sequence.store(2 * (index + 1), Ordering::Release);
        true
    }

    fn read(&self, index: u64) -> Option<[u64; 2]> {
        let before = self.sequence.load(Ordering::Acquire);
        if before != 2 * (index + 1) {
            return None;
        }
        let words = [0, 1].map(|i| self.words[i].load(Ordering::Relaxed));
        loom::sync::atomic::fence(Ordering::Acquire);
        (self.sequence.load(Ordering::Relaxed) == before).then_some(words)
    }
}

#[test]
fn a_message_slot_is_never_read_torn() {
    loom::model(|| {
        let slot = Arc::new(MessageSlot {
            sequence: loom::sync::atomic::AtomicU64::new(0),
            words: [
                loom::sync::atomic::AtomicU64::new(0),
                loom::sync::atomic::AtomicU64::new(0),
            ],
        });
        let writer = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || slot.push(1, 22))
        };
        slot.push(0, 11);
        for index in [0, 1] {
            if let Some(words) = slot.read(index) {
                assert_eq!(words[0], words[1], "torn read");
                assert_eq!(words[0], [11, 22][index as usize]);
            }
        }
        writer.join().unwrap();
    });
}

/// Model of a `TapSlot` being reserved for a `TapId`: the id is stored
/// (Relaxed) before the name is published (Release); a reader matching by
/// id (`find_tap`) first observes the name (Acquire).
struct IdSlot {
    id: AtomicU64,
    name: SlotName,
    latest: AtomicU64,
}

impl IdSlot {
    fn reserve(&self, id: u64, name: [u8; 4]) {
        self.latest.store(0, Ordering::Relaxed);
        self.id.store(id, Ordering::Relaxed);
        self.name.set(name);
    }

    /// `find_tap`'s match: published, and holding `id`.
    fn holds(&self, id: u64) -> bool {
        self.name.get().is_some() && self.id.load(Ordering::Relaxed) == id
    }
}

/// Check: a reader that sees a slot's name published also sees its id —
/// a face resolving a `TapId` while the engine registers never matches a
/// half-reserved slot, and once the name is visible the id matches.
#[test]
fn a_published_tap_slot_carries_its_id() {
    loom::model(|| {
        let slot = Arc::new(IdSlot {
            id: AtomicU64::new(0),
            name: SlotName::new(),
            latest: AtomicU64::new(0),
        });
        let engine = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || {
                slot.reserve(0xfeed, *b"spin");
                // `Tap::set` after registration: a relaxed store.
                slot.latest.store(42, Ordering::Relaxed);
            })
        };
        if slot.name.get().is_some() {
            assert!(slot.holds(0xfeed), "name visible but id not");
        }
        engine.join().unwrap();
        assert!(slot.holds(0xfeed));
        assert_eq!(slot.latest.load(Ordering::Relaxed), 42);
    });
}

/// Check: two threads registering different taps (the engine's `Taps::new`
/// and a face reserving a slot to draw before audio) get distinct slots,
/// each found by its own id.
#[test]
fn concurrent_tap_registration_finds_each_id() {
    loom::model(|| {
        let used = Arc::new(AtomicU16::new(0));
        let slots = Arc::new([0, 1].map(|_| IdSlot {
            id: AtomicU64::new(0),
            name: SlotName::new(),
            latest: AtomicU64::new(0),
        }));
        let reserve = |used: &AtomicU16| loop {
            let cur = used.load(Ordering::Relaxed);
            if used
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return cur as usize;
            }
        };
        let t = {
            let (used, slots) = (Arc::clone(&used), Arc::clone(&slots));
            thread::spawn(move || slots[reserve(&used)].reserve(1, *b"spin"))
        };
        slots[reserve(&used)].reserve(2, *b"levl");
        t.join().unwrap();
        let n = used.load(Ordering::Acquire) as usize;
        assert_eq!(n, 2);
        for id in [1, 2] {
            assert_eq!(slots[..n].iter().filter(|s| s.holds(id)).count(), 1);
        }
    });
}
