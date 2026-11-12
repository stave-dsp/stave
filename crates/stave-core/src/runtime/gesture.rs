//! Face edits on their way to a CLAP host (`out_events`).

use core::sync::atomic::{AtomicU32, Ordering};

use crate::params::ParamId;

/// A GUI-side parameter gesture event, en route to the host.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GestureEvent {
    Begin {
        id: ParamId,
    },
    /// `value` in the param's own units.
    Value {
        id: ParamId,
        value: f64,
    },
    End {
        id: ParamId,
    },
}

const GESTURE_CAP: usize = 64;

/// Most gestures one face holds open at once (a pointer holds one).
const OPEN_CAP: usize = 8;

/// Bounded SPSC ring for UI→audio gesture delivery (CLAP emits param
/// changes through `out_events` on the audio thread; VST3 calls its
/// handler directly from the UI thread and doesn't need this).
///
/// Producer: UI thread (`push`). Consumer: audio thread (`pop`).
///
/// Admission keeps every gesture bracketed at the host however full the
/// ring gets (the protocol is model-checked in `stave_model::gesture`):
/// each open gesture reserves a slot for its `End`, so an `End` always
/// fits; a `Begin` needs room for itself and its `End` beyond the open
/// gestures' reserve, a `Value` room beyond the reserve; the `End` (and
/// any repeat `Begin`) of a gesture whose `Begin` was refused is refused
/// too. A refused event is counted — never silent; a 64-deep ring only
/// fills when audio is wedged.
pub struct GestureQueue {
    kinds: [core::sync::atomic::AtomicU8; GESTURE_CAP],
    ids: [AtomicU32; GESTURE_CAP],
    values: [core::sync::atomic::AtomicU64; GESTURE_CAP],
    head: AtomicU32, // consumer position
    tail: AtomicU32, // producer position
    dropped: AtomicU32,
    /// The ids whose `Begin` was admitted and `End` not yet: the
    /// producer's own bookkeeping (only `push` touches it).
    open: [AtomicU32; OPEN_CAP],
    open_len: AtomicU32,
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
            open: [const { AtomicU32::new(0) }; OPEN_CAP],
            open_len: AtomicU32::new(0),
        }
    }

    /// Where `id` sits in the open list.
    fn open_at(&self, id: u32) -> Option<usize> {
        let n = self.open_len.load(Ordering::Relaxed) as usize;
        (0..n).find(|&i| self.open[i].load(Ordering::Relaxed) == id)
    }

    /// UI-thread producer. Returns false (and counts) when the event is
    /// refused (see the type's admission rule).
    pub fn push(&self, ev: GestureEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let free = GESTURE_CAP.saturating_sub(tail.wrapping_sub(head) as usize);
        let open = self.open_len.load(Ordering::Relaxed) as usize;
        let admit = match ev {
            GestureEvent::Begin { id } => {
                open < OPEN_CAP && free >= open + 2 && self.open_at(id.0).is_none()
            }
            GestureEvent::Value { .. } => free > open,
            // The reserve: free >= open >= 1 whenever `id` is open.
            GestureEvent::End { id } => self.open_at(id.0).is_some() && free > 0,
        };
        if !admit {
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
        match ev {
            GestureEvent::Begin { id } => {
                self.open[open].store(id.0, Ordering::Relaxed);
                self.open_len.store(open as u32 + 1, Ordering::Relaxed);
            }
            GestureEvent::End { id } => {
                if let Some(i) = self.open_at(id.0) {
                    let last = self.open[open - 1].load(Ordering::Relaxed);
                    self.open[i].store(last, Ordering::Relaxed);
                    self.open_len.store(open as u32 - 1, Ordering::Relaxed);
                }
            }
            GestureEvent::Value { .. } => {}
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(q: &GestureQueue) -> Vec<GestureEvent> {
        core::iter::from_fn(|| q.pop()).collect()
    }

    /// The host-side bracket the model (stave_model::gesture) checks:
    /// a long drag into a wedged audio thread fills the ring, Values
    /// are refused past the reserve, and the release still lands.
    #[test]
    fn a_drag_that_fills_the_ring_still_ends_its_gesture() {
        let q = GestureQueue::new();
        let id = ParamId(3);
        assert!(q.push(GestureEvent::Begin { id }));
        let mut admitted = 0;
        for i in 0..200 {
            admitted += usize::from(q.push(GestureEvent::Value { id, value: f64::from(i) }));
        }
        assert_eq!(admitted, GESTURE_CAP - 2, "one slot for the Begin, one kept for the End");
        assert!(q.push(GestureEvent::End { id }));
        assert_eq!(q.dropped() as usize, 200 - admitted);
        let events = drain(&q);
        assert_eq!(events.first(), Some(&GestureEvent::Begin { id }));
        assert_eq!(events.last(), Some(&GestureEvent::End { id }));
    }

    #[test]
    fn a_refused_begin_refuses_its_end() {
        let q = GestureQueue::new();
        for i in 0..GESTURE_CAP - 1 {
            assert!(q.push(GestureEvent::Value { id: ParamId(9), value: i as f64 }));
        }
        let id = ParamId(4);
        assert!(!q.push(GestureEvent::Begin { id }), "no room for Begin and End");
        drain(&q);
        assert!(!q.push(GestureEvent::End { id }), "the host never saw it begin");
        assert!(q.push(GestureEvent::Begin { id }));
        assert!(!q.push(GestureEvent::Begin { id }), "begun once");
        assert!(q.push(GestureEvent::End { id }));
        assert_eq!(drain(&q), [GestureEvent::Begin { id }, GestureEvent::End { id }]);
    }
}

/// Kani: the ring is FIFO and bounded at every position of its `u32`
/// counters, including across their wrap, and its admission rule keeps
/// every admitted gesture's `End`.
#[cfg(kani)]
mod proofs {
    use super::*;

    /// Bit-exact equality (a NaN value is delivered as the same NaN).
    fn same(a: GestureEvent, b: GestureEvent) -> bool {
        match (a, b) {
            (GestureEvent::Begin { id: x }, GestureEvent::Begin { id: y })
            | (GestureEvent::End { id: x }, GestureEvent::End { id: y }) => x == y,
            (GestureEvent::Value { id: x, value: v }, GestureEvent::Value { id: y, value: w }) => {
                x == y && v.to_bits() == w.to_bits()
            }
            _ => false,
        }
    }

    /// From any starting count (the wrap included), any interleaving of
    /// three pushes and pops delivers exactly what was pushed, in order,
    /// and never holds more than its capacity.
    #[kani::proof]
    #[kani::unwind(5)]
    fn delivers_in_order_across_the_counter_wrap() {
        let q = GestureQueue::new();
        let start: u32 = kani::any();
        q.head.store(start, Ordering::Relaxed);
        q.tail.store(start, Ordering::Relaxed);
        let mut pushed = [GestureEvent::Begin { id: ParamId(0) }; 3];
        let (mut n_pushed, mut n_popped) = (0usize, 0usize);
        for _ in 0..4 {
            if kani::any() && n_pushed < 3 {
                let ev = any_event_but_begin_end();
                assert!(q.push(ev), "never full below capacity");
                pushed[n_pushed] = ev;
                n_pushed += 1;
            } else {
                match q.pop() {
                    Some(ev) => {
                        assert!(n_popped < n_pushed);
                        assert!(same(ev, pushed[n_popped]), "FIFO, bit-exact");
                        n_popped += 1;
                    }
                    None => assert_eq!(n_popped, n_pushed, "empty only when drained"),
                }
            }
            let held = q.tail.load(Ordering::Relaxed).wrapping_sub(q.head.load(Ordering::Relaxed));
            assert_eq!(held as usize, n_pushed - n_popped);
        }
        assert_eq!(q.dropped(), 0);
    }

    /// A full ring refuses a new gesture, counts it, and keeps the oldest
    /// event at the head — at counter positions up to and across the
    /// `u32` wrap (a fully symbolic start blows the solver up on the
    /// 64-slot arrays; the FIFO harness covers every start).
    #[kani::proof]
    #[kani::unwind(10)]
    fn a_full_ring_refuses_a_new_gesture_and_counts_it() {
        let q = GestureQueue::new();
        let start: u32 = u32::MAX - u32::from(kani::any::<u8>());
        q.head.store(start, Ordering::Relaxed);
        q.tail.store(start.wrapping_add(GESTURE_CAP as u32), Ordering::Relaxed);
        let slot = start as usize % GESTURE_CAP;
        q.kinds[slot].store(1, Ordering::Relaxed);
        q.ids[slot].store(7, Ordering::Relaxed);
        assert!(!q.push(GestureEvent::Begin { id: ParamId(kani::any()) }));
        assert_eq!(q.dropped(), 1);
        assert!(same(q.pop().unwrap(), GestureEvent::Begin { id: ParamId(7) }));
        assert!(!q.push(GestureEvent::Begin { id: ParamId(kani::any()) }), "needs room for its End too");
        assert!(q.push(any_event_but_begin_end()), "a Value fits in the freed slot");
    }

    fn any_event_but_begin_end() -> GestureEvent {
        GestureEvent::Value {
            id: ParamId(kani::any()),
            value: f64::from_bits(kani::any()),
        }
    }

    /// At any fill level and counter position: once a gesture's `Begin`
    /// is admitted, its `End` is admitted, whatever Values were pushed in
    /// between (the reserve) — the host never sees a gesture left open.
    #[kani::proof]
    #[kani::unwind(6)]
    fn an_admitted_begin_always_gets_its_end() {
        let q = GestureQueue::new();
        let start: u32 = kani::any();
        let held: u32 = kani::any_where(|h: &u32| *h as usize <= GESTURE_CAP);
        q.head.store(start, Ordering::Relaxed);
        q.tail.store(start.wrapping_add(held), Ordering::Relaxed);
        let id = ParamId(kani::any());
        if !q.push(GestureEvent::Begin { id }) {
            // Refused: so is its End, and the host never hears of it.
            assert!(!q.push(GestureEvent::End { id }));
            return;
        }
        for _ in 0..4 {
            if kani::any() {
                q.push(any_event_but_begin_end());
            }
        }
        assert!(q.push(GestureEvent::End { id }), "the reserved slot");
        assert!(!q.push(GestureEvent::End { id }), "ended once");
    }
}
