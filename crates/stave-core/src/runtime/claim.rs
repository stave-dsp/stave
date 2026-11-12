//! [`ClaimCell`]: a value one thread at a time may hold, claimed by an
//! atomic token. The processor's [`PluginCell`](super::PluginCell) is
//! one; the loom model (`tests/loom.rs`) runs this very type, because
//! under `cfg(loom)` its token and cell are loom's.

use core::ops::{Deref, DerefMut};

#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(loom))]
type Cell<T> = core::cell::UnsafeCell<T>;
#[cfg(loom)]
type Cell<T> = loom::cell::UnsafeCell<T>;

/// A value behind a claim token: [`try_claim`](Self::try_claim) hands
/// out at most one [`ClaimGuard`] at a time, and the Acquire claim pairs
/// with the Release on the guard's drop, so each holder sees every write
/// its predecessors made.
pub struct ClaimCell<T> {
    value: Cell<T>,
    claimed: AtomicBool,
}

// SAFETY: the value is only reached through a `ClaimGuard`, and the token
// lets one exist at a time (the compare-exchange in `try_claim`), with
// Acquire/Release ordering one holder's writes before the next's reads.
// The value moves between threads with the holder, so it must be `Send`.
unsafe impl<T: Send> Sync for ClaimCell<T> {}

impl<T> ClaimCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: Cell::new(value),
            claimed: AtomicBool::new(false),
        }
    }

    /// The value, exclusively, until the guard drops; `None` while
    /// another thread holds it.
    pub fn try_claim(&self) -> Option<ClaimGuard<'_, T>> {
        self.claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        Some(ClaimGuard {
            cell: self,
            #[cfg(loom)]
            access: Some(self.value.get_mut()),
        })
    }
}

/// Exclusive access to a [`ClaimCell`]'s value; released on drop.
pub struct ClaimGuard<'a, T> {
    cell: &'a ClaimCell<T>,
    /// Loom's record of this access, held while the guard lives so the
    /// model checks it against every other.
    #[cfg(loom)]
    access: Option<loom::cell::MutPtr<T>>,
}

impl<T> ClaimGuard<'_, T> {
    /// Where the value is; valid while the guard lives.
    #[cfg(not(loom))]
    fn value(&self) -> *mut T {
        self.cell.value.get()
    }

    /// Where the value is; valid while the guard lives (and `access`
    /// with it).
    #[cfg(loom)]
    fn value(&self) -> *mut T {
        let access = self.access.as_ref().expect("held until drop");
        // SAFETY: the claim token makes this guard the value's sole
        // accessor, and loom checks exactly that.
        core::ptr::from_mut(unsafe { access.deref() })
    }
}

impl<T> Deref for ClaimGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the claim token makes this guard the value's sole
        // accessor for its lifetime.
        unsafe { &*self.value() }
    }
}

impl<T> DerefMut for ClaimGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`, and `&mut self` makes this the only
        // borrow through the guard.
        unsafe { &mut *self.value() }
    }
}

impl<T> Drop for ClaimGuard<'_, T> {
    fn drop(&mut self) {
        // The access ends before the token is released.
        #[cfg(loom)]
        drop(self.access.take());
        self.cell.claimed.store(false, Ordering::Release);
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time_and_writes_carry_over() {
        let cell = ClaimCell::new(1u32);
        let mut first = cell.try_claim().expect("free");
        assert!(cell.try_claim().is_none(), "held");
        *first += 1;
        drop(first);
        assert_eq!(*cell.try_claim().expect("released"), 2);
    }
}
