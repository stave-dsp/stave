//! IAudioProcessor: setup, arrangements, latency, tail and `process`.

use super::*;

pub(super) unsafe extern "C" fn ap_can_process_sample_size(
    _this: *mut FUnknown,
    symbolic_sample_size: i32,
) -> Tresult {
    if symbolic_sample_size == SAMPLE_SIZE_32 {
        K_RESULT_OK
    } else {
        K_RESULT_FALSE
    }
}

pub(super) unsafe extern "C" fn ap_get_latency_samples<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // Read the atomic mirror, not the plugin: the audio thread may hold
    // the processor exclusively right now.
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.shared.latency()
}

pub(super) unsafe extern "C" fn ap_setup_processing<P: Vst3Export>(
    this: *mut FUnknown,
    setup: *mut ProcessSetup,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, setup) = unsafe { (parent_ref::<P>(this), setup.as_ref()) };
    inst.guarded(
        "setupProcessing",
        || K_INTERNAL_ERROR,
        || {
            inst.diagnostics.report_faults();
            let Some(s) = setup else {
                return inst.answer("setupProcessing", Err(CallError::NullArgument("setup")));
            };
            inst.shared.set_sample_rate(s.sample_rate as f32);
            inst.shared.set_max_frames(s.max_samples_per_block as u32);
            // Non-realtime context: size the in-place scratch here too, so a
            // host that skips straight to process after setup is still covered.
            if let Some(mut proc) = inst.plugin.claim() {
                proc.prepare_scratch(
                    max_in_channels::<P>(),
                    (s.max_samples_per_block as usize).max(1),
                );
            }
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn ap_set_processing(_this: *mut FUnknown, _state: TBool) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn ap_get_tail_samples<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.tail.load(Ordering::Acquire)
}

pub(super) unsafe extern "C" fn ap_process<P: Vst3Export>(
    this: *mut FUnknown,
    data: *mut ProcessData,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, process) = unsafe { (parent_ref::<P>(this), Process::new(data)) };
    let Some(process) = process else {
        inst.diagnostics
            .faults
            .record(ProcessFault::MissingProcessData, 1);
        return K_INVALID_ARGUMENT;
    };
    // The realtime guard: a panic here is recorded, and logged by the
    // next (guarded) lifecycle or state call.
    inst.guard.run_realtime(
        "process",
        || {
            process.silence_output();
            K_INTERNAL_ERROR
        },
        || process_block(inst, &process),
    )
}

fn process_block<P: Vst3Export>(inst: &Instance<P>, process: &Process<'_>) -> Tresult {
    let d = process.data();
    if d.symbolic_sample_size != SAMPLE_SIZE_32 {
        inst.diagnostics
            .faults
            .record(ProcessFault::UnsupportedSampleSize, 1);
        return K_INTERNAL_ERROR;
    }
    let frames = d.num_samples as usize;
    // Serve the host-accepted configuration.
    let cfg = inst.config.load(Ordering::Acquire);
    let ports = P::LAYOUTS[cfg.min(P::LAYOUTS.len() - 1)];

    let input_ptrs = match ports.input_channels() {
        0 => &[],
        _ => process.input(0).unwrap_or(&[]),
    };

    let mut param_buf = [stave_core::ParamChange {
        id: stave_core::ParamId(0),
        frame: 0,
        value: 0.0,
        kind: stave_core::ParamChangeKind::Value,
    }; 256];
    let mut param_count = 0;
    let mut params_dropped = 0;

    // Host automation drain happens before any buffer checks: hosts
    // legitimately call process with NO buffers purely to flush
    // parameter changes (validator: "Parameters Flush").
    if let Some(changes) = process.param_changes() {
        (param_count, params_dropped) =
            events::drain_param_changes(&changes, inst.params.as_ref(), &mut param_buf);
        // In place: the standard stable sort may allocate.
        stave_core::runtime::sort_param_changes(&mut param_buf[..param_count]);
        inst.diagnostics
            .faults
            .record(ProcessFault::ParamChangesDropped, params_dropped);
    }
    // No outputs = a flush call, not an error.
    let Some(output_ptrs) = process.output() else {
        return K_RESULT_OK;
    };
    let Some(output_ptrs) = output_ptrs else {
        inst.diagnostics
            .faults
            .record(ProcessFault::MissingOutput, 1);
        return K_INTERNAL_ERROR;
    };

    // Translate input events (note on/off only for v0).
    let mut events_buf: [StaveEvent; 256] = [StaveEvent {
        frame: 0,
        kind: EventKind::Note(stave_core::NoteEvent {
            kind: stave_core::NoteKind::Choke,
            key: -1,
            channel: -1,
            velocity: 0.0,
            note_id: -1,
        }),
    }; 256];
    let event_count = if let Some(events) = process.events() {
        let (written, dropped) = events::translate_input_events(&events, &mut events_buf);
        inst.diagnostics
            .faults
            .record(ProcessFault::EventsDropped, dropped);
        written
    } else {
        0
    };

    // Robustness guards: zero-length blocks are a silent
    // success; blocks beyond the declared max_frames are refused —
    // scratch and the bypass ring are sized to the host's own
    // declaration, and a host that violates it gets an error, not UB.
    if frames == 0 {
        return K_RESULT_OK;
    }
    let max_frames = inst.shared.max_frames() as usize;
    if max_frames > 0 && frames > max_frames {
        inst.diagnostics
            .faults
            .record(ProcessFault::BlockTooLong, 1);
        return K_INVALID_ARGUMENT;
    }

    // FTZ/DAZ backstop for the whole block (manual flushes in
    // stave-dsp remain primary; see stave_core::runtime::ScopedFtz).
    let _ftz = stave_core::runtime::ScopedFtz::engage();

    // Exclusive processor access for the duration of the block.
    let Some(mut proc) = inst.plugin.claim() else {
        inst.diagnostics
            .faults
            .record(ProcessFault::ConcurrentProcess, 1);
        return K_INTERNAL_ERROR;
    };

    // Aux inputs: aux buses follow the main input, declaration
    // order. An inactive/absent bus (hosts routinely leave sidechains
    // unconnected) becomes a zero-channel buffer — the `Aux` contract.
    const MAX_AUX: usize = stave_core::runtime::MAX_AUX_PORTS;
    let n_aux = ports.aux.len().min(MAX_AUX);
    let main_ins = usize::from(ports.input_channels() > 0);
    let mut aux_ptrs: [&[*mut f32]; MAX_AUX] = [&[]; MAX_AUX];
    for (a, ptrs) in aux_ptrs.iter_mut().enumerate().take(n_aux) {
        *ptrs = process.input(main_ins + a).unwrap_or(&[]);
    }

    // Channel lists the buffers can take. In-place hosts (Live, notably)
    // pass input and output pointing at the same memory — which would
    // violate the buffers' no-alias contract — so overlapping inputs
    // are staged through preallocated scratch; null channels read zeroed
    // scratch. Zero-copy otherwise.
    // SAFETY: the host's channel pointers are valid for `frames` (SDK).
    let borrows =
        unsafe { proc.split_for_block(input_ptrs, output_ptrs, &aux_ptrs[..n_aux], frames) };
    let (plugin, gate, io) = (borrows.plugin, borrows.bypass, borrows.io);
    if io.truncated {
        inst.diagnostics
            .faults
            .record(ProcessFault::BlockTruncated, 1);
    }
    let run = io.frames;

    // Bypass dry capture BEFORE the plugin runs (post-staging, so the
    // dry path survives in-place processing). The bypass param comes
    // from the table's cached index (no scan).
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
    let input_ptrs = io.input;

    let mut block = Block::new(inst.shared.sample_rate(), run, input, output)
        .with_aux(&aux_bufs[..n_aux])
        .with_transport(process.context().map(translate_transport))
        .with_params(stave_core::ParamBlock::with_dropped(&param_buf[..param_count], params_dropped))
        .with_events(&events_buf[..event_count]);

    // Generic I/O level metering (read-only) for the editor's frame
    // rails, published as taps below: input pre-process, output
    // post-process.
    // SAFETY: `split_for_block` lists are valid for `run` frames.
    let input_peaks = unsafe { stave_core::runtime::peaks(input_ptrs, run) };

    let block_id = inst.telemetry.tick_block();
    // Time strictly the plugin's own work (Instant is a wait-free
    // syscall pair — no alloc, no lock; audio-thread safe).
    let t_process = std::time::Instant::now();
    plugin.process(&mut block);
    let cpu_frac =
        t_process.elapsed().as_secs_f64() / (frames as f64 / f64::from(inst.shared.sample_rate()));
    // Framework bypass: crossfade plugin output with latency-matched
    // dry. One branch when fully active.
    if bypass.is_some() {
        // SAFETY: the output channels are writable for `run` (SDK).
        unsafe { gate.mix(bypassed, output_ptrs, run) };
    }
    // SAFETY: the host's output channels are writable for `frames` (SDK).
    unsafe { stave_core::runtime::zero_tail(output_ptrs, run, frames) };

    // SAFETY: valid for `frames` (SDK).
    let output_peaks = unsafe { stave_core::runtime::peaks(output_ptrs, frames) };
    inst.taps.publish_peaks(input_peaks, output_peaks);
    inst.taps.cpu.set(cpu_frac);

    // If a breakpoint or the host has paused telemetry,
    // zero out the audio buffers so the next stage hears silence. The plugin
    // already ran, so its internal state advanced — that's what the inspector
    // wants. Mute is applied *after* the capture write below so the user sees
    // what the plugin actually produced this block.
    {
        // Capture-frame write (before mute, so the inspector sees the
        // plugin's true output even while audio is muted). Skipped when
        // the plugin owns the capture and writes its own pairs (a
        // shaper's input against its output, for a transfer scope).
        let plugin_owns_capture = inst.telemetry.plugin_owns_capture();
        let chans = output_ptrs.len().min(stave_core::telemetry::CAPTURE_CHANNELS);
        if chans > 0 && !plugin_owns_capture {
            let mut tap_buf: [&[f32]; stave_core::telemetry::CAPTURE_CHANNELS] = [&[], &[]];
            for (tap, &ptr) in tap_buf.iter_mut().zip(output_ptrs).take(chans) {
                if !ptr.is_null() {
                    // SAFETY: valid for `frames` (SDK).
                    *tap = unsafe { core::slice::from_raw_parts(ptr, frames) };
                }
            }
            inst.telemetry.write_capture(&tap_buf[..chans], block_id);
        }
        if inst.telemetry.is_paused() {
            for &ptr in output_ptrs.iter().filter(|p| !p.is_null()) {
                // SAFETY: writable for `frames` (SDK); the plugin's
                // borrows of it ended with `process`.
                unsafe { core::slice::from_raw_parts_mut(ptr, frames) }.fill(0.0);
            }
            process.mark_output_silent();
        }
    }
    K_RESULT_OK
}
