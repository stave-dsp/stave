//! The event trace: what happened to a plugin instance, in order, as
//! typed records on its telemetry block — its lifecycle (prepare,
//! release), every edit gesture (begin, perform, end), every restore
//! (state, preset) and every fault — so whoever holds the block (a test,
//! a tool, a host, the wrapper writing a session file) reads it back
//! and checks it against the protocol's laws ([`check`]). Observability
//! is verification: the same trace a person reads to see what a DAW did
//! is the one a test asserts clean.
//!
//! Writing is lock-free and allocation-free from any thread (the audio
//! thread records faults): a ring of seqlocked slots, as [`Messages`]
//! is. A reader that falls more than [`EVENT_SLOTS`] behind loses the
//! oldest, and knows it ([`Recorder::lost`]).
//!
//! ```text
//! 0 @0 prepare 48000 512
//! 1 @3 begin 7
//! 2 @3 perform 7 0.25
//! 3 @4 end 7
//! 4 @9 restore ok
//! 5 @9 preset 2
//! 6 @12 fault unprepared
//! 7 @12 release
//! ```
//!
//! [`Messages`]: crate::Messages

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::str::FromStr;
use core::sync::atomic::{fence, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::EVENT_SLOTS;

/// What went wrong, as the wrapper saw it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Fault {
    /// A host call panicked (caught at the boundary).
    Panic,
    /// A block dropped (silenced) because another call held the plugin:
    /// a state save or load on another thread, or an overlapping call.
    ConcurrentProcess,
    /// A process call with no process data.
    MissingProcessData,
    /// A block longer than the host's declared maximum.
    BlockTooLong,
    /// A process call with no output buffers.
    MissingOutput,
    /// Samples in a format other than 32-bit float.
    UnsupportedSampleSize,
    /// More parameter changes in one block than the wrapper holds.
    ParamChangesDropped,
    /// More note events in one block than the wrapper holds.
    EventsDropped,
    /// A block cut to the prepared scratch.
    BlockTruncated,
    /// A process call while the plugin was not prepared.
    Unprepared,
}

impl Fault {
    pub const ALL: [Fault; 10] = [
        Fault::Panic,
        Fault::ConcurrentProcess,
        Fault::MissingProcessData,
        Fault::BlockTooLong,
        Fault::MissingOutput,
        Fault::UnsupportedSampleSize,
        Fault::ParamChangesDropped,
        Fault::EventsDropped,
        Fault::BlockTruncated,
        Fault::Unprepared,
    ];

    /// The word a trace line names it by.
    pub fn name(self) -> &'static str {
        match self {
            Fault::Panic => "panic",
            Fault::ConcurrentProcess => "concurrent-process",
            Fault::MissingProcessData => "missing-process-data",
            Fault::BlockTooLong => "block-too-long",
            Fault::MissingOutput => "missing-output",
            Fault::UnsupportedSampleSize => "unsupported-sample-size",
            Fault::ParamChangesDropped => "param-changes-dropped",
            Fault::EventsDropped => "events-dropped",
            Fault::BlockTruncated => "block-truncated",
            Fault::Unprepared => "unprepared",
        }
    }

    fn code(self) -> u32 {
        Fault::ALL.iter().position(|&f| f == self).unwrap_or(0) as u32
    }
}

/// One thing that happened to the instance. Parameters are named by their
/// id's number (`ParamId.0`); values are normalized.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Event {
    /// Prepared (activated) at these conditions.
    Prepare { sample_rate: f32, max_block: u32 },
    /// Released (deactivated): no processing until the next prepare.
    Release,
    /// An edit gesture opened on a parameter.
    Begin(u32),
    /// A parameter moved to a normalized value inside a gesture.
    Perform(u32, f64),
    /// The gesture on a parameter closed.
    End(u32),
    /// A saved state was applied (`true`), or refused and nothing changed.
    Restore(bool),
    /// Factory preset `n` was applied.
    Preset(u32),
    Fault(Fault),
}

/// One event as recorded: its place in the trace, the block it landed in
/// (the telemetry's block count when it was written), and what it was.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Record {
    pub index: u64,
    pub block: u64,
    pub event: Event,
}

// ----------------------------------------------------------------- ring

const PREPARE: u8 = 1;
const RELEASE: u8 = 2;
const BEGIN: u8 = 3;
const PERFORM: u8 = 4;
const END: u8 = 5;
const RESTORE: u8 = 6;
const PRESET: u8 = 7;
const FAULT: u8 = 8;

#[repr(C)]
struct Slot {
    sequence: AtomicU64,
    kind: AtomicU8,
    a: AtomicU32,
    b: AtomicU64,
    block: AtomicU64,
}

impl Slot {
    const fn zeroed() -> Self {
        Slot {
            sequence: AtomicU64::new(0),
            kind: AtomicU8::new(0),
            a: AtomicU32::new(0),
            b: AtomicU64::new(0),
            block: AtomicU64::new(0),
        }
    }
}

/// The instance's event ring (on the telemetry block:
/// [`Telemetry::events`](crate::Telemetry::events)).
#[repr(C)]
pub struct Events {
    written: AtomicU64,
    /// Lost to a slot another writer held at the same moment.
    dropped: AtomicU32,
    slots: [Slot; EVENT_SLOTS],
}

impl Default for Events {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl Events {
    pub const fn zeroed() -> Self {
        Events {
            written: AtomicU64::new(0),
            dropped: AtomicU32::new(0),
            slots: [const { Slot::zeroed() }; EVENT_SLOTS],
        }
    }

    /// Record `event` as landing in `block`. Any thread, lock-free, never
    /// allocates.
    pub fn push(&self, block: u64, event: Event) {
        let (kind, a, b) = encode(event);
        let index = self.written.fetch_add(1, Ordering::Relaxed);
        let slot = &self.slots[index as usize % EVENT_SLOTS];
        let current = slot.sequence.load(Ordering::Relaxed);
        if current % 2 == 1
            || slot
                .sequence
                .compare_exchange(current, current | 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        fence(Ordering::Release);
        slot.kind.store(kind, Ordering::Relaxed);
        slot.a.store(a, Ordering::Relaxed);
        slot.b.store(b, Ordering::Relaxed);
        slot.block.store(block, Ordering::Relaxed);
        slot.sequence.store(published(index), Ordering::Release);
    }

    /// How many events have been written (the next one's index).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    /// Events lost to a writer collision (two threads on one slot).
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The records with index ≥ `from` still in the ring, oldest first.
    pub fn read_from(&self, from: u64) -> impl Iterator<Item = Record> + '_ {
        let written = self.written();
        let oldest = written.saturating_sub(EVENT_SLOTS as u64).max(from);
        (oldest..written).filter_map(move |index| self.read(index))
    }

    fn read(&self, index: u64) -> Option<Record> {
        let slot = &self.slots[index as usize % EVENT_SLOTS];
        let before = slot.sequence.load(Ordering::Acquire);
        if before != published(index) {
            return None;
        }
        let (kind, a, b, block) = (
            slot.kind.load(Ordering::Relaxed),
            slot.a.load(Ordering::Relaxed),
            slot.b.load(Ordering::Relaxed),
            slot.block.load(Ordering::Relaxed),
        );
        fence(Ordering::Acquire);
        if slot.sequence.load(Ordering::Relaxed) != before {
            return None;
        }
        Some(Record { index, block, event: decode(kind, a, b)? })
    }
}

/// The even sequence a slot holds once event `index` is in it.
fn published(index: u64) -> u64 {
    2 * (index + 1)
}

fn encode(event: Event) -> (u8, u32, u64) {
    match event {
        Event::Prepare { sample_rate, max_block } => (PREPARE, max_block, u64::from(sample_rate.to_bits())),
        Event::Release => (RELEASE, 0, 0),
        Event::Begin(p) => (BEGIN, p, 0),
        Event::Perform(p, v) => (PERFORM, p, v.to_bits()),
        Event::End(p) => (END, p, 0),
        Event::Restore(ok) => (RESTORE, u32::from(ok), 0),
        Event::Preset(n) => (PRESET, n, 0),
        Event::Fault(f) => (FAULT, f.code(), 0),
    }
}

fn decode(kind: u8, a: u32, b: u64) -> Option<Event> {
    Some(match kind {
        PREPARE => Event::Prepare { sample_rate: f32::from_bits(b as u32), max_block: a },
        RELEASE => Event::Release,
        BEGIN => Event::Begin(a),
        PERFORM => Event::Perform(a, f64::from_bits(b)),
        END => Event::End(a),
        RESTORE => Event::Restore(a != 0),
        PRESET => Event::Preset(a),
        FAULT => Event::Fault(*Fault::ALL.get(a as usize)?),
        _ => return None,
    })
}

// ------------------------------------------------------------- recorder

/// A reader that keeps everything: drain it often enough (every main-
/// thread call, every frame) and it holds the whole session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorder {
    next: u64,
    lost: u64,
    records: Vec<Record>,
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take every record written since the last drain; returns how many.
    pub fn drain(&mut self, events: &Events) -> usize {
        let before = self.records.len();
        for r in events.read_from(self.next) {
            self.lost += r.index - self.next;
            self.next = r.index + 1;
            self.records.push(r);
        }
        let written = events.written();
        if written > self.next {
            // Written but not readable: overwritten before this drain, or
            // being written now (the next drain finds those).
            let overwritten = written.saturating_sub(EVENT_SLOTS as u64);
            if overwritten > self.next {
                self.lost += overwritten - self.next;
                self.next = overwritten;
            }
        }
        self.records.len() - before
    }

    /// Records the ring overwrote before this reader saw them.
    pub fn lost(&self) -> u64 {
        self.lost
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Everything kept so far, as a trace.
    pub fn trace(&self) -> Trace {
        Trace(self.records.clone())
    }
}

// ---------------------------------------------------------------- laws

/// A run's records in order: what [`check`] reads, and what a session
/// file holds (one [`Record`] per line, see the module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace(pub Vec<Record>);

/// Where a trace broke a law.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// The record's index (`None` for a law about the trace's end).
    pub index: Option<u64>,
    pub law: &'static str,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(i) => write!(f, "event {i}: {}", self.law),
            None => write!(f, "at the end: {}", self.law),
        }
    }
}

/// The laws every run keeps, checked over its trace:
///
/// * a gesture on a parameter is begun once and ended once (never begun
///   while open, never ended unbegun), and a face's edit lands inside
///   one (`perform` only while its parameter's gesture is open);
/// * a gap in the indices (a reader that fell behind) is reported, not
///   guessed across;
/// * release follows a prepare;
/// * no fault.
///
/// Every violation, in order. A trace that ends with a gesture open is
/// [`quiescent`] no more; that is a separate question (a session cut
/// short mid-drag is not wrong).
pub fn check(trace: &Trace) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut open: Vec<u32> = Vec::new();
    let mut prepared: Option<bool> = None;
    let mut next: Option<u64> = None;
    for r in &trace.0 {
        let at = |law| Violation { index: Some(r.index), law };
        if next.is_some_and(|n| r.index != n) {
            out.push(at("events missing before this one (the reader fell behind)"));
            open.clear();
            prepared = None;
        }
        next = Some(r.index + 1);
        match r.event {
            Event::Begin(p) if open.contains(&p) => out.push(at("a gesture begun while open")),
            Event::Begin(p) => open.push(p),
            Event::End(p) => match open.iter().position(|&q| q == p) {
                Some(i) => {
                    open.swap_remove(i);
                }
                None => out.push(at("a gesture ended that was not begun")),
            },
            Event::Perform(p, v) => {
                if !open.contains(&p) {
                    out.push(at("an edit outside its gesture"));
                }
                if !(0.0..=1.0).contains(&v) {
                    out.push(at("an edit outside 0–1"));
                }
            }
            Event::Prepare { .. } => prepared = Some(true),
            Event::Release => {
                if prepared == Some(false) {
                    out.push(at("released while not prepared"));
                }
                prepared = Some(false);
            }
            Event::Restore(_) | Event::Preset(_) => {}
            Event::Fault(_) => out.push(at("a fault")),
        }
    }
    out
}

/// Every gesture the trace opened is closed by its end: the host was
/// left with nothing "touched".
pub fn quiescent(trace: &Trace) -> Result<(), Violation> {
    let mut open: Vec<u32> = Vec::new();
    for r in &trace.0 {
        match r.event {
            Event::Begin(p) => open.push(p),
            Event::End(p) => open.retain(|&q| q != p),
            _ => {}
        }
    }
    match open.first() {
        None => Ok(()),
        Some(_) => Err(Violation { index: None, law: "a gesture left open" }),
    }
}

impl Trace {
    /// The faults in the trace, in order.
    pub fn faults(&self) -> Vec<Fault> {
        self.0.iter().filter_map(|r| if let Event::Fault(f) = r.event { Some(f) } else { None }).collect()
    }

    /// The events alone, in order (for asserting what a run did).
    pub fn events(&self) -> Vec<Event> {
        self.0.iter().map(|r| r.event).collect()
    }
}

// ----------------------------------------------------------------- text

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Event::Prepare { sample_rate, max_block } => write!(f, "prepare {sample_rate} {max_block}"),
            Event::Release => f.write_str("release"),
            Event::Begin(p) => write!(f, "begin {p}"),
            Event::Perform(p, v) => write!(f, "perform {p} {v}"),
            Event::End(p) => write!(f, "end {p}"),
            Event::Restore(ok) => f.write_str(if ok { "restore ok" } else { "restore refused" }),
            Event::Preset(n) => write!(f, "preset {n}"),
            Event::Fault(fault) => write!(f, "fault {}", fault.name()),
        }
    }
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @{} {}", self.index, self.block, self.event)
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.0 {
            writeln!(f, "{r}")?;
        }
        Ok(())
    }
}

impl FromStr for Event {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let w: Vec<&str> = s.split_whitespace().collect();
        let int = |i: usize| w.get(i).and_then(|x| x.parse::<u32>().ok()).ok_or(());
        Ok(match (w.first().copied().ok_or(())?, w.len()) {
            ("prepare", 3) => Event::Prepare { sample_rate: w[1].parse().map_err(|_| ())?, max_block: int(2)? },
            ("release", 1) => Event::Release,
            ("begin", 2) => Event::Begin(int(1)?),
            ("perform", 3) => Event::Perform(int(1)?, w[2].parse().map_err(|_| ())?),
            ("end", 2) => Event::End(int(1)?),
            ("restore", 2) => Event::Restore(match w[1] {
                "ok" => true,
                "refused" => false,
                _ => return Err(()),
            }),
            ("preset", 2) => Event::Preset(int(1)?),
            ("fault", 2) => Event::Fault(*Fault::ALL.iter().find(|f| f.name() == w[1]).ok_or(())?),
            _ => return Err(()),
        })
    }
}

/// Why a trace line did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceError {
    pub line: usize,
    pub text: String,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event trace line {}: cannot read `{}`", self.line, self.text)
    }
}

impl FromStr for Trace {
    type Err = TraceError;
    /// One record per line (`index @block event`); blank lines and `#`
    /// comments are skipped.
    fn from_str(text: &str) -> Result<Self, TraceError> {
        let mut records = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = || TraceError { line: i + 1, text: line.into() };
            let mut parts = line.splitn(3, ' ');
            let index = parts.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?;
            let block = parts.next().and_then(|x| x.strip_prefix('@')?.parse().ok()).ok_or_else(bad)?;
            let event = parts.next().ok_or_else(bad)?.parse().map_err(|()| bad())?;
            records.push(Record { index, block, event });
        }
        Ok(Trace(records))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::string::ToString;
    use alloc::vec;

    fn ring() -> Box<Events> {
        Box::new(Events::zeroed())
    }

    fn trace(events: &[Event]) -> Trace {
        Trace(events.iter().enumerate().map(|(i, &event)| Record { index: i as u64, block: 0, event }).collect())
    }

    #[test]
    fn every_event_reads_back_as_written() {
        let all = [
            Event::Prepare { sample_rate: 44_100.0, max_block: 512 },
            Event::Begin(7),
            Event::Perform(7, 0.25),
            Event::End(7),
            Event::Restore(true),
            Event::Restore(false),
            Event::Preset(3),
            Event::Fault(Fault::Unprepared),
            Event::Fault(Fault::Panic),
            Event::Release,
        ];
        let events = ring();
        for (i, &e) in all.iter().enumerate() {
            events.push(i as u64 * 10, e);
        }
        let mut rec = Recorder::new();
        assert_eq!(rec.drain(&events), all.len());
        assert_eq!(rec.trace().events(), all);
        assert_eq!(rec.records()[3].block, 30);
        let text = rec.trace().to_string();
        assert_eq!(text.parse::<Trace>(), Ok(rec.trace()), "the text form round-trips:\n{text}");
    }

    #[test]
    fn a_reader_that_falls_behind_knows_how_much_it_lost() {
        let events = ring();
        let mut rec = Recorder::new();
        events.push(0, Event::Begin(1));
        rec.drain(&events);
        for _ in 0..EVENT_SLOTS + 5 {
            events.push(0, Event::Preset(0));
        }
        rec.drain(&events);
        assert_eq!(rec.lost(), 5);
        assert_eq!(rec.records().len(), 1 + EVENT_SLOTS);
        assert!(check(&rec.trace()).iter().any(|v| v.law.contains("missing")), "the gap is reported");
    }

    #[test]
    fn a_clean_session_keeps_every_law() {
        let t = trace(&[
            Event::Prepare { sample_rate: 48_000.0, max_block: 64 },
            Event::Begin(1),
            Event::Begin(2),
            Event::Perform(1, 0.5),
            Event::Perform(2, 1.0),
            Event::End(1),
            Event::End(2),
            Event::Restore(true),
            Event::Release,
            Event::Prepare { sample_rate: 96_000.0, max_block: 64 },
        ]);
        assert_eq!(check(&t), vec![]);
        assert_eq!(quiescent(&t), Ok(()));
    }

    #[test]
    fn each_law_names_the_event_that_broke_it() {
        let laws = |events: &[Event]| -> Vec<(Option<u64>, &str)> { check(&trace(events)).into_iter().map(|v| (v.index, v.law)).collect() };
        assert_eq!(laws(&[Event::Begin(1), Event::Begin(1)]), [(Some(1), "a gesture begun while open")]);
        assert_eq!(laws(&[Event::End(4)]), [(Some(0), "a gesture ended that was not begun")]);
        assert_eq!(laws(&[Event::Perform(4, 0.5)]), [(Some(0), "an edit outside its gesture")]);
        assert_eq!(laws(&[Event::Begin(4), Event::Perform(4, 1.5)]), [(Some(1), "an edit outside 0–1")]);
        assert_eq!(laws(&[Event::Release, Event::Release]), [(Some(1), "released while not prepared")]);
        assert_eq!(laws(&[Event::Fault(Fault::BlockTooLong)]), [(Some(0), "a fault")]);
        assert!(quiescent(&trace(&[Event::Begin(3), Event::Perform(3, 0.0)])).is_err());
    }

    #[test]
    fn a_bad_line_is_named() {
        let e = "0 @0 begin 1\n# a note\n1 @0 bgein 1\n".parse::<Trace>().unwrap_err();
        assert_eq!((e.line, e.text.as_str()), (3, "1 @0 bgein 1"));
        assert!("0 @0 fault no-such-fault".parse::<Trace>().is_err());
    }
}
