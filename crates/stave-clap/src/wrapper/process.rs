//! `process`: events in, audio through the plugin, gestures out.

use super::*;

pub(super) unsafe extern "C" fn plugin_process<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    process: *const sys::ClapProcess,
) -> sys::ClapProcessStatus {
    // SAFETY: host contract (see `host`): `plugin` came from
    // `create_plugin`, `process` is this call's.
    let (inst, process) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            Process::new(process),
        )
    };
    let Some(process) = process else {
        inst.fault(ProcessFault::MissingProcessData, 1);
        return sys::CLAP_PROCESS_ERROR;
    };
    // The realtime guard: a panic here is recorded, and logged from
    // `on_main_thread`, which this asks the host for.
    let status = inst.guard.run_realtime(
        "process",
        || {
            process.silence_output();
            sys::CLAP_PROCESS_ERROR
        },
        || process_block(inst, &process),
    );
    if inst.guard.unreported() {
        if let Some(host) = inst.host {
            host.request_callback();
        }
    }
    status
}

fn process_block<P: ClapExport>(
    inst: &Instance<P>,
    process: &Process<'_>,
) -> sys::ClapProcessStatus {
    let frames = process.frames();

    // Serve the host-selected configuration.
    let cfg = inst.config.load(core::sync::atomic::Ordering::Acquire);
    let want = P::LAYOUTS[cfg.min(P::LAYOUTS.len() - 1)];

    // A plugin without inputs still gets a (zero-channel) input buffer.
    let input_ptrs = match want.input_channels() {
        0 => &[],
        _ => process.input(0).unwrap_or(&[]),
    };
    let Some(output_ptrs) = process.output() else {
        inst.fault(ProcessFault::MissingOutput, 1);
        return sys::CLAP_PROCESS_ERROR;
    };

    // Stack buffers: no heap traffic on the audio thread. Overflow is
    // dropped and counted.
    let mut events_buf: [Event; 256] = [Event {
        frame: 0,
        kind: EventKind::Note(NoteEvent {
            kind: NoteKind::Choke,
            key: -1,
            channel: -1,
            velocity: 0.0,
            note_id: -1,
        }),
    }; 256];
    let mut param_buf = [stave_core::ParamChange {
        id: ParamId(0),
        frame: 0,
        value: 0.0,
        kind: stave_core::ParamChangeKind::Value,
    }; 256];
    let (mut param_count, mut params_dropped, mut event_count) = (0, 0, 0);
    if let Some(events) = process.in_events() {
        (param_count, params_dropped) =
            apply_param_events(&events, inst.params.as_ref(), &mut param_buf);
        // In place: the standard stable sort may allocate.
        stave_core::runtime::sort_param_changes(&mut param_buf[..param_count]);
        let events_dropped;
        (event_count, events_dropped) = translate_input_events(&events, &mut events_buf);
        inst.fault(ProcessFault::EventsDropped, events_dropped);
    }
    inst.fault(ProcessFault::ParamChangesDropped, params_dropped);

    // Robustness guards: zero-length succeeds silently;
    // beyond declared max_frames is refused (buffers are sized to the
    // host's own declaration).
    if frames == 0 {
        emit_gestures(&inst.gestures, process.out_events());
        return sys::CLAP_PROCESS_CONTINUE;
    }
    let max_frames = inst.shared.max_frames() as usize;
    if max_frames > 0 && frames > max_frames {
        inst.fault(ProcessFault::BlockTooLong, 1);
        return sys::CLAP_PROCESS_ERROR;
    }

    // FTZ/DAZ backstop for the whole block (manual flushes in
    // stave-dsp remain primary; see stave_core::runtime::ScopedFtz).
    let _ftz = stave_core::runtime::ScopedFtz::engage();

    // Exclusive processor access for the block. Held elsewhere — a
    // `state.save`/`load` on the main thread (legal while processing) or
    // an overlapping call (not) — the block is dropped, not raced:
    // silence, and the status an unprepared block gets while inactive.
    let Some(mut proc) = inst.plugin.claim_for_block() else {
        // SAFETY: the host's output channels are writable for `frames`.
        unsafe { stave_core::runtime::zero_tail(output_ptrs, 0, frames) };
        if !inst.shared.activated() {
            inst.fault(ProcessFault::Unprepared, 1);
            return sys::CLAP_PROCESS_ERROR;
        }
        inst.fault(ProcessFault::ConcurrentProcess, 1);
        return sys::CLAP_PROCESS_CONTINUE;
    };

    // Aux inputs: declared aux ports follow the main input in
    // declaration order. A missing port becomes a zero-channel buffer —
    // the `Aux` contract (plugin fully functional without).
    const MAX_AUX: usize = stave_core::runtime::MAX_AUX_PORTS;
    let n_aux = want.aux.len().min(MAX_AUX);
    let main_ports = usize::from(want.input_channels() > 0);
    let mut aux_ptrs: [&[*mut f32]; MAX_AUX] = [&[]; MAX_AUX];
    for (a, ptrs) in aux_ptrs.iter_mut().enumerate().take(n_aux) {
        *ptrs = process.input(main_ports + a).unwrap_or(&[]);
    }

    // Channel lists the buffers can take: in-place inputs staged through
    // preallocated scratch (the buffers' no-alias contract), null
    // channels replaced by zeroed scratch.
    // SAFETY: the host's channel pointers are valid for `frames` (clap.h).
    let Some(borrows) =
        (unsafe { proc.split_for_block(input_ptrs, output_ptrs, &aux_ptrs[..n_aux], frames) })
    else {
        // Not prepared (no activate): the plugin never runs unprepared.
        inst.fault(ProcessFault::Unprepared, 1);
        // SAFETY: the host's output channels are writable for `frames`.
        unsafe { stave_core::runtime::zero_tail(output_ptrs, 0, frames) };
        return sys::CLAP_PROCESS_ERROR;
    };
    let (plugin, gate, io) = (borrows.plugin, borrows.bypass, borrows.io);
    if io.truncated {
        inst.fault(ProcessFault::BlockTruncated, 1);
    }
    let run = io.frames;

    // The bypass param from the table's cached index (no scan).
    let bypass = inst.bypass_id.and_then(|_| inst.params.bypass());
    let bypassed = bypass.is_some_and(|p| p.value() >= 0.5);
    if bypass.is_some() {
        // SAFETY: `split_for_block` lists are valid for `run` frames.
        unsafe { gate.feed(io.input, run) };
    }

    // SAFETY: `split_for_block` lists have no nulls, are valid for `run`
    // frames, and staging removed any input/output aliasing.
    let (input, output) = unsafe {
        (
            InputBuffer::from_raw(io.input, run),
            OutputBuffer::from_raw(io.output, run),
        )
    };
    let mut aux_bufs = [const { InputBuffer::empty() }; MAX_AUX];
    for (aux, &ptrs) in aux_bufs.iter_mut().zip(&io.aux).take(n_aux) {
        // SAFETY: as above.
        *aux = unsafe { InputBuffer::from_raw(ptrs, run) };
    }

    let transport = process.transport().map(|t| Transport {
        playing: t.flags & sys::CLAP_TRANSPORT_IS_PLAYING != 0,
        tempo_bpm: (t.flags & sys::CLAP_TRANSPORT_HAS_TEMPO != 0).then_some(t.tempo),
        ppq_position: (t.flags & sys::CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0)
            .then(|| t.song_pos_beats as f64 / sys::CLAP_SECTIME_UNIT),
        song_pos_seconds: (t.flags & sys::CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0)
            .then(|| t.song_pos_seconds as f64 / sys::CLAP_SECTIME_UNIT),
    });

    let mut block = Block::new(inst.shared.sample_rate(), run, input, output)
        .with_aux(&aux_bufs[..n_aux])
        .with_transport(transport)
        .with_params(stave_core::ParamBlock::with_dropped(&param_buf[..param_count], params_dropped))
        .with_events(&events_buf[..event_count]);

    // The editor's rails: input peaks before the plugin, output after.
    // SAFETY: `split_for_block` lists are valid for `run` frames.
    let input_peaks = unsafe { stave_core::runtime::peaks(io.input, run) };
    let block_id = inst.telemetry.tick_block();
    // The plugin's own work, timed (Instant: no alloc, no lock).
    let started = std::time::Instant::now();
    plugin.process(&mut block);
    let cpu = started.elapsed().as_secs_f64() / (frames as f64 / f64::from(inst.shared.sample_rate()));
    if bypass.is_some() {
        // SAFETY: the output channels are writable for `run` (clap.h).
        unsafe { gate.mix(bypassed, output_ptrs, run) };
    }
    // SAFETY: the host's output channels are writable for `frames`.
    unsafe { stave_core::runtime::zero_tail(output_ptrs, run, frames) };
    drop(proc);
    // SAFETY: the host's output channels are valid for `frames`.
    let output_peaks = unsafe { stave_core::runtime::peaks(output_ptrs, frames) };
    inst.taps.publish_peaks(input_peaks, output_peaks);
    inst.taps.cpu.set(cpu);
    // Capture first, so the inspector sees what the plugin produced;
    // then a breakpoint's pause mutes what the host hears (the plugin
    // ran, so its state stays observable).
    publish_capture(&inst.telemetry, output_ptrs, frames, block_id);
    // SAFETY: the host's output channels are writable for `frames`
    // (clap.h); the plugin's borrows of them ended with `process`.
    let muted = unsafe { stave_core::runtime::mute_if_paused(&inst.telemetry, output_ptrs, frames) };
    process.mark_output_constant(muted);
    // Outbound: pending GUI gestures go to the host this block.
    emit_gestures(&inst.gestures, process.out_events());
    sys::CLAP_PROCESS_CONTINUE
}

/// The block's output as the telemetry's capture (for a face's scope),
/// unless the plugin writes its own.
fn publish_capture(telemetry: &Telemetry, output: &[*mut f32], frames: usize, block_id: u64) {
    if telemetry.plugin_owns_capture() {
        return;
    }
    let chans = output.len().min(stave_core::telemetry::CAPTURE_CHANNELS);
    let mut capture: [&[f32]; stave_core::telemetry::CAPTURE_CHANNELS] = [&[]; stave_core::telemetry::CAPTURE_CHANNELS];
    for (c, &ptr) in capture.iter_mut().zip(output).take(chans) {
        if !ptr.is_null() {
            // SAFETY: the host's output channels are valid for `frames`
            // (clap.h); the plugin's borrows of them ended with `process`.
            *c = unsafe { core::slice::from_raw_parts(ptr, frames) };
        }
    }
    if chans > 0 {
        telemetry.write_capture(&capture[..chans], block_id);
    }
}

/// Note events into `dst`; returns (written, dropped for want of room).
pub(super) fn translate_input_events(events: &InputEvents<'_>, dst: &mut [Event]) -> (usize, u32) {
    let mut written = 0;
    let mut dropped = 0u32;
    for event in events.iter() {
        let CoreEvent::Note(kind, note) = event else {
            continue;
        };
        let Some(slot) = dst.get_mut(written) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        *slot = Event {
            frame: note.header.time as usize,
            kind: EventKind::Note(NoteEvent {
                kind,
                key: note.key,
                channel: note.channel,
                velocity: note.velocity,
                note_id: note.note_id,
            }),
        };
        written += 1;
    }
    (written, dropped)
}
