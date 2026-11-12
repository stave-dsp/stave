//! A panic's message and location in the plugin's log. Each plugin is
//! its own binary with its own copy of std, so the hook sees only this
//! plugin's panics.
//!
//! Off the audio thread the hook writes the log line at once — with
//! `panic = "abort"` the last thing the plugin says before its host goes
//! down. On the audio thread (inside [`realtime`]) it must not open a
//! file, take a lock or allocate: it formats into one of a few
//! preallocated records, and the next main-thread entry point drains
//! them into the instance's log ([`drain_panics`], called by
//! `PanicGuard::report`). The default hook (stderr, backtrace lock) runs
//! only off the audio thread.

use super::{Diagnostics, Log, Severity};
use crate::Info;
use core::cell::{Cell, UnsafeCell};
use core::fmt::{self, Write as _};
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Once;

/// Idempotent; the format entry points call it.
pub fn log_panics(plugin: &'static Info) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Under `panic = "abort"` the process is going down: nothing
            // drains a record, so the line is written now, realtime or not.
            if REALTIME.get() && cfg!(panic = "unwind") {
                PENDING.record(info);
                return;
            }
            Diagnostics::new(plugin).log.write(Severity::Fatal, info);
            previous(info);
        }));
    });
}

thread_local! {
    /// Whether this thread is inside a realtime call ([`realtime`]).
    /// A `const` initialiser: no lazy allocation on first access.
    static REALTIME: Cell<bool> = const { Cell::new(false) };
}

/// Runs `call` as realtime code: a panic inside it is recorded, not
/// written (see the module docs). Nests; restores the outer state even
/// when `call` unwinds.
pub fn realtime<R>(call: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REALTIME.set(self.0);
        }
    }
    let _restore = Restore(REALTIME.replace(true));
    call()
}

/// Writes every panic the audio thread recorded since the last drain to
/// `log`, oldest slot first, plus a count of any that found no free
/// record. Non-realtime.
pub fn drain_panics(log: &Log) {
    PENDING.drain(|text| log.write(Severity::Fatal, &text));
    let dropped = PENDING.dropped.swap(0, Ordering::Relaxed);
    if dropped > 0 {
        log.write(
            Severity::Fatal,
            &format_args!("{dropped} more realtime panic(s); no record was free"),
        );
    }
}

/// Records the audio thread can fill before a drain.
const RECORDS: usize = 4;
/// Bytes of message per record; longer messages are cut at a char
/// boundary and end in `…`.
const MESSAGE: usize = 512;

/// [`Record::state`] values.
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const FULL: u8 = 2;
const READING: u8 = 3;

static PENDING: Pending = Pending {
    records: [const { Record::new() }; RECORDS],
    dropped: AtomicU32::new(0),
};

struct Pending {
    records: [Record; RECORDS],
    /// Panics that found every record taken.
    dropped: AtomicU32,
}

struct Record {
    /// `EMPTY` → (one writer) `WRITING` → `FULL` → (one reader)
    /// `READING` → `EMPTY`.
    state: AtomicU8,
    len: Cell<usize>,
    text: UnsafeCell<[u8; MESSAGE]>,
}

// SAFETY: `len` and `text` are touched only by the one thread that moved
// `state` out of EMPTY (the writer, until its Release store of FULL) or
// out of FULL (the reader, until its Release store of EMPTY). Those are
// compare-exchanges, so at most one thread holds a record at a time, and
// the Acquire half of each exchange orders the previous holder's writes
// before the next holder's accesses.
unsafe impl Sync for Record {}

impl Record {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            len: Cell::new(0),
            text: UnsafeCell::new([0; MESSAGE]),
        }
    }

    fn claim(&self, from: u8, to: u8) -> bool {
        self.state
            .compare_exchange(from, to, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}

impl Pending {
    /// Realtime: no lock, no allocation, no I/O.
    fn record(&self, message: &dyn fmt::Display) {
        let Some(record) = self.records.iter().find(|r| r.claim(EMPTY, WRITING)) else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        // SAFETY: the claim made this thread the record's only holder
        // (see the `Sync` impl).
        let text = unsafe { &mut *record.text.get() };
        let mut out = Truncating { buf: text, len: 0 };
        // A Display error only cuts the message short.
        let _ = write!(out, "{message}");
        record.len.set(out.len);
        record.state.store(FULL, Ordering::Release);
    }

    fn drain(&self, mut write: impl FnMut(&str)) {
        for record in &self.records {
            if !record.claim(FULL, READING) {
                continue;
            }
            // SAFETY: the claim made this thread the record's only holder,
            // after the writer's Release of FULL (see the `Sync` impl).
            let text = unsafe { &*record.text.get() };
            let len = record.len.get();
            // `Truncating` only ever cuts at a char boundary.
            write(core::str::from_utf8(&text[..len]).unwrap_or("(unreadable panic message)"));
            record.state.store(EMPTY, Ordering::Release);
        }
    }
}

/// Writes into a fixed buffer, cutting at a char boundary and marking the
/// cut with `…` once it is full.
struct Truncating<'a> {
    buf: &'a mut [u8; MESSAGE],
    len: usize,
}

const ELLIPSIS: &str = "…";

impl fmt::Write for Truncating<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = MESSAGE - ELLIPSIS.len();
        if self.len >= room {
            return Err(fmt::Error);
        }
        if self.len + s.len() <= room {
            self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
            return Ok(());
        }
        let mut cut = room - self.len;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        self.buf[self.len..self.len + cut].copy_from_slice(&s.as_bytes()[..cut]);
        self.len += cut;
        self.buf[self.len..self.len + ELLIPSIS.len()].copy_from_slice(ELLIPSIS.as_bytes());
        self.len += ELLIPSIS.len();
        Err(fmt::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::LogSink;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Vec<String>>>);

    impl LogSink for Memory {
        fn write(&self, _severity: Severity, message: &dyn fmt::Display) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    /// One test drives the process-wide records (tests run in parallel).
    #[test]
    fn records_hold_what_the_audio_thread_wrote_until_drained() {
        let memory = Memory::default();
        let log = Log::new(vec![Box::new(memory.clone())]);
        PENDING.record(&"short");
        let long = "é".repeat(MESSAGE);
        PENDING.record(&long);
        for _ in 0..RECORDS {
            PENDING.record(&"overflow");
        }
        drain_panics(&log);
        let lines = memory.0.lock().unwrap().clone();
        assert_eq!(lines.len(), RECORDS + 1, "{lines:?}");
        assert_eq!(lines[0], "short");
        assert!(lines[1].len() <= MESSAGE && lines[1].ends_with("é…"));
        assert_eq!(
            lines[RECORDS],
            "2 more realtime panic(s); no record was free"
        );
        drain_panics(&log);
        assert_eq!(memory.0.lock().unwrap().len(), RECORDS + 1, "drained once");
    }

    #[test]
    fn the_realtime_scope_nests_and_survives_an_unwind() {
        assert!(!REALTIME.get());
        let caught = std::panic::catch_unwind(|| {
            realtime(|| {
                realtime(|| assert!(REALTIME.get()));
                assert!(REALTIME.get());
                panic!("inside");
            })
        });
        assert!(caught.is_err());
        assert!(!REALTIME.get());
    }
}
