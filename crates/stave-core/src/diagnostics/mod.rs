//! A plugin instance's account of what went wrong, for the person
//! debugging it inside a host: failures at the format boundary as log
//! messages, audio-thread faults as counts reported from the main thread;
//! and the instance's event trace (lifecycle, gestures, restores, faults)
//! on its telemetry, written to a session file on request ([`Session`]).

mod call_error;
mod faults;
mod log;
mod panic_log;
mod session;

pub use call_error::CallError;
pub use faults::{FaultCounts, ProcessFault};
pub use log::{log_dir, FileSink, Log, LogSink, Severity, TelemetrySink};
pub use panic_log::{drain_panics, log_panics, realtime};
pub use session::Session;

use std::sync::Arc;

use stave_telemetry::{Event, Telemetry};

use crate::Info;

pub struct Diagnostics {
    pub log: Log,
    pub faults: FaultCounts,
    /// The trace's session file (`STAVE_EVENTS`).
    pub session: Session,
}

impl Diagnostics {
    /// Logs to `<log_dir>/<plugin id>.log`.
    pub fn new(info: &Info) -> Self {
        let file = log_dir().map(|dir| -> Box<dyn LogSink> {
            Box::new(FileSink {
                path: dir.join(format!("{}.log", info.id)),
                source: info.name.to_string(),
            })
        });
        Self {
            log: Log::new(file.into_iter().collect()),
            faults: FaultCounts::default(),
            session: Session::new(info.id),
        }
    }

    /// Record the instance's events in `telemetry`'s trace from now on
    /// (the first call wins): faults as they are recorded, and every
    /// [`event`](Self::event) the wrapper reports.
    pub fn watch(&self, telemetry: &Arc<Telemetry>) {
        let _ = self.faults.events.set(telemetry.clone());
    }

    /// Record `event` in the watched trace (nothing before [`watch`](Self::watch)).
    pub fn event(&self, event: Event) {
        if let Some(t) = self.faults.events.get() {
            t.event(event);
        }
    }

    /// Main thread: log the faults recorded since the last report, and
    /// write the trace's new records to the session file (if any).
    pub fn report_faults(&self) {
        self.faults.report(&self.log);
        if let Some(t) = self.faults.events.get() {
            self.session.flush(t.events());
        }
    }

    /// Logs `"<call>: <error>"`.
    pub fn report(&self, call: &str, error: &CallError) {
        self.log
            .write(error.severity(), &format_args!("{call}: {error}"));
    }
}

/// The session file gets the trace's last records (a release before the
/// host destroys the instance).
impl Drop for Diagnostics {
    fn drop(&mut self) {
        if let Some(t) = self.faults.events.get() {
            self.session.flush(t.events());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Display;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Vec<(Severity, String)>>>);

    impl LogSink for Memory {
        fn write(&self, severity: Severity, message: &dyn Display) {
            self.0.lock().unwrap().push((severity, message.to_string()));
        }
    }

    #[test]
    fn faults_are_reported_once_with_their_count() {
        let memory = Memory::default();
        let diagnostics = Diagnostics {
            log: Log::new(vec![Box::new(memory.clone())]),
            faults: FaultCounts::default(),
            session: Session::default(),
        };
        let faults = &diagnostics.faults;
        assert!(faults.record(ProcessFault::ConcurrentProcess, 1));
        assert!(!faults.record(ProcessFault::ConcurrentProcess, 1));
        assert!(!faults.record(ProcessFault::EventsDropped, 7));
        diagnostics.report_faults();
        diagnostics.report_faults();
        assert!(faults.record(ProcessFault::EventsDropped, 1));
        let messages = memory.0.lock().unwrap().clone();
        assert_eq!(
            messages,
            [
                (
                    Severity::HostMisbehaving,
                    "process refused: called while another process call ran (×2)".to_string()
                ),
                (
                    Severity::Warning,
                    "note events dropped: more than a block holds (×7)".to_string()
                ),
            ]
        );
    }
}
