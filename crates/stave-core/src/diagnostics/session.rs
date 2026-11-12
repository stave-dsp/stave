//! The event trace of a real session, written as it happens: with
//! `STAVE_EVENTS` set, each instance appends its trace to
//! `<log dir>/<plugin id>-<pid>-<n>.events` (one record per line, the
//! text `stave_telemetry::events::Trace` reads back), from the main
//! thread, each time the wrapper reports. `stave-events FILE` checks it
//! against the protocol's laws — so a DAW session is a test run.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use stave_telemetry::Events;

/// Where an instance's trace goes, and how far it has been written.
#[derive(Default)]
pub struct Session {
    file: Option<Mutex<(PathBuf, u64)>>,
}

impl Session {
    /// A session file for an instance of plugin `id`, when `STAVE_EVENTS`
    /// is set and there is a log directory; otherwise none.
    pub fn new(id: &str) -> Self {
        static INSTANCES: AtomicU32 = AtomicU32::new(0);
        let on = std::env::var_os("STAVE_EVENTS").is_some_and(|v| !v.is_empty());
        let file = on.then(super::log_dir).flatten().map(|dir| {
            let n = INSTANCES.fetch_add(1, Ordering::Relaxed);
            Mutex::new((dir.join(format!("{id}-{}-{n}.events", std::process::id())), 0))
        });
        Session { file }
    }

    /// Append every record written since the last call. Not realtime
    /// (file I/O); a failed write is dropped (the trace is evidence, not
    /// state).
    pub fn flush(&self, events: &Events) {
        let Some(file) = &self.file else { return };
        let mut file = file.lock().unwrap_or_else(PoisonError::into_inner);
        let (path, next) = &mut *file;
        let mut text = String::new();
        for r in events.read_from(*next) {
            *next = r.index + 1;
            text.push_str(&r.to_string());
            text.push('\n');
        }
        if text.is_empty() {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&*path) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_telemetry::events::{check, Trace};
    use stave_telemetry::{Event, Telemetry};

    #[test]
    #[cfg_attr(miri, ignore = "Miri isolates the file system")]
    fn a_session_file_reads_back_as_the_trace_it_wrote() {
        let dir = std::env::temp_dir().join(format!("stave-session-{}", std::process::id()));
        let path = dir.join("one.events");
        let _ = std::fs::remove_file(&path);
        let session = Session { file: Some(Mutex::new((path.clone(), 0))) };
        let t = Telemetry::new();
        t.event(Event::Prepare { sample_rate: 48_000.0, max_block: 128 });
        t.event(Event::Begin(3));
        session.flush(t.events());
        t.event(Event::End(3));
        t.event(Event::Release);
        session.flush(t.events());
        session.flush(t.events());
        let trace: Trace = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(
            trace.events(),
            [Event::Prepare { sample_rate: 48_000.0, max_block: 128 }, Event::Begin(3), Event::End(3), Event::Release]
        );
        assert_eq!(check(&trace), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
