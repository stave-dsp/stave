//! [`handoff()`]: a single-producer, single-consumer ring of preallocated
//! values, exchanged by swapping — lock-free, wait-free and
//! allocation-free after construction. The loom model (`tests/loom.rs`)
//! runs this very type: under `cfg(loom)` its atomics, cells and `Arc`
//! are loom's.

#[cfg(not(loom))]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(not(loom))]
use std::sync::Arc;

#[cfg(loom)]
use loom::sync::atomic::{AtomicUsize, Ordering};
#[cfg(loom)]
use loom::sync::Arc;

#[cfg(not(loom))]
type Cell<T> = core::cell::UnsafeCell<T>;
#[cfg(loom)]
type Cell<T> = loom::cell::UnsafeCell<T>;

/// Run `f` on the cell's value.
///
/// # Safety
/// No other access to the value may overlap this one.
#[cfg(not(loom))]
unsafe fn with_mut<T, R>(cell: &Cell<T>, f: impl FnOnce(&mut T) -> R) -> R {
    // SAFETY: exclusive, per this function's contract.
    f(unsafe { &mut *cell.get() })
}

/// Run `f` on the cell's value (loom checks the access).
///
/// # Safety
/// No other access to the value may overlap this one.
#[cfg(loom)]
unsafe fn with_mut<T, R>(cell: &Cell<T>, f: impl FnOnce(&mut T) -> R) -> R {
    // SAFETY: exclusive, per this function's contract.
    cell.with_mut(|p| f(unsafe { &mut *p }))
}

struct Inner<T> {
    slots: Box<[Cell<T>]>,
    /// Values published: written by the sender only.
    head: AtomicUsize,
    /// Values released: written by the receiver only.
    tail: AtomicUsize,
}

// SAFETY: a slot is reached by exactly one side at a time — the sender
// between seeing it free (`tail` Acquire) and publishing it (`head`
// Release), the receiver between seeing it published (`head` Acquire)
// and releasing it (`tail` Release) — so the cells are never shared;
// the values cross threads, so they must be `Send`.
unsafe impl<T: Send> Sync for Inner<T> {}
// SAFETY: as above; the ring owns its values.
unsafe impl<T: Send> Send for Inner<T> {}

/// A ring of `slots.len()` values (each preallocated, typically a
/// buffer) between one sending and one receiving thread. The sender
/// swaps its value into a free slot and gets the slot's old one back;
/// the receiver works on the oldest published value in place, then
/// releases it. No call allocates, locks or waits.
///
/// # Panics
///
/// If `slots` is empty.
pub fn handoff<T: Send>(slots: Vec<T>) -> (Sender<T>, Receiver<T>) {
    assert!(!slots.is_empty(), "a handoff needs at least one slot");
    let inner = Arc::new(Inner {
        slots: slots.into_iter().map(Cell::new).collect(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        Sender {
            inner: Arc::clone(&inner),
            head: 0,
        },
        Receiver { inner, tail: 0 },
    )
}

/// The sending side of a [`handoff()`].
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
    /// Our count of values published (the shared `head`'s only writer).
    head: usize,
}

impl<T> Sender<T> {
    /// Swap `value` into the next free slot and publish it: `value`
    /// comes back holding the slot's previous content. `false`, with
    /// `value` untouched, when every slot is in flight.
    pub fn send_swap(&mut self, value: &mut T) -> bool {
        let tail = self.inner.tail.load(Ordering::Acquire);
        let capacity = self.inner.slots.len();
        if self.head.wrapping_sub(tail) == capacity {
            return false;
        }
        let slot = &self.inner.slots[self.head % capacity];
        // SAFETY: the slot is free — the receiver released it (`tail`,
        // Acquire above) and will not touch it until `head` publishes it.
        unsafe { with_mut(slot, |v| core::mem::swap(v, value)) };
        self.head = self.head.wrapping_add(1);
        self.inner.head.store(self.head, Ordering::Release);
        true
    }

    /// Values sent and not yet released by the receiver.
    pub fn in_flight(&self) -> usize {
        self.head
            .wrapping_sub(self.inner.tail.load(Ordering::Acquire))
    }
}

/// The receiving side of a [`handoff()`].
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
    /// Our count of values released (the shared `tail`'s only writer).
    tail: usize,
}

impl<T> Receiver<T> {
    /// Run `f` on the oldest published value, in place, without
    /// releasing it; `None` when nothing is published.
    pub fn peek<R>(&mut self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let head = self.inner.head.load(Ordering::Acquire);
        if head == self.tail {
            return None;
        }
        let slot = &self.inner.slots[self.tail % self.inner.slots.len()];
        // SAFETY: the slot is published (`head`, Acquire above) and stays
        // ours until `release` advances `tail`.
        Some(unsafe { with_mut(slot, f) })
    }

    /// Release the oldest published value back to the sender (no-op
    /// when nothing is published).
    pub fn release(&mut self) {
        if self.inner.head.load(Ordering::Acquire) != self.tail {
            self.tail = self.tail.wrapping_add(1);
            self.inner.tail.store(self.tail, Ordering::Release);
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn values_arrive_in_order_and_slots_come_back() {
        let (mut tx, mut rx) = handoff(vec![0u32; 2]);
        for v in 1..=2 {
            let mut x = v;
            assert!(tx.send_swap(&mut x));
            assert_eq!(x, 0, "a fresh slot's value comes back");
        }
        let mut x = 3;
        assert!(!tx.send_swap(&mut x), "full");
        assert_eq!(x, 3, "untouched when full");
        assert_eq!(tx.in_flight(), 2);
        assert_eq!(rx.peek(|v| *v), Some(1));
        rx.release();
        assert!(tx.send_swap(&mut x));
        assert_eq!(x, 1, "the released slot's value comes back");
        assert_eq!(rx.peek(|v| *v), Some(2));
        rx.release();
        assert_eq!(rx.peek(|v| *v), Some(3));
        rx.release();
        assert_eq!(rx.peek(|v| *v), None);
        assert_eq!(tx.in_flight(), 0);
    }
}
