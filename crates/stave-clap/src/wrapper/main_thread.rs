//! [`MainThreadCell`]: interior mutability for state only [main-thread]
//! calls touch (the open face).

use core::cell::{Ref, RefCell, RefMut};
use std::sync::OnceLock;
use std::thread::{self, ThreadId};

use stave_core::diagnostics::CallError;

/// A `RefCell` for state that only [main-thread] host calls touch (clap
/// gui.h), with two guards a bare `RefCell` lacks:
///
/// - a re-entrant call (the host calling back into `clap.gui` from inside
///   another gui call, say while the view attaches to its parent) gets a
///   [`CallError`] to refuse with, not a `BorrowMutError` panic that
///   disables the whole instance (or aborts, outside the panic guard);
/// - debug builds assert every access is on the thread that made the
///   first one, so a host calling gui functions off the main thread is
///   caught where it happens.
pub(crate) struct MainThreadCell<T> {
    /// The thread of the first access: the host's main thread.
    owner: OnceLock<ThreadId>,
    cell: RefCell<T>,
}

impl<T> MainThreadCell<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            owner: OnceLock::new(),
            cell: RefCell::new(value),
        }
    }

    fn check_thread(&self) {
        if cfg!(debug_assertions) {
            let me = thread::current().id();
            let owner = *self.owner.get_or_init(|| me);
            debug_assert_eq!(owner, me, "a [main-thread] clap.gui call arrived on another thread");
        }
    }

    /// Shared access, or a refusal while another call holds it mutably.
    pub(crate) fn try_borrow(&self) -> Result<Ref<'_, T>, CallError> {
        self.check_thread();
        self.cell.try_borrow().map_err(|_| busy())
    }

    /// Exclusive access, or a refusal while another call holds it.
    pub(crate) fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, CallError> {
        self.check_thread();
        self.cell.try_borrow_mut().map_err(|_| busy())
    }
}

fn busy() -> CallError {
    CallError::WindowRefused("another gui call is still running (a re-entrant host call)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reentrant_borrow_is_refused_not_a_panic() {
        let cell = MainThreadCell::new(0u32);
        let held = cell.try_borrow_mut().unwrap();
        assert!(cell.try_borrow().is_err());
        assert!(cell.try_borrow_mut().is_err());
        drop(held);
        let shared = cell.try_borrow().unwrap();
        assert!(cell.try_borrow().is_ok());
        assert!(cell.try_borrow_mut().is_err());
        drop(shared);
        *cell.try_borrow_mut().unwrap() = 1;
        assert_eq!(*cell.try_borrow().unwrap(), 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn an_access_off_the_first_thread_asserts() {
        let cell = std::sync::Arc::new(std::sync::Mutex::new(MainThreadCell::new(0u32)));
        let _ = cell.lock().unwrap().try_borrow();
        let other = std::sync::Arc::clone(&cell);
        let off_thread = thread::spawn(move || {
            let _ = other.lock().unwrap().try_borrow();
        })
        .join();
        assert!(off_thread.is_err(), "the off-thread access did not assert");
    }
}
