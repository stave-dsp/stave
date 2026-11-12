//! A panic stays inside the plugin: every format entry point runs under
//! [`PanicGuard::run`] (or [`PanicGuard::run_realtime`] on the audio
//! thread). The first panic disables the instance; from then on every
//! call takes its safe answer (silence, refusal) without touching plugin
//! state again — so whatever the panic left half-updated is never
//! observed, which is what makes catching it sound.
//!
//! The panic is logged from a non-realtime call: a realtime panic only
//! records the call's name in a preallocated slot, and the next
//! [`run`](PanicGuard::run) (every main-thread entry point, disabled or
//! not) writes the log message.

use crate::diagnostics::{Diagnostics, Severity};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// [`PanicGuard::slot`] states.
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const RECORDED: u8 = 2;
const REPORTED: u8 = 3;

pub struct PanicGuard {
    disabled: AtomicBool,
    /// Which call panicked first, until it is logged: `EMPTY` → (one
    /// writer) `WRITING` → `RECORDED` → (one reporter) `REPORTED`.
    state: AtomicU8,
    slot: UnsafeCell<&'static str>,
}

// SAFETY: `slot` is written only by the thread that moved `state` from
// EMPTY to WRITING (a compare-exchange, so at most one thread ever), and
// read only by the thread that moved it from RECORDED to REPORTED
// (likewise one); the writer's Release store of RECORDED orders its write
// before that reader's Acquire exchange. No other access exists.
unsafe impl Sync for PanicGuard {}

impl Default for PanicGuard {
    fn default() -> Self {
        Self {
            disabled: AtomicBool::new(false),
            state: AtomicU8::new(EMPTY),
            slot: UnsafeCell::new(""),
        }
    }
}

impl PanicGuard {
    /// Non-realtime entry points. `call()`, or `failed()` once the
    /// instance is disabled or when `call` panics. Logs any panic not yet
    /// logged, including one an earlier realtime call recorded.
    pub fn run<R>(
        &self,
        diagnostics: &Diagnostics,
        name: &str,
        failed: impl FnOnce() -> R,
        call: impl FnOnce() -> R,
    ) -> R {
        self.report(diagnostics);
        if self.disabled() {
            return failed();
        }
        // AssertUnwindSafe: after a panic the instance is disabled, so no
        // state `call` touched is read again.
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => value,
            Err(_) => {
                self.disabled.store(true, Ordering::Release);
                diagnostics.log.write(
                    Severity::Fatal,
                    &format_args!("{name} panicked; the instance is disabled and outputs silence"),
                );
                failed()
            }
        }
    }

    /// The audio thread's entry point: as [`run`](Self::run), but a panic
    /// is only recorded (no lock, no allocation by the guard, no I/O);
    /// the next `run` logs it. [`unreported`](Self::unreported) afterwards
    /// is the cue to ask the host for a main-thread callback.
    pub fn run_realtime<R>(
        &self,
        name: &'static str,
        failed: impl FnOnce() -> R,
        call: impl FnOnce() -> R,
    ) -> R {
        if self.disabled() {
            return failed();
        }
        // AssertUnwindSafe: after a panic the instance is disabled, so no
        // state `call` touched is read again.
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => value,
            Err(_) => {
                self.disabled.store(true, Ordering::Release);
                self.record(name);
                failed()
            }
        }
    }

    /// Whether a panic is recorded and not yet logged.
    pub fn unreported(&self) -> bool {
        self.state.load(Ordering::Acquire) == RECORDED
    }

    pub fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Acquire)
    }

    fn record(&self, name: &'static str) {
        if self
            .state
            .compare_exchange(EMPTY, WRITING, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            // SAFETY: the exchange above made this thread the only writer
            // (see the `Sync` impl), and no reader runs before RECORDED.
            unsafe { *self.slot.get() = name };
            self.state.store(RECORDED, Ordering::Release);
        }
    }

    /// Log a recorded panic, once. Non-realtime: for main-thread calls
    /// that do not go through [`run`](Self::run) (CLAP `on_main_thread`).
    pub fn report(&self, diagnostics: &Diagnostics) {
        if self
            .state
            .compare_exchange(RECORDED, REPORTED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            // SAFETY: the exchange above made this thread the only reader,
            // after the writer's Release of RECORDED (see the `Sync` impl).
            let name = unsafe { *self.slot.get() };
            diagnostics.log.write(
                Severity::Fatal,
                &format_args!("{name} panicked; the instance is disabled and outputs silence"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Log, LogSink};
    use core::fmt::Display;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Vec<String>>>);

    impl LogSink for Memory {
        fn write(&self, _severity: Severity, message: &dyn Display) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    fn diagnostics() -> (Diagnostics, Memory) {
        let memory = Memory::default();
        let diagnostics = Diagnostics {
            log: Log::new(vec![Box::new(memory.clone())]),
            faults: Default::default(),
        };
        (diagnostics, memory)
    }

    #[test]
    fn a_panic_disables_the_instance_and_every_later_call_fails_safe() {
        let (diagnostics, memory) = diagnostics();
        let guard = PanicGuard::default();
        assert_eq!(guard.run(&diagnostics, "ok", || 0, || 1), 1);
        assert_eq!(
            guard.run(&diagnostics, "boom", || 0, || panic!("plugin bug")),
            0
        );
        assert!(guard.disabled());
        assert_eq!(memory.0.lock().unwrap().len(), 1, "logged by the call");
        let mut ran = false;
        assert_eq!(
            guard.run(
                &diagnostics,
                "after",
                || 0,
                || {
                    ran = true;
                    1
                }
            ),
            0
        );
        assert!(!ran, "a disabled instance never runs plugin code again");
    }

    /// The audio thread's panic is recorded, not logged; the next
    /// non-realtime call — on a disabled instance — logs it once.
    #[test]
    fn a_realtime_panic_is_logged_by_the_next_main_thread_call() {
        let (diagnostics, memory) = diagnostics();
        let guard = PanicGuard::default();
        assert_eq!(guard.run_realtime("process", || 0, || panic!("dsp bug")), 0);
        assert!(guard.disabled() && guard.unreported());
        assert!(
            memory.0.lock().unwrap().is_empty(),
            "nothing logged realtime"
        );
        assert_eq!(guard.run_realtime("process", || 0, || 1), 0);
        assert_eq!(guard.run(&diagnostics, "setActive", || 7, || 1), 7);
        guard.run(&diagnostics, "setActive", || (), || ());
        assert_eq!(
            *memory.0.lock().unwrap(),
            ["process panicked; the instance is disabled and outputs silence"]
        );
        assert!(!guard.unreported());
    }
}
