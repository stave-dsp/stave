//! One plugin instance as the standalone runs it: the same runtime the
//! format wrappers drive (`stave_core::runtime`: the processor claim,
//! the scratch, the bypass gate, the wrapper's taps, the state chunk),
//! with the audio device in the host's seat.
//!
//! [`Session`] is shared: the audio callback calls [`Session::process`],
//! the window's controller edits the parameter table and saves state,
//! the MIDI thread queues notes. Every field is either immutable or
//! synchronised (atomics, the claim, a mutex the audio thread never
//! takes).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use stave_core::diagnostics::{CallError, Diagnostics, ProcessFault};
use stave_core::params::Params;
use stave_core::runtime::{self, PanicGuard, PluginCell, SharedState, WrapperTaps};
use stave_core::state::StateStore;
use stave_core::{
    Block, Environment, Event, EventKind, GestureSink, InputBuffer, Layout, NoteEvent, NoteKind,
    OutputBuffer, ParamId, PluginController, Preset, Telemetry,
};
use stave_face::Face;

use crate::Standalone;

/// Notes the MIDI thread queues for the next block (more are dropped).
const NOTE_SLOTS: usize = 256;

/// The erased surface the device and the window drive, so neither is
/// generic over the plugin.
pub(crate) trait Instance: Send + Sync {
    /// The plugin's main input and output channel counts.
    fn channels(&self) -> (usize, usize);
    /// Whether the plugin plays notes (instruments, note effects, and
    /// layouts without an audio input).
    fn takes_notes(&self) -> bool;
    /// Prepare for `sample_rate` and blocks of up to `max_frames`.
    /// Never while a device is running the instance.
    fn prepare(&self, sample_rate: f32, max_frames: usize);
    /// One block, realtime.
    ///
    /// # Safety
    /// Every pointer in `input` and `output` is valid for `frames`
    /// samples, `output`'s writable, none aliasing another.
    unsafe fn process(&self, input: &[*mut f32], output: &[*mut f32], frames: usize);
    /// Queue a raw MIDI message for the next block (any thread).
    fn midi(&self, message: [u8; 3]);
    /// The state chunk: parameters and the plugin's own state.
    fn save(&self) -> Vec<u8>;
    /// Apply a state chunk; on a decode error nothing changes.
    fn load(&self, bytes: &[u8]) -> Result<(), CallError>;
    /// The editor contract over the parameter table and telemetry.
    fn controller(&self, sink: Box<dyn GestureSink>) -> PluginController;
    /// The plugin's face, if it has one.
    fn face(&self) -> Option<Face>;
    /// Every parameter's value, in table order (to see an edit without
    /// claiming the processor).
    fn values(&self, out: &mut Vec<f64>);
    /// Log the faults the audio thread recorded since the last call.
    fn report_faults(&self);
}

/// A lock-free single-producer single-consumer queue of MIDI messages:
/// the MIDI thread pushes, the audio thread pops. A full queue drops.
pub(crate) struct NoteQueue {
    slots: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl NoteQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        NoteQueue {
            slots: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// False when full (the message is dropped).
    pub(crate) fn push(&self, message: [u8; 3]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= self.slots.len() {
            return false;
        }
        let word = u32::from_le_bytes([message[0], message[1], message[2], 0]);
        self.slots[tail % self.slots.len()].store(word, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    pub(crate) fn pop(&self) -> Option<[u8; 3]> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let [a, b, c, _] = self.slots[head % self.slots.len()].load(Ordering::Relaxed).to_le_bytes();
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some([a, b, c])
    }
}

/// A raw MIDI message as a stave note event (note on with velocity 0 is
/// a note off), or `None` for anything else.
pub(crate) fn note_of(message: [u8; 3]) -> Option<NoteEvent> {
    let [status, key, velocity] = message;
    let channel = i16::from(status & 0x0F);
    let kind = match (status & 0xF0, velocity) {
        (0x90, 1..) => NoteKind::On,
        (0x90, 0) | (0x80, _) => NoteKind::Off,
        _ => return None,
    };
    Some(NoteEvent {
        kind,
        key: i16::from(key & 0x7F),
        channel,
        velocity: f64::from(velocity & 0x7F) / 127.0,
        note_id: -1,
    })
}

pub(crate) struct Session<P: Standalone> {
    shared: SharedState,
    plugin: PluginCell<P>,
    params: Arc<dyn Params>,
    presets: Arc<[Preset]>,
    face: Option<Face>,
    layout: Layout,
    store: Mutex<StateStore>,
    telemetry: Arc<Telemetry>,
    taps: WrapperTaps,
    diagnostics: Diagnostics,
    guard: PanicGuard,
    bypass: Option<ParamId>,
    notes: NoteQueue,
}

impl<P: Standalone> Session<P> {
    /// A fresh instance, or why its parameter table is unusable.
    pub(crate) fn new() -> Result<Self, String> {
        let telemetry = Telemetry::new();
        let taps = WrapperTaps::new(&telemetry);
        let floor = [WrapperTaps::FLOOR_DB; 3];
        taps.publish_peaks(floor, floor);
        let plugin = P::new(&telemetry);
        let params = plugin.params();
        params.validate().map_err(|e| format!("param table invalid: {e}"))?;
        let (face, presets) = (plugin.face(), plugin.presets());
        let diagnostics = Diagnostics::new(&P::INFO);
        diagnostics.watch(&telemetry);
        let bypass = params.bypass().map(|p| p.meta().id);
        Ok(Session {
            shared: SharedState::new(48_000.0),
            plugin: PluginCell::new(plugin),
            params,
            presets,
            face,
            layout: layout::<P>(),
            store: Mutex::new(StateStore::default()),
            telemetry,
            taps,
            diagnostics,
            guard: PanicGuard::default(),
            bypass,
            notes: NoteQueue::new(NOTE_SLOTS),
        })
    }

    fn fault(&self, fault: ProcessFault, count: u32) {
        // Recorded now, logged by `report_faults` (the saver thread).
        self.diagnostics.faults.record(fault, count);
    }

    fn block(&self, input: &[*mut f32], output: &[*mut f32], frames: usize) {
        let mut events = [Event::note(
            0,
            NoteEvent { kind: NoteKind::Choke, key: -1, channel: -1, velocity: 0.0, note_id: -1 },
        ); NOTE_SLOTS];
        let mut count = 0;
        while let Some(message) = self.notes.pop() {
            if let Some(note) = note_of(message) {
                events[count] = Event { frame: 0, kind: EventKind::Note(note) };
                count += 1;
                if count == events.len() {
                    break;
                }
            }
        }
        let _ftz = runtime::ScopedFtz::engage();
        let Some(mut proc) = self.plugin.claim_for_block() else {
            // A state save holds the processor: silence, not a race.
            // SAFETY: the caller's output pointers are writable for `frames`.
            unsafe { runtime::zero_tail(output, 0, frames) };
            self.fault(ProcessFault::ConcurrentProcess, 1);
            return;
        };
        // SAFETY: the caller's pointers are valid for `frames` (`process`).
        let Some(borrows) = (unsafe { proc.split_for_block(input, output, &[], frames) }) else {
            self.fault(ProcessFault::Unprepared, 1);
            // SAFETY: as above.
            unsafe { runtime::zero_tail(output, 0, frames) };
            return;
        };
        let (plugin, gate, io) = (borrows.plugin, borrows.bypass, borrows.io);
        if io.truncated {
            self.fault(ProcessFault::BlockTruncated, 1);
        }
        let run = io.frames;
        let bypass = self.bypass.and_then(|_| self.params.bypass());
        let bypassed = bypass.is_some_and(|p| p.value() >= 0.5);
        if bypass.is_some() {
            // SAFETY: `split_for_block` lists are valid for `run` frames.
            unsafe { gate.feed(io.input, run) };
        }
        // SAFETY: `split_for_block` lists have no nulls, are valid for
        // `run` frames, and never alias.
        let (inputs, outputs) = unsafe { (InputBuffer::from_raw(io.input, run), OutputBuffer::from_raw(io.output, run)) };
        let mut block = Block::new(self.shared.sample_rate(), run, inputs, outputs).with_events(&events[..count]);
        // SAFETY: as above.
        let input_peaks = unsafe { runtime::peaks(io.input, run) };
        let block_id = self.telemetry.tick_block();
        let started = std::time::Instant::now();
        plugin.process(&mut block);
        let cpu = started.elapsed().as_secs_f64() / (frames as f64 / f64::from(self.shared.sample_rate()));
        if bypass.is_some() {
            // SAFETY: the output channels are writable for `run`.
            unsafe { gate.mix(bypassed, output, run) };
        }
        // SAFETY: the caller's output channels are writable for `frames`.
        unsafe { runtime::zero_tail(output, run, frames) };
        drop(proc);
        // SAFETY: as above.
        let output_peaks = unsafe { runtime::peaks(output, frames) };
        self.taps.publish_peaks(input_peaks, output_peaks);
        self.taps.cpu.set(cpu);
        publish_capture(&self.telemetry, output, frames, block_id);
        // SAFETY: as above; the plugin's borrows ended with `process`.
        unsafe { runtime::mute_if_paused(&self.telemetry, output, frames) };
    }
}

/// The layout the standalone serves: the first stereo-out one the plugin
/// declares, else its first.
fn layout<P: Standalone>() -> Layout {
    P::LAYOUTS
        .iter()
        .find(|l| l.output_channels() == 2)
        .or_else(|| P::LAYOUTS.first())
        .copied()
        .unwrap_or(Layout::STEREO)
}

/// The block's output as the telemetry's capture (a face's scope),
/// unless the plugin writes its own.
fn publish_capture(telemetry: &Telemetry, output: &[*mut f32], frames: usize, block_id: u64) {
    use stave_core::telemetry::CAPTURE_CHANNELS;
    if telemetry.plugin_owns_capture() {
        return;
    }
    let chans = output.len().min(CAPTURE_CHANNELS);
    let mut capture: [&[f32]; CAPTURE_CHANNELS] = [&[]; CAPTURE_CHANNELS];
    for (c, &ptr) in capture.iter_mut().zip(output).take(chans) {
        // SAFETY: the caller's output channels are valid for `frames`
        // and no longer borrowed by the plugin.
        *c = unsafe { core::slice::from_raw_parts(ptr, frames) };
    }
    if chans > 0 {
        telemetry.write_capture(&capture[..chans], block_id);
    }
}

impl<P: Standalone> Instance for Session<P> {
    fn channels(&self) -> (usize, usize) {
        (self.layout.input_channels(), self.layout.output_channels())
    }

    fn takes_notes(&self) -> bool {
        use stave_core::Feature as F;
        P::INFO.features.iter().any(|f| matches!(f, F::Instrument | F::Synthesizer | F::Sampler | F::Drum | F::NoteEffect))
            || self.layout.input.is_none()
    }

    fn prepare(&self, sample_rate: f32, max_frames: usize) {
        let Some(mut proc) = self.plugin.claim_with_retry(runtime::state::STATE_CLAIM_WAIT) else {
            self.diagnostics.report("prepare", &CallError::ProcessorBusy);
            return;
        };
        self.shared.set_sample_rate(sample_rate);
        self.shared.set_max_frames(max_frames as u32);
        let env = Environment::new(sample_rate, max_frames).with_layout(self.layout);
        let in_channels = P::LAYOUTS.iter().map(|l| l.input_channels()).max().unwrap_or(0);
        let latency = self.guard.run(&self.diagnostics, "prepare", || 0, || proc.prepare(&env, in_channels));
        self.shared.set_latency(latency);
        self.shared.set_activated(true);
        self.diagnostics.event(stave_core::telemetry::Event::Prepare { sample_rate, max_block: max_frames as u32 });
    }

    unsafe fn process(&self, input: &[*mut f32], output: &[*mut f32], frames: usize) {
        if frames == 0 {
            return;
        }
        self.guard.run_realtime(
            "process",
            // SAFETY: the caller's output pointers are writable for `frames`.
            || unsafe { runtime::zero_tail(output, 0, frames) },
            || self.block(input, output, frames),
        );
    }

    fn midi(&self, message: [u8; 3]) {
        if !self.notes.push(message) {
            self.fault(ProcessFault::EventsDropped, 1);
        }
    }

    fn save(&self) -> Vec<u8> {
        runtime::state::save(&self.plugin, self.params.as_ref(), &self.store, &self.diagnostics)
    }

    fn load(&self, bytes: &[u8]) -> Result<(), CallError> {
        runtime::state::load(bytes, &self.plugin, self.params.as_ref(), &self.store)
    }

    fn controller(&self, sink: Box<dyn GestureSink>) -> PluginController {
        PluginController::new(self.params.clone(), self.telemetry.clone(), self.presets.clone(), sink)
    }

    fn face(&self) -> Option<Face> {
        self.face.clone()
    }

    fn report_faults(&self) {
        self.diagnostics.report_faults();
    }

    fn values(&self, out: &mut Vec<f64>) {
        out.clear();
        out.extend((0..self.params.count()).filter_map(|i| self.params.by_index(i)).map(|p| p.value()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    /// Notes the test plugin has seen (one test plays it).
    static NOTES: AtomicU32 = AtomicU32::new(0);

    /// Halves its input; counts note-ons; saves what it last loaded.
    struct Half {
        extra: Vec<u8>,
    }

    impl stave_core::runtime::Erased for Half {
        const INFO: stave_core::Info = stave_core::Info { id: "test.half", name: "Half", vendor: "test", ..stave_core::Info::EFFECT };
        fn new(_telemetry: &Telemetry) -> Self {
            Half { extra: b"fresh".to_vec() }
        }
        fn process(&mut self, block: &mut Block<'_>) {
            let on = |e: &&Event| matches!(e.kind, EventKind::Note(NoteEvent { kind: NoteKind::On, .. }));
            NOTES.fetch_add(block.events().iter().filter(on).count() as u32, Ordering::Relaxed);
            for c in 0..2 {
                let x: Vec<f32> = block.input().channel(c).to_vec();
                for (y, x) in block.output().channel_mut(c).iter_mut().zip(x) {
                    *y = x * 0.5;
                }
            }
        }
        fn save(&self) -> Vec<u8> {
            self.extra.clone()
        }
        fn load(&mut self, bytes: &[u8], _version: u32) {
            self.extra = bytes.to_vec();
        }
    }

    impl Standalone for Half {}

    fn run(s: &dyn Instance, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![1.0f32; frames], vec![-1.0f32; frames]);
        let (mut ol, mut or) = (vec![9.0f32; frames], vec![9.0f32; frames]);
        let input = [l.as_mut_ptr(), r.as_mut_ptr()];
        let output = [ol.as_mut_ptr(), or.as_mut_ptr()];
        // SAFETY: four separate vectors of `frames` samples each.
        unsafe { s.process(&input, &output, frames) };
        ol.into_iter().chain(or).collect()
    }

    #[test]
    fn an_unprepared_instance_is_silent_and_a_prepared_one_runs() {
        let s = Session::<Half>::new().unwrap();
        assert_eq!(s.channels(), (2, 2));
        assert!(!s.takes_notes());
        assert!(run(&s, 4).iter().all(|&y| y == 0.0));
        s.prepare(48_000.0, 64);
        assert_eq!(run(&s, 4), [0.5, 0.5, 0.5, 0.5, -0.5, -0.5, -0.5, -0.5]);
    }

    #[test]
    fn midi_note_ons_reach_the_next_block_and_other_messages_do_not() {
        let s = Session::<Half>::new().unwrap();
        s.prepare(48_000.0, 64);
        for m in [[0x90, 60, 100], [0x90, 60, 0], [0x80, 61, 0], [0xB0, 1, 64], [0x91, 62, 1]] {
            s.midi(m);
        }
        run(&s, 8);
        assert_eq!(NOTES.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn state_round_trips_through_a_fresh_instance() {
        let a = Session::<Half>::new().unwrap();
        a.plugin.claim().unwrap().plugin.extra = b"edited".to_vec();
        let chunk = a.save();
        let b = Session::<Half>::new().unwrap();
        b.load(&chunk).unwrap();
        assert_eq!(b.plugin.claim().unwrap().plugin.extra, b"edited");
        assert!(b.load(b"not a state chunk").is_err());
    }

    #[test]
    fn note_of_reads_on_off_and_ignores_the_rest() {
        assert!(matches!(note_of([0x93, 64, 127]), Some(NoteEvent { kind: NoteKind::On, key: 64, channel: 3, .. })));
        assert!(matches!(note_of([0x90, 64, 0]), Some(NoteEvent { kind: NoteKind::Off, .. })));
        assert!(note_of([0xE0, 0, 64]).is_none());
        let q = NoteQueue::new(2);
        assert!(q.push([1, 2, 3]) && q.push([4, 5, 6]) && !q.push([7, 8, 9]));
        assert_eq!((q.pop(), q.pop(), q.pop()), (Some([1, 2, 3]), Some([4, 5, 6]), None));
    }
}
