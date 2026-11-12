//! A plugin instance's account of what went wrong, for the person
//! debugging it inside a host: failures at the format boundary as log
//! messages, audio-thread faults as counts reported from the main thread.

mod call_error;
mod faults;
mod log;
mod panic_log;

pub use call_error::CallError;
pub use faults::{FaultCounts, ProcessFault};
pub use log::{log_dir, FileSink, Log, LogSink, Severity, TelemetrySink};
pub use panic_log::{drain_panics, log_panics, realtime};

use crate::Info;

pub struct Diagnostics {
    pub log: Log,
    pub faults: FaultCounts,
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
        }
    }

    pub fn report_faults(&self) {
        self.faults.report(&self.log);
    }

    /// Logs `"<call>: <error>"`.
    pub fn report(&self, call: &str, error: &CallError) {
        self.log
            .write(error.severity(), &format_args!("{call}: {error}"));
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
