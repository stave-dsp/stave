//! Face edits on their way to a CLAP host (`out_events`).

use core::sync::atomic::{AtomicU32, Ordering};

use crate::params::ParamId;

/// A GUI-side parameter gesture event, en route to the host.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GestureEvent {
    Begin { id: ParamId },
    /// `value` in the param's own units.
    Value { id: ParamId, value: f64 },
    End { id: ParamId },
}

const GESTURE_CAP: usize = 64;

/// Bounded SPSC ring for UI→audio gesture delivery (CLAP emits param
/// changes through `out_events` on the audio thread; VST3 calls its
/// handler directly from the UI thread and doesn't need this).
///
/// Producer: UI thread (`push`). Consumer: audio thread (`pop`).
/// Overflow drops the NEWEST event and counts it — never silent;
/// a 64-deep ring only overflows if audio is wedged.
pub struct GestureQueue {
    kinds: [core::sync::atomic::AtomicU8; GESTURE_CAP],
    ids: [AtomicU32; GESTURE_CAP],
    values: [core::sync::atomic::AtomicU64; GESTURE_CAP],
    head: AtomicU32, // consumer position
    tail: AtomicU32, // producer position
    dropped: AtomicU32,
}

impl Default for GestureQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureQueue {
    pub fn new() -> Self {
        Self {
            kinds: [const { core::sync::atomic::AtomicU8::new(0) }; GESTURE_CAP],
            ids: [const { AtomicU32::new(0) }; GESTURE_CAP],
            values: [const { core::sync::atomic::AtomicU64::new(0) }; GESTURE_CAP],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// UI-thread producer. Returns false (and counts) on overflow.
    pub fn push(&self, ev: GestureEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) as usize >= GESTURE_CAP {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = tail as usize % GESTURE_CAP;
        let (kind, id, bits) = match ev {
            GestureEvent::Begin { id } => (1u8, id.0, 0u64),
            GestureEvent::Value { id, value } => (2, id.0, value.to_bits()),
            GestureEvent::End { id } => (3, id.0, 0),
        };
        self.kinds[slot].store(kind, Ordering::Relaxed);
        self.ids[slot].store(id, Ordering::Relaxed);
        self.values[slot].store(bits, Ordering::Relaxed);
        // Publish AFTER the payload: the consumer gates on tail.
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Audio-thread consumer.
    pub fn pop(&self) -> Option<GestureEvent> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = head as usize % GESTURE_CAP;
        let kind = self.kinds[slot].load(Ordering::Relaxed);
        let id = ParamId(self.ids[slot].load(Ordering::Relaxed));
        let bits = self.values[slot].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(match kind {
            1 => GestureEvent::Begin { id },
            3 => GestureEvent::End { id },
            _ => GestureEvent::Value {
                id,
                value: f64::from_bits(bits),
            },
        })
    }

    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}
