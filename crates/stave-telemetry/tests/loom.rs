//! Loom models of the telemetry protocols, run on the shipping types:
//! under `cfg(loom)` the crate builds [`InlineStr`], [`TapSlot`],
//! [`reserve_index`] and the [`Message`] slot on loom's atomics and cell
//! (`src/sync.rs`), and `stave_telemetry::model` exposes them. A whole
//! `Telemetry` is not built here — it is thousands of atomics — but every
//! piece of it that two threads race on is, exactly as it ships.
//!
//! Run with: RUSTFLAGS="--cfg loom" cargo test -p stave-telemetry --test loom --release
#![cfg(loom)]

use loom::sync::atomic::{AtomicU16, Ordering};
use loom::sync::Arc;
use loom::thread;
use stave_telemetry::model::{reserve_index, InlineStr, Message, TapSlot};

type Name = InlineStr<5>;

/// Check: a reader concurrent with slot publication sees either nothing
/// or the complete name — never torn bytes. The `InlineStr` soundness
/// claim, under every interleaving loom can produce.
#[test]
fn published_name_is_all_or_nothing() {
    loom::model(|| {
        let slot = Arc::new(Name::zeroed());
        let writer = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || slot.set("gain"))
        };
        match slot.as_str() {
            "" => {}
            seen => assert_eq!(seen, "gain", "torn read"),
        }
        writer.join().unwrap();
        assert_eq!(slot.as_str(), "gain");
    });
}

/// Check: two concurrent registrars never double-reserve — they get
/// distinct indices, the counter ends at 2, and both slots end published
/// with their own names.
#[test]
fn concurrent_reservation_never_double_claims() {
    loom::model(|| {
        let used = Arc::new(AtomicU16::new(0));
        let slots = Arc::new([Name::zeroed(), Name::zeroed()]);
        let t = {
            let (used, slots) = (Arc::clone(&used), Arc::clone(&slots));
            thread::spawn(move || {
                let i = reserve_index(&used, 2).expect("slot available");
                slots[i as usize].set("aaaa");
                i
            })
        };
        let mine = reserve_index(&used, 2).expect("slot available");
        slots[mine as usize].set("bbbb");
        let theirs = t.join().unwrap();

        assert_ne!(mine, theirs, "double-reserved slot");
        assert_eq!(used.load(Ordering::Acquire), 2);
        assert_eq!(reserve_index(&used, 2), None, "full");
        assert_eq!(slots[mine as usize].as_str(), "bbbb");
        assert_eq!(slots[theirs as usize].as_str(), "aaaa");
    });
}

/// Check: a message slot written by two threads at once is never read
/// torn — a reader keeps a read only when it is wholly one message.
#[test]
fn a_message_slot_is_never_read_torn() {
    loom::model(|| {
        let slot = Arc::new(Message::zeroed());
        let writer = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || slot.write(1, 2, "twenty-two"))
        };
        slot.write(0, 1, "eleven");
        for (index, want) in [(0, (1, "eleven")), (1, (2, "twenty-two"))] {
            if let Some((severity, text)) = slot.read(index) {
                assert_eq!((severity, text.as_str()), want, "torn read");
            }
        }
        writer.join().unwrap();
    });
}

/// Check: a reader that sees a tap slot's name published also sees its
/// id — a face resolving a `TapId` while the engine registers never
/// matches a half-reserved slot, and once the name is visible the id
/// matches.
#[test]
fn a_published_tap_slot_carries_its_id() {
    loom::model(|| {
        let slot = Arc::new(TapSlot::zeroed());
        let engine = {
            let slot = Arc::clone(&slot);
            thread::spawn(move || {
                slot.claim("spin", 0xfeed);
                // `Tap::set` after registration.
                slot.record(42.0);
            })
        };
        if slot.holds(0xfeed) {
            // Matched by id: the claim (and its reset) came first.
            let latest = slot.latest();
            assert!(latest == 0.0 || latest == 42.0, "{latest}");
        }
        engine.join().unwrap();
        assert!(slot.holds(0xfeed));
        assert_eq!(slot.latest(), 42.0);
    });
}

/// Check: two threads registering different taps (the engine's
/// `Taps::new` and a face reserving a slot to draw before audio) get
/// distinct slots, each found by its own id.
#[test]
fn concurrent_tap_registration_finds_each_id() {
    loom::model(|| {
        let used = Arc::new(AtomicU16::new(0));
        let slots = Arc::new([TapSlot::zeroed(), TapSlot::zeroed()]);
        let t = {
            let (used, slots) = (Arc::clone(&used), Arc::clone(&slots));
            thread::spawn(move || {
                let i = reserve_index(&used, 2).expect("slot available");
                slots[i as usize].claim("spin", 1);
            })
        };
        let i = reserve_index(&used, 2).expect("slot available");
        slots[i as usize].claim("levl", 2);
        t.join().unwrap();
        let n = used.load(Ordering::Acquire) as usize;
        assert_eq!(n, 2);
        for id in [1, 2] {
            assert_eq!(slots[..n].iter().filter(|s| s.holds(id)).count(), 1);
        }
    });
}
