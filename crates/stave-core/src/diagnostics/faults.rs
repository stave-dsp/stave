//! What the audio thread could not do, as counts: recording is one atomic
//! add, so a fault costs nothing realtime; a non-realtime entry point
//! turns the counts into log messages.

use super::{Log, Severity};
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// A process call the wrapper had to refuse, or ran with input dropped.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessFault {
    /// A second process call while one was running.
    ConcurrentProcess,
    MissingProcessData,
    /// A block longer than the host's declared maximum.
    BlockTooLong,
    MissingOutput,
    /// Samples in a format other than 32-bit float.
    UnsupportedSampleSize,
    /// More parameter changes in one block than the wrapper holds.
    ParamChangesDropped,
    /// More note events in one block than the wrapper holds.
    EventsDropped,
}

impl ProcessFault {
    pub const ALL: [Self; 7] = [
        Self::ConcurrentProcess,
        Self::MissingProcessData,
        Self::BlockTooLong,
        Self::MissingOutput,
        Self::UnsupportedSampleSize,
        Self::ParamChangesDropped,
        Self::EventsDropped,
    ];

    pub fn severity(self) -> Severity {
        match self {
            Self::ConcurrentProcess
            | Self::MissingProcessData
            | Self::BlockTooLong
            | Self::MissingOutput
            | Self::UnsupportedSampleSize => Severity::HostMisbehaving,
            Self::ParamChangesDropped | Self::EventsDropped => Severity::Warning,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for ProcessFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ConcurrentProcess => "process refused: called while another process call ran",
            Self::MissingProcessData => "process refused: no process data",
            Self::BlockTooLong => "process refused: block longer than the declared maximum",
            Self::MissingOutput => "process refused: no output buffers",
            Self::UnsupportedSampleSize => "process refused: samples are not 32-bit float",
            Self::ParamChangesDropped => "parameter changes dropped: more than a block holds",
            Self::EventsDropped => "note events dropped: more than a block holds",
        })
    }
}

#[derive(Default)]
pub struct FaultCounts {
    recorded: [AtomicU32; ProcessFault::ALL.len()],
    reported: [AtomicU32; ProcessFault::ALL.len()],
    unreported: AtomicBool,
}

impl FaultCounts {
    /// Realtime-safe. True for the first fault since the last report — the
    /// caller's cue to ask the host for a main-thread callback.
    pub fn record(&self, fault: ProcessFault, count: u32) -> bool {
        if count == 0 {
            return false;
        }
        self.recorded[fault.index()].fetch_add(count, Ordering::Relaxed);
        !self.unreported.swap(true, Ordering::AcqRel)
    }

    /// One message per fault recorded since the last report. Any number
    /// of threads may report; each count is reported once.
    pub fn report(&self, log: &Log) {
        self.unreported.store(false, Ordering::Release);
        for fault in ProcessFault::ALL {
            let i = fault.index();
            let now = self.recorded[i].load(Ordering::Relaxed);
            let new = now.wrapping_sub(self.reported[i].swap(now, Ordering::Relaxed));
            if new > 0 {
                log.write(fault.severity(), &format_args!("{fault} (×{new})"));
            }
        }
    }
}
