//! Messages from a plugin instance to whoever is listening: the host's
//! log (CLAP `clap.log`), a file, a test. Non-realtime only — the audio
//! thread records [`super::ProcessFault`]s instead.

use stave_debug::DebugBus;
use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// CLAP's `clap_log_severity`, values included.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
    Fatal = 4,
    /// The host broke the format's contract.
    HostMisbehaving = 5,
    /// The plugin broke the format's contract.
    PluginMisbehaving = 6,
}

impl Severity {
    /// The severity a value from `Severity as u8` names.
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Debug,
            1 => Self::Info,
            2 => Self::Warning,
            3 => Self::Error,
            4 => Self::Fatal,
            5 => Self::HostMisbehaving,
            6 => Self::PluginMisbehaving,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Fatal => "fatal",
            Self::HostMisbehaving => "host-misbehaving",
            Self::PluginMisbehaving => "plugin-misbehaving",
        }
    }
}

pub trait LogSink: Send + Sync {
    fn write(&self, severity: Severity, message: &dyn Display);
}

/// Fans each message out to every sink. Sinks can join after creation
/// (a CLAP host's log is only reachable from `init`).
#[derive(Default)]
pub struct Log {
    sinks: Mutex<Vec<Box<dyn LogSink>>>,
}

impl Log {
    pub fn new(sinks: Vec<Box<dyn LogSink>>) -> Self {
        Self {
            sinks: Mutex::new(sinks),
        }
    }

    pub fn add(&self, sink: Box<dyn LogSink>) {
        self.sinks().push(sink);
    }

    pub fn write(&self, severity: Severity, message: &dyn Display) {
        for sink in self.sinks().iter() {
            sink.write(severity, message);
        }
    }

    fn sinks(&self) -> MutexGuard<'_, Vec<Box<dyn LogSink>>> {
        self.sinks.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Into the debug bus's message ring, where a stave host reads it live.
pub struct BusSink(pub Arc<DebugBus>);

impl LogSink for BusSink {
    fn write(&self, severity: Severity, message: &dyn Display) {
        self.0.messages.push(severity as u8, &message.to_string());
    }
}

/// Appends one line per message, `<UTC time> <severity> <source>: <message>`.
/// The file is opened per write: messages are rare, and a closed file
/// survives the user deleting or rotating it.
pub struct FileSink {
    pub path: PathBuf,
    /// Names the instance in each line (the plugin's name).
    pub source: String,
}

impl LogSink for FileSink {
    fn write(&self, severity: Severity, message: &dyn Display) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let _ = writeln!(
            file,
            "{} {} {}: {message}",
            utc_timestamp(SystemTime::now()),
            severity.as_str(),
            self.source,
        );
    }
}

/// Where plugin logs go: `$STAVE_LOG_DIR`, else the platform's per-user
/// log directory (macOS `~/Library/Logs/stave`, Windows
/// `%LOCALAPPDATA%\stave\logs`, elsewhere `$XDG_STATE_HOME/stave` or
/// `~/.local/state/stave`). `None` when no home directory is known.
pub fn log_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("STAVE_LOG_DIR") {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Logs/stave"))
    } else if cfg!(windows) {
        var("LOCALAPPDATA").map(|local| local.join("stave").join("logs"))
    } else {
        var("XDG_STATE_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".local/state")))
            .map(|state| state.join("stave"))
    }
}

/// RFC 3339 UTC to the second.
fn utc_timestamp(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → proleptic Gregorian (year, month, day)
/// (Hinnant, "chrono-Compatible Low-Level Date Algorithms").
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_rfc3339_utc() {
        let at = |secs| utc_timestamp(UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_791_892_799), "2026-10-13T11:59:59Z");
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri isolates the file system")]
    fn the_file_sink_appends_lines() {
        let dir = std::env::temp_dir().join(format!("stave-log-{}", std::process::id()));
        let sink = FileSink {
            path: dir.join("nested").join("probe.log"),
            source: "Probe".into(),
        };
        sink.write(Severity::Warning, &"first");
        sink.write(Severity::HostMisbehaving, &format_args!("second {}", 2));
        let text = std::fs::read_to_string(&sink.path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" warning Probe: first"), "{}", lines[0]);
        assert!(
            lines[1].ends_with(" host-misbehaving Probe: second 2"),
            "{}",
            lines[1]
        );
    }
}
