//! The atomics, fence and cell telemetry's lock-free protocols are built
//! on: core's, or loom's under `cfg(loom)`, so the loom models
//! (`tests/loom.rs`) run the shipping types — [`InlineStr`], the slot
//! reservation, [`TapSlot`], a [`Message`] slot — not copies of them.
//!
//! Loom's atomics have no `const fn new`, so the constructors that are
//! `const` (a block is kilobytes of them, built in place) are written
//! once through [`zeroed!`] and arrays of them through [`zeroed_array!`].
//!
//! [`InlineStr`]: crate::inline_str::InlineStr
//! [`TapSlot`]: crate::slot::TapSlot
//! [`Message`]: crate::messages::Message

#[cfg(not(loom))]
pub(crate) use core::sync::atomic::{fence, AtomicBool, AtomicU16, AtomicU64, AtomicU8, Ordering};
#[cfg(loom)]
pub(crate) use loom::sync::atomic::{fence, AtomicBool, AtomicU16, AtomicU64, AtomicU8, Ordering};

/// Interior mutability for bytes one writer fills before publishing
/// them: core's `UnsafeCell`, or loom's (which checks every access).
pub(crate) struct UnsafeCell<T>(
    #[cfg(not(loom))] core::cell::UnsafeCell<T>,
    #[cfg(loom)] loom::cell::UnsafeCell<T>,
);

impl<T> UnsafeCell<T> {
    #[cfg(not(loom))]
    pub(crate) const fn new(value: T) -> Self {
        Self(core::cell::UnsafeCell::new(value))
    }

    #[cfg(loom)]
    pub(crate) fn new(value: T) -> Self {
        Self(loom::cell::UnsafeCell::new(value))
    }

    /// Write through `&self`.
    ///
    /// # Safety
    /// No other access to the value may overlap this one.
    #[cfg(not(loom))]
    pub(crate) unsafe fn write(&self, f: impl FnOnce(&mut T)) {
        // SAFETY: exclusive, per this function's contract.
        f(unsafe { &mut *self.0.get() });
    }

    /// Write through `&self`.
    ///
    /// # Safety
    /// No other access to the value may overlap this one.
    #[cfg(loom)]
    pub(crate) unsafe fn write(&self, f: impl FnOnce(&mut T)) {
        // SAFETY: as above; loom checks it.
        self.0.with_mut(|p| f(unsafe { &mut *p }));
    }

    /// The value, shared.
    ///
    /// # Safety
    /// No write may overlap the returned borrow.
    #[cfg(not(loom))]
    pub(crate) unsafe fn read(&self) -> &T {
        // SAFETY: no overlapping write, per this function's contract.
        unsafe { &*self.0.get() }
    }

    /// The value, shared.
    ///
    /// # Safety
    /// No write may overlap the returned borrow.
    #[cfg(loom)]
    pub(crate) unsafe fn read(&self) -> &T {
        // SAFETY: as above; loom checks the access as it is made.
        unsafe { &*self.0.with(|p| p) }
    }
}

/// `$vis const fn zeroed() -> Self $body` (a plain `fn` under loom).
macro_rules! zeroed {
    ($(#[$doc:meta])* $vis:vis fn zeroed() -> Self $body:block) => {
        $(#[$doc])*
        #[cfg(not(loom))]
        $vis const fn zeroed() -> Self $body

        $(#[$doc])*
        #[cfg(loom)]
        $vis fn zeroed() -> Self $body
    };
}

/// `[const { $ty::zeroed() }; $n]` (built element by element under loom).
#[cfg(not(loom))]
macro_rules! zeroed_array {
    ($ty:ty; $n:expr) => {
        [const { <$ty>::zeroed() }; $n]
    };
}

/// `[const { $ty::zeroed() }; $n]` (built element by element under loom).
#[cfg(loom)]
macro_rules! zeroed_array {
    ($ty:ty; $n:expr) => {
        core::array::from_fn::<$ty, { $n }, _>(|_| <$ty>::zeroed())
    };
}

pub(crate) use {zeroed, zeroed_array};
