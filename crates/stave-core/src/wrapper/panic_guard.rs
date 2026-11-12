//! A panic stays inside the plugin: every format entry point runs under
//! [`PanicGuard::run`]. The first panic is logged and disables the
//! instance; from then on every call takes its safe answer (silence,
//! refusal) without touching plugin state again — so whatever the panic
//! left half-updated is never observed, which is what makes catching it
//! sound.

use crate::diagnostics::{Diagnostics, Severity};
use core::sync::atomic::{AtomicBool, Ordering};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Default)]
pub struct PanicGuard {
    disabled: AtomicBool,
}

impl PanicGuard {
    /// `call()`, or `failed()` once the instance is disabled or when
    /// `call` panics.
    pub fn run<R>(
        &self,
        diagnostics: &Diagnostics,
        name: &str,
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
                diagnostics.log.write(
                    Severity::Fatal,
                    &format_args!("{name} panicked; the instance is disabled and outputs silence"),
                );
                failed()
            }
        }
    }

    pub fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Log;

    #[test]
    fn a_panic_disables_the_instance_and_every_later_call_fails_safe() {
        let diagnostics = Diagnostics {
            log: Log::default(),
            faults: Default::default(),
        };
        let guard = PanicGuard::default();
        assert_eq!(guard.run(&diagnostics, "ok", || 0, || 1), 1);
        assert_eq!(
            guard.run(&diagnostics, "boom", || 0, || panic!("plugin bug")),
            0
        );
        assert!(guard.disabled());
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
}
