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
    inst.guarded(
        "process",
        || {
            process.silence_output();
            sys::CLAP_PROCESS_ERROR
        },
        || process_block(inst, &process),
    )
}

fn process_block<P: ClapExport>(
    inst: &Instance<P>,
    process: &Process<'_>,
) -> sys::ClapProcessStatus {
    let frames = process.frames();

    // Serve the host-selected configuration.
    let cfg = inst.config.load(core::sync::atomic::Ordering::Acquire);
    let want = <P as Plugin>::CONFIGS[cfg.min(<P as Plugin>::CONFIGS.len() - 1)];

    // A plugin without inputs still gets a (zero-channel) input buffer.
    let input_ptrs = match want.in_channels() {
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
        sample_offset: 0,
        plain_value: 0.0,
        kind: stave_core::ParamChangeKind::Value,
    }; 256];
    let (mut param_count, mut params_dropped, mut event_count) = (0, 0, 0);
    if let Some(events) = process.in_events() {
        (param_count, params_dropped) =
            apply_param_events(&events, inst.param_table.as_ref(), &mut param_buf);
        param_buf[..param_count].sort_by_key(|change| change.sample_offset);
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
    // stave-dsp remain primary; see stave_core::wrapper::ScopedFtz).
    let _ftz = stave_core::ScopedFtz::engage();

    // Exclusive processor access for the block; refuse (error status)
    // on a host protocol violation instead of racing.
    let Some(mut proc) = inst.plugin.claim() else {
        inst.fault(ProcessFault::ConcurrentProcess, 1);
        return sys::CLAP_PROCESS_ERROR;
    };

    // In-place hosts alias input and output; stage input through
    // preallocated scratch so the AudioBuffer no-alias contract holds.
    let stage = ranges_alias(input_ptrs, output_ptrs, frames).then_some((input_ptrs, frames));
    // SAFETY: the host's channel pointers are valid for `frames` (clap.h).
    let (plugin, gate, staged) = unsafe { proc.split_for_block(stage) };
    let input_ptrs = staged.unwrap_or(input_ptrs);

    let bypassed = inst.bypass_id.is_some_and(|id| {
        inst.param_table
            .by_id(ParamId(id))
            .is_some_and(|p| p.plain() >= 0.5)
    });
    if inst.bypass_id.is_some() {
        // SAFETY: as above.
        unsafe { gate.feed(input_ptrs, frames) };
    }

    // SAFETY: valid for `frames` (clap.h); staging removed any aliasing.
    let (input, output) = unsafe {
        (
            AudioBuffer::from_raw(input_ptrs, frames),
            AudioBuffer::from_raw(output_ptrs, frames),
        )
    };

    // Aux inputs: declared aux ports follow the main input in
    // declaration order. A missing/null port becomes a zero-channel
    // buffer — the AuxIn contract (plugin fully functional without).
    const MAX_AUX: usize = 4;
    let mut aux_bufs = [const { AudioBuffer::empty() }; MAX_AUX];
    let n_aux = want.aux_in.len().min(MAX_AUX);
    let main_ports = usize::from(want.in_channels() > 0);
    for (a, aux) in aux_bufs.iter_mut().enumerate().take(n_aux) {
        if let Some(ptrs) = process.input(main_ports + a) {
            // SAFETY: valid for `frames` (clap.h).
            *aux = unsafe { AudioBuffer::from_raw(ptrs, frames) };
        }
    }

    let transport = process.transport().map(|t| Transport {
        playing: t.flags & sys::CLAP_TRANSPORT_IS_PLAYING != 0,
        tempo_bpm: (t.flags & sys::CLAP_TRANSPORT_HAS_TEMPO != 0).then_some(t.tempo),
        ppq_position: (t.flags & sys::CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0)
            .then(|| t.song_pos_beats as f64 / sys::CLAP_SECTIME_UNIT),
        song_pos_seconds: (t.flags & sys::CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0)
            .then(|| t.song_pos_seconds as f64 / sys::CLAP_SECTIME_UNIT),
    });

    let mut ctx = ProcessCtx {
        sample_rate: inst.shared.sample_rate(),
        frames,
        transport,
        input,
        output,
        aux: &aux_bufs[..n_aux],
        params: stave_core::ParamBlock::with_dropped(&param_buf[..param_count], params_dropped),
        events: &events_buf[..event_count],
    };

    plugin.process(&mut ctx);
    if inst.bypass_id.is_some() {
        // SAFETY: the output channels are writable for `frames` (clap.h).
        unsafe { gate.mix(bypassed, output_ptrs, frames) };
    }
    drop(proc);
    // Outbound: pending GUI gestures go to the host this block.
    emit_gestures(&inst.gestures, process.out_events());
    sys::CLAP_PROCESS_CONTINUE
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
            frame: note.header.time,
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
