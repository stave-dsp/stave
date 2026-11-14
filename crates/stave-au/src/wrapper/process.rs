//! The audio callback (`AudioUnitRender`'s selector): notes in, input
//! pulled from the host, audio through the plugin, the host's notifications
//! around it; and the calls that feed it — MIDI, reset, notifications.
//!
//! As in the CLAP and VST3 wrappers: no allocation, locks only tried
//! (a block that finds the configuration held is dropped and counted),
//! the plugin claimed for the block, in-place and null buffers staged
//! through the runtime's scratch, the bypass gate, the I/O taps.

use core::ffi::c_void;
use core::ptr;

use stave_core::diagnostics::ProcessFault;
use stave_core::event::NoteKind;
use stave_core::runtime::MAX_AUX_PORTS;
use stave_core::{Block, Event, EventKind, InputBuffer, NoteEvent, OutputBuffer, Transport};

use super::formats::{Source, MAX_CHANNELS};
use super::*;

/// Notes a block can carry; more are dropped and counted.
const NOTE_CAP: usize = 256;

const NO_EVENT: Event = Event {
    frame: 0,
    kind: EventKind::Note(NoteEvent {
        kind: NoteKind::Choke,
        key: -1,
        channel: -1,
        velocity: 0.0,
        note_id: -1,
    }),
};

/// Notes queued by `MusicDeviceMIDIEvent` for the next block.
pub(crate) struct Notes {
    events: [Event; NOTE_CAP],
    len: usize,
    dropped: u32,
}

impl Default for Notes {
    fn default() -> Self {
        Notes {
            events: [NO_EVENT; NOTE_CAP],
            len: 0,
            dropped: 0,
        }
    }
}

impl Notes {
    fn push(&mut self, event: Event) {
        match self.events.get_mut(self.len) {
            Some(slot) => {
                *slot = event;
                self.len += 1;
            }
            None => self.dropped = self.dropped.saturating_add(1),
        }
    }
}

/// A MIDI 1.0 channel message as a note event: note on (velocity 0 is
/// off), note off, and all-sound-off / all-notes-off (CC 120, 123) as a
/// choke of every note on the channel. Anything else is not a note.
pub(crate) fn note(status: u32, data1: u32, data2: u32) -> Option<NoteEvent> {
    let channel = (status & 0x0F) as i16;
    let (key, velocity) = ((data1 & 0x7F) as i16, f64::from(data2 & 0x7F) / 127.0);
    let kind = match status & 0xF0 {
        0x90 if data2 & 0x7F > 0 => NoteKind::On,
        0x90 | 0x80 => NoteKind::Off,
        0xB0 if matches!(data1, 120 | 123) => {
            return Some(NoteEvent { kind: NoteKind::Choke, key: -1, channel, velocity: 0.0, note_id: -1 });
        }
        _ => return None,
    };
    Some(NoteEvent { kind, key, channel, velocity, note_id: -1 })
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn midi_event<P: AuExport>(
    this: *mut c_void,
    status: u32,
    data1: u32,
    data2: u32,
    offset: u32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let Some(n) = note(status, data1, data2) else {
        return sys::noErr;
    };
    match inst.notes.try_lock() {
        Ok(mut notes) => notes.push(Event::note(offset as usize, n)),
        Err(_) => inst.fault(ProcessFault::EventsDropped, 1),
    }
    sys::noErr
}

/// System exclusive is not a note: accepted and ignored.
///
/// # Safety
/// AudioToolbox's contract.
pub(crate) unsafe extern "C" fn sysex(_this: *mut c_void, _data: *const u8, _len: u32) -> OSStatus {
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn reset<P: AuExport>(
    this: *mut c_void,
    _scope: sys::AudioUnitScope,
    _element: sys::AudioUnitElement,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    inst.guarded(
        "Reset",
        || sys::noErr,
        || {
            if let Ok(mut notes) = inst.notes.lock() {
                notes.len = 0;
            }
            match inst.plugin.claim_for_block() {
                Some(mut proc) => proc.plugin.reset(),
                None => inst.report("Reset", &CallError::ProcessorBusy),
            }
            sys::noErr
        },
    )
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn add_notify<P: AuExport>(
    this: *mut c_void,
    proc_: Option<sys::AURenderCallback>,
    refcon: *mut c_void,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let Some(proc_) = proc_ else {
        return sys::kAudio_ParamError;
    };
    let mut io = lock(&inst.io);
    if !io.notifies.iter().any(|&(p, r)| p as usize == proc_ as usize && r == refcon) {
        io.notifies.push((proc_, refcon));
    }
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn remove_notify<P: AuExport>(
    this: *mut c_void,
    proc_: Option<sys::AURenderCallback>,
    refcon: *mut c_void,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if let Some(proc_) = proc_ {
        lock(&inst.io).notifies.retain(|&(p, r)| !(p as usize == proc_ as usize && r == refcon));
    }
    sys::noErr
}

impl<P: AuExport> Instance<P> {
    /// Realtime-safe: counted, reported from the next main-thread call.
    pub(crate) fn fault(&self, fault: ProcessFault, count: u32) {
        self.diagnostics.faults.record(fault, count);
    }
}

/// A buffer list with room for an element's channels (the C type's
/// layout, its one-element array spelled out).
#[repr(C)]
struct List {
    count: u32,
    buffers: [sys::AuBuffer; MAX_CHANNELS],
}

impl List {
    /// `channels` buffers of `frames` over `data` (one channel after
    /// another, each `stride` long).
    fn over(data: &mut [f32], channels: usize, stride: usize, frames: usize) -> Self {
        let mut list = List {
            count: channels as u32,
            buffers: [sys::AuBuffer { mNumberChannels: 1, mDataByteSize: 0, mData: ptr::null_mut() }; MAX_CHANNELS],
        };
        for (c, b) in list.buffers.iter_mut().enumerate().take(channels) {
            b.mDataByteSize = (frames * 4) as u32;
            b.mData = data[c * stride..].as_mut_ptr().cast();
        }
        list
    }

    fn as_raw(&mut self) -> *mut sys::AudioBufferList {
        (self as *mut List).cast()
    }
}

/// Zero every buffer the host passed.
///
/// # Safety
/// `data` is a live buffer list whose non-null buffers are writable for
/// their stated size.
unsafe fn silence(data: *mut sys::AudioBufferList) {
    // SAFETY: the contract; the list holds `mNumberBuffers` buffers.
    unsafe {
        let n = (*data).mNumberBuffers as usize;
        let buffers = core::slice::from_raw_parts((*data).mBuffers.as_ptr(), n);
        for b in buffers.iter().filter(|b| !b.mData.is_null()) {
            ptr::write_bytes(b.mData.cast::<u8>(), 0, b.mDataByteSize as usize);
        }
    }
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; `time` and
/// `data` are this call's (the list's buffers writable for `frames`, or
/// null for the unit to supply).
pub(crate) unsafe extern "C" fn process<P: AuExport>(
    this: *mut c_void,
    flags: *mut u32,
    time: *const sys::AudioTimeStamp,
    bus: u32,
    frames: u32,
    data: *mut sys::AudioBufferList,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if data.is_null() || time.is_null() {
        return sys::kAudio_ParamError;
    }
    if bus != 0 {
        return sys::kAudioUnitErr_InvalidElement;
    }
    let status = inst.guard.run_realtime(
        "Render",
        // SAFETY: the list is this call's (the contract).
        || unsafe {
            silence(data);
            sys::noErr
        },
        // SAFETY: as above.
        || unsafe { process_block(inst, flags, time, frames as usize, data) },
    );
    inst.last_error.store(status, Ordering::Relaxed);
    status
}

/// # Safety
/// As [`process()`].
unsafe fn process_block<P: AuExport>(
    inst: &Instance<P>,
    flags: *mut u32,
    time: *const sys::AudioTimeStamp,
    frames: usize,
    data: *mut sys::AudioBufferList,
) -> OSStatus {
    if !inst.shared.activated() {
        return sys::kAudioUnitErr_Uninitialized;
    }
    if frames > inst.shared.max_frames() as usize {
        return sys::kAudioUnitErr_TooManyFramesToProcess;
    }
    let Ok(mut io) = inst.io.try_lock() else {
        inst.fault(ProcessFault::ConcurrentProcess, 1);
        // SAFETY: the contract.
        unsafe { silence(data) };
        return sys::noErr;
    };
    // SAFETY: the list holds `mNumberBuffers` buffers (the contract).
    let host_buffers = unsafe { core::slice::from_raw_parts_mut((*data).mBuffers.as_mut_ptr(), (*data).mNumberBuffers as usize) };
    if host_buffers.len() != io.output_channels as usize {
        return sys::kAudio_ParamError;
    }
    let base_flags = if flags.is_null() { 0 } else {
        // SAFETY: non-null, this call's.
        unsafe { *flags }
    };
    let notify = |io: &formats::Io, phase: u32| {
        let mut f = base_flags | phase;
        for &(proc_, refcon) in &io.notifies {
            // SAFETY: a notification the host registered, called with this
            // call's own time stamp and buffer list.
            unsafe { proc_(refcon, &mut f, time, 0, frames as u32, data) };
        }
    };
    notify(&io, sys::kAudioUnitRenderAction_PreRender);

    // Pull every input element from its source, into our buffers or the
    // provider's own.
    let main = usize::from(formats::has_main_input::<P>());
    let stride = io.frames;
    let mut inputs = [[ptr::null_mut::<f32>(); MAX_CHANNELS]; 1 + MAX_AUX_PORTS];
    for e in 0..io.inputs.len().min(1 + MAX_AUX_PORTS) {
        let channels = io.inputs[e] as usize;
        let source = io.sources[e];
        let mut list = List::over(&mut io.input[e], channels, stride, frames);
        let mut f = 0u32;
        let status = match source {
            Source::None if e < main => sys::kAudioUnitErr_NoConnection,
            Source::None => {
                io.input[e].fill(0.0);
                sys::noErr
            }
            Source::Callback(cb) => match cb.inputProc {
                // SAFETY: the host's input callback, with a list of
                // `channels` buffers of `frames` it may fill or replace.
                Some(pull) => unsafe { pull(cb.inputProcRefCon, &mut f, time, e as u32, frames as u32, list.as_raw()) },
                None => sys::kAudioUnitErr_NoConnection,
            },
            // SAFETY: the unit the host connected, asked for `frames`
            // into the same list.
            Source::Connection(c) => unsafe {
                sys::AudioUnitRender(c.sourceAudioUnit, &mut f, time, c.sourceOutputNumber, frames as u32, list.as_raw())
            },
        };
        if status != sys::noErr {
            return status;
        }
        for (dst, b) in inputs[e].iter_mut().zip(&list.buffers).take(channels) {
            *dst = b.mData.cast();
        }
    }
    // Output: the host's buffers, or ours where it passed none.
    let mut outputs = [ptr::null_mut::<f32>(); MAX_CHANNELS];
    for (c, b) in host_buffers.iter_mut().enumerate().take(MAX_CHANNELS) {
        if b.mData.is_null() {
            b.mData = io.output[c * stride..].as_mut_ptr().cast();
        }
        b.mDataByteSize = (frames * 4) as u32;
        outputs[c] = b.mData.cast();
    }
    let outputs = &outputs[..host_buffers.len().min(MAX_CHANNELS)];
    let main_in: &[*mut f32] = if main == 1 { &inputs[0][..io.inputs[0] as usize] } else { &[] };
    let aux_count = io.inputs.len().saturating_sub(main).min(MAX_AUX_PORTS);
    let mut aux: [&[*mut f32]; MAX_AUX_PORTS] = [&[]; MAX_AUX_PORTS];
    for (a, list) in aux.iter_mut().enumerate().take(aux_count) {
        *list = &inputs[main + a][..io.inputs[main + a] as usize];
    }

    let mut events = [NO_EVENT; NOTE_CAP];
    let count = take_notes(inst, &mut events, frames);
    let transport = transport(io.host.as_ref(), inst.shared.sample_rate());
    let muted = run(inst, main_in, outputs, &aux[..aux_count], frames, &events[..count], transport);
    if muted && !flags.is_null() {
        // SAFETY: non-null, this call's.
        unsafe { *flags |= sys::kAudioUnitRenderAction_OutputIsSilence };
    }
    notify(&io, sys::kAudioUnitRenderAction_PostRender);
    sys::noErr
}

/// The queued notes due in this block into `dst`, ordered by frame;
/// returns how many.
fn take_notes<P: AuExport>(inst: &Instance<P>, dst: &mut [Event], frames: usize) -> usize {
    let Ok(mut notes) = inst.notes.try_lock() else {
        return 0;
    };
    let n = notes.len.min(dst.len());
    for (d, e) in dst.iter_mut().zip(&notes.events[..n]) {
        *d = Event { frame: e.frame.min(frames.saturating_sub(1)), ..*e };
    }
    let dropped = core::mem::take(&mut notes.dropped);
    notes.len = 0;
    drop(notes);
    inst.fault(ProcessFault::EventsDropped, dropped);
    // In place and stable (the standard sort may allocate).
    for i in 1..n {
        let mut j = i;
        while j > 0 && dst[j - 1].frame > dst[j].frame {
            dst.swap(j - 1, j);
            j -= 1;
        }
    }
    n
}

/// The host's transport, through its callbacks.
fn transport(host: Option<&sys::HostCallbackInfo>, rate: f32) -> Option<Transport> {
    let host = host?;
    let (mut beat, mut tempo) = (0.0f64, 0.0f64);
    let tempo_ok = host
        .beatAndTempoProc
        // SAFETY: the host's callback with its own user data and two
        // f64 out-pointers.
        .is_some_and(|f| unsafe { f(host.hostUserData, &mut beat, &mut tempo) } == sys::noErr);
    let (mut playing, mut recording, mut changed, mut cycling) = (0u8, 0u8, 0u8, 0u8);
    let (mut sample, mut start, mut end) = (0.0f64, 0.0f64, 0.0f64);
    let status = match (host.transportStateProc2, host.transportStateProc) {
        // SAFETY: the host's callback with its user data and out-pointers
        // of the types it names.
        (Some(f), _) => Some(unsafe {
            f(host.hostUserData, &mut playing, &mut recording, &mut changed, &mut sample, &mut cycling, &mut start, &mut end)
        }),
        // SAFETY: as above.
        (None, Some(f)) => Some(unsafe {
            f(host.hostUserData, &mut playing, &mut changed, &mut sample, &mut cycling, &mut start, &mut end)
        }),
        (None, None) => None,
    };
    let state_ok = status == Some(sys::noErr);
    Some(Transport {
        playing: state_ok && playing != 0,
        tempo_bpm: (tempo_ok && tempo > 0.0).then_some(tempo),
        ppq_position: tempo_ok.then_some(beat),
        song_pos_seconds: (state_ok && rate > 0.0).then(|| sample / f64::from(rate)),
    })
}

/// One block through the plugin; returns whether the output is muted
/// (a breakpoint's pause).
fn run<P: AuExport>(
    inst: &Instance<P>,
    input: &[*mut f32],
    output: &[*mut f32],
    aux: &[&[*mut f32]],
    frames: usize,
    events: &[Event],
    transport: Option<Transport>,
) -> bool {
    let _ftz = stave_core::runtime::ScopedFtz::engage();
    let Some(mut proc) = inst.plugin.claim_for_block() else {
        // SAFETY: the output channels are writable for `frames`.
        unsafe { stave_core::runtime::zero_tail(output, 0, frames) };
        inst.fault(ProcessFault::ConcurrentProcess, 1);
        return false;
    };
    // SAFETY: every pointer is valid for `frames` (the host's, ours, or
    // the input provider's for this call).
    let Some(borrows) = (unsafe { proc.split_for_block(input, output, aux, frames) }) else {
        inst.fault(ProcessFault::Unprepared, 1);
        // SAFETY: as above.
        unsafe { stave_core::runtime::zero_tail(output, 0, frames) };
        return false;
    };
    let (plugin, gate, io) = (borrows.plugin, borrows.bypass, borrows.io);
    if io.truncated {
        inst.fault(ProcessFault::BlockTruncated, 1);
    }
    let run = io.frames;
    // The host's bypass (an effect's BypassEffect) or the plugin's own
    // param, through the same click-free, latency-matched gate.
    let param_bypass = inst.bypass_id.and_then(|_| inst.params.bypass());
    let bypassed = inst.effect_bypass.load(Ordering::Relaxed) || param_bypass.is_some_and(|p| p.value() >= 0.5);
    let gated = param_bypass.is_some() || !input.is_empty();
    if gated {
        // SAFETY: `split_for_block` lists are valid for `run` frames.
        unsafe { gate.feed(io.input, run) };
    }
    // SAFETY: `split_for_block` lists have no nulls, are valid for `run`
    // frames, and staging removed any input/output aliasing.
    let (input_buf, output_buf) = unsafe { (InputBuffer::from_raw(io.input, run), OutputBuffer::from_raw(io.output, run)) };
    let mut aux_bufs = [const { InputBuffer::empty() }; MAX_AUX_PORTS];
    for (a, &ptrs) in aux_bufs.iter_mut().zip(&io.aux).take(aux.len()) {
        // SAFETY: as above.
        *a = unsafe { InputBuffer::from_raw(ptrs, run) };
    }
    let mut block = Block::new(inst.shared.sample_rate(), run, input_buf, output_buf)
        .with_aux(&aux_bufs[..aux.len()])
        .with_transport(transport)
        .with_params(stave_core::ParamBlock::empty())
        .with_events(events);
    // SAFETY: `split_for_block` lists are valid for `run` frames.
    let input_peaks = unsafe { stave_core::runtime::peaks(io.input, run) };
    let block_id = inst.telemetry.tick_block();
    let started = std::time::Instant::now();
    plugin.process(&mut block);
    let cpu = started.elapsed().as_secs_f64() / (frames as f64 / f64::from(inst.shared.sample_rate()));
    if gated {
        // SAFETY: the output channels are writable for `run`.
        unsafe { gate.mix(bypassed, output, run) };
    }
    // SAFETY: the output channels are writable for `frames`.
    unsafe { stave_core::runtime::zero_tail(output, run, frames) };
    drop(proc);
    // SAFETY: the output channels are valid for `frames`.
    let output_peaks = unsafe { stave_core::runtime::peaks(output, frames) };
    inst.taps.publish_peaks(input_peaks, output_peaks);
    inst.taps.cpu.set(cpu);
    publish_capture(&inst.telemetry, output, frames, block_id);
    // SAFETY: the output channels are writable for `frames`; the
    // plugin's borrows of them ended with `process`.
    unsafe { stave_core::runtime::mute_if_paused(&inst.telemetry, output, frames) }
}

/// The block's output as the telemetry's capture (for a face's scope),
/// unless the plugin writes its own.
fn publish_capture(telemetry: &Telemetry, output: &[*mut f32], frames: usize, block_id: u64) {
    use stave_core::telemetry::CAPTURE_CHANNELS;
    if telemetry.plugin_owns_capture() {
        return;
    }
    let chans = output.len().min(CAPTURE_CHANNELS);
    let mut capture: [&[f32]; CAPTURE_CHANNELS] = [&[]; CAPTURE_CHANNELS];
    for (c, &p) in capture.iter_mut().zip(output).take(chans) {
        if !p.is_null() {
            // SAFETY: the output channels are valid for `frames`; the
            // plugin's borrows of them have ended.
            *c = unsafe { core::slice::from_raw_parts(p, frames) };
        }
    }
    if chans > 0 {
        telemetry.write_capture(&capture[..chans], block_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_channel_messages_become_notes() {
        let on = note(0x91, 60, 127).unwrap();
        assert_eq!((on.kind, on.key, on.channel, on.velocity), (NoteKind::On, 60, 1, 1.0));
        assert_eq!(note(0x90, 60, 0).unwrap().kind, NoteKind::Off, "velocity 0 is an off");
        assert_eq!(note(0x80, 61, 64).unwrap().kind, NoteKind::Off);
        assert_eq!(note(0xB0, 123, 0).unwrap().kind, NoteKind::Choke);
        assert_eq!(note(0xB0, 7, 100), None, "a volume CC is not a note");
        assert_eq!(note(0xE0, 0, 64), None, "pitch bend is not a note");
    }
}
