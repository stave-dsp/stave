//! Shared debug log + audio-thread stats.
//!
//! `DebugLog` is a fixed-size ring of log lines, written from any thread,
//! read by the GUI for the debug panel. We keep it deliberately small and
//! lock-only-on-write (parking would be wrong on the audio thread, so the
//! audio path goes through `record_stats` which is lock-free).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

const LOG_CAPACITY: usize = 256;
/// Rolling buffer of recent callback durations (us) — lets the GUI
/// show a sparkline + percentiles without a heap allocation.
pub const PERF_HISTORY: usize = 512;

#[derive(Clone, Debug)]
pub struct LogLine {
    pub when_ms: u64,
    pub level: LogLevel,
    pub text: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

pub struct DebugLog {
    epoch: Instant,
    lines: Mutex<Vec<LogLine>>,
    /// Lock-free counters updated from the audio callback.
    pub callback_count: AtomicU64,
    pub frames_processed: AtomicU64,
    /// Last callback duration in microseconds.
    pub last_cb_us: AtomicU32,
    /// Running max callback duration in microseconds (since session start).
    pub max_cb_us: AtomicU32,
    /// Session count of callbacks that overran their realtime budget
    /// (a dropout the listener may have heard). Audio thread bumps it.
    pub deadline_misses: AtomicU64,
    /// Events the GUI failed to push (queue full).
    pub dropped_events: AtomicU64,
    /// Events the audio thread popped.
    pub consumed_events: AtomicU64,
    /// Rolling history of callback durations (us). Indexed by
    /// `perf_idx & (PERF_HISTORY - 1)`.
    pub perf_us: [AtomicU32; PERF_HISTORY],
    pub perf_idx: AtomicU64,
    /// Rolling history of frames per callback so the GUI can compute
    /// CPU % = perf_us / (frames × 1e6 / sample_rate).
    pub perf_frames: [AtomicU32; PERF_HISTORY],
    /// Plugin-reported processing latency in samples (set when the
    /// plugin is opened; remains until the next reload).
    pub plugin_latency_samples: AtomicU32,
}

impl DebugLog {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            epoch: Instant::now(),
            lines: Mutex::new(Vec::with_capacity(LOG_CAPACITY)),
            callback_count: AtomicU64::new(0),
            frames_processed: AtomicU64::new(0),
            last_cb_us: AtomicU32::new(0),
            max_cb_us: AtomicU32::new(0),
            deadline_misses: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            consumed_events: AtomicU64::new(0),
            perf_us: std::array::from_fn(|_| AtomicU32::new(0)),
            perf_idx: AtomicU64::new(0),
            perf_frames: std::array::from_fn(|_| AtomicU32::new(0)),
            plugin_latency_samples: AtomicU32::new(0),
        })
    }

    fn push(&self, level: LogLevel, text: String) {
        let ms = self.epoch.elapsed().as_millis() as u64;
        let line = LogLine {
            when_ms: ms,
            level,
            text,
        };
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() >= LOG_CAPACITY {
            lines.remove(0);
        }
        lines.push(line);
    }

    pub fn info(&self, text: impl Into<String>) {
        self.push(LogLevel::Info, text.into());
    }
    pub fn warn(&self, text: impl Into<String>) {
        self.push(LogLevel::Warn, text.into());
    }
    pub fn error(&self, text: impl Into<String>) {
        self.push(LogLevel::Error, text.into());
    }

    pub fn snapshot(&self) -> Vec<LogLine> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn clear(&self) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Audio-thread entry. Updates the timing atomics; never blocks.
    pub fn record_callback(&self, frames: u64, took_us: u32) {
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        self.frames_processed.fetch_add(frames, Ordering::Relaxed);
        self.last_cb_us.store(took_us, Ordering::Relaxed);
        self.max_cb_us.fetch_max(took_us, Ordering::Relaxed);
        let i = self.perf_idx.fetch_add(1, Ordering::Relaxed) as usize & (PERF_HISTORY - 1);
        self.perf_us[i].store(took_us, Ordering::Relaxed);
        self.perf_frames[i].store(frames as u32, Ordering::Relaxed);
    }

    /// Snapshot the rolling perf history into a Vec. The audio thread
    /// keeps writing while we read; values may be torn at the leading
    /// edge but the GUI doesn't care about a single sample.
    pub fn perf_snapshot(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::with_capacity(PERF_HISTORY);
        let cur = self.perf_idx.load(Ordering::Relaxed) as usize;
        let n = PERF_HISTORY.min(cur);
        for k in 0..n {
            let idx = (cur + PERF_HISTORY - n + k) & (PERF_HISTORY - 1);
            out.push((
                self.perf_us[idx].load(Ordering::Relaxed),
                self.perf_frames[idx].load(Ordering::Relaxed),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_history_is_chronological_and_bounded() {
        let log = DebugLog::new();
        for i in 0..(PERF_HISTORY as u32 + 3) {
            log.record_callback(64, i);
        }
        let h = log.perf_snapshot();
        assert_eq!(h.len(), PERF_HISTORY);
        assert_eq!((h[0].0, h.last().unwrap().0), (3, PERF_HISTORY as u32 + 2));
        assert_eq!(
            log.max_cb_us.load(Ordering::Relaxed),
            PERF_HISTORY as u32 + 2
        );
        assert_eq!(
            log.frames_processed.load(Ordering::Relaxed),
            64 * (PERF_HISTORY as u64 + 3)
        );
    }

    #[test]
    fn log_lines_keep_level_and_text() {
        let log = DebugLog::new();
        log.warn("careful");
        let lines = log.snapshot();
        assert_eq!(
            (lines[0].level, lines[0].text.as_str()),
            (LogLevel::Warn, "careful")
        );
        log.clear();
        assert!(log.snapshot().is_empty());
    }
}
