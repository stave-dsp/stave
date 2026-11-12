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
    inst.guarded(
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
    let ports = <P as Plugin>::CONFIGS[cfg.min(<P as Plugin>::CONFIGS.len() - 1)];

    let input_ptrs = match ports.in_channels() {
        0 => &[],
        _ => process.input(0).unwrap_or(&[]),
    };

    let mut param_buf = [stave_core::ParamChange {
        id: stave_core::ParamId(0),
        sample_offset: 0,
        plain_value: 0.0,
        kind: stave_core::ParamChangeKind::Value,
    }; 256];
    let mut param_count = 0;
    let mut params_dropped = 0;

    // Host automation drain happens before any buffer checks: hosts
    // legitimately call process with NO buffers purely to flush
    // parameter changes (validator: "Parameters Flush").
    if let Some(changes) = process.param_changes() {
        (param_count, params_dropped) =
            events::drain_param_changes(&changes, inst.param_table.as_ref(), &mut param_buf);
        param_buf[..param_count].sort_by_key(|change| change.sample_offset);
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
    // stave-dsp remain primary; see stave_core::wrapper::ScopedFtz).
    let _ftz = stave_core::ScopedFtz::engage();

    // Exclusive processor access for the duration of the block.
    let Some(mut proc) = inst.plugin.claim() else {
        inst.diagnostics
            .faults
            .record(ProcessFault::ConcurrentProcess, 1);
        return K_INTERNAL_ERROR;
    };

    // In-place hosts (Live, notably) pass input and output pointing at
    // the same memory — which would violate the AudioBuffer no-alias
    // contract. Stage the input through preallocated scratch when the
    // ranges overlap; zero-copy otherwise.
    let stage = ranges_alias(input_ptrs, output_ptrs, frames).then_some((input_ptrs, frames));
    // SAFETY: the host's channel pointers are valid for `frames` (SDK).
    let (plugin, gate, staged) = unsafe { proc.split_for_block(stage) };
    let input_ptrs = staged.unwrap_or(input_ptrs);

    // Bypass dry capture BEFORE the plugin runs (post-staging, so the
    // dry path survives in-place processing).
    let bypassed = inst.bypass_id.is_some_and(|id| {
        inst.param_table
            .by_id(stave_core::ParamId(id))
            .is_some_and(|p| p.plain() >= 0.5)
    });
    if inst.bypass_id.is_some() {
        // SAFETY: as above.
        unsafe { gate.feed(input_ptrs, frames) };
    }

    // SAFETY: valid for `frames` (SDK); staging removed any aliasing.
    let (input, output) = unsafe {
        (
            AudioBuffer::from_raw(input_ptrs, frames),
            AudioBuffer::from_raw(output_ptrs, frames),
        )
    };

    // Aux inputs: aux buses follow the main input, declaration
    // order. An inactive/absent bus (hosts routinely leave sidechains
    // unconnected) becomes a zero-channel buffer — the AuxIn contract.
    const MAX_AUX: usize = 4;
    let mut aux_bufs = [const { AudioBuffer::empty() }; MAX_AUX];
    let n_aux = ports.aux_in.len().min(MAX_AUX);
    let main_ins = usize::from(ports.in_channels() > 0);
    for (a, aux) in aux_bufs.iter_mut().enumerate().take(n_aux) {
        if let Some(ptrs) = process.input(main_ins + a).filter(|p| !p.is_empty()) {
            // SAFETY: valid for `frames` (SDK).
            *aux = unsafe { AudioBuffer::from_raw(ptrs, frames) };
        }
    }

    let mut ctx = ProcessCtx {
        sample_rate: inst.shared.sample_rate(),
        frames,
        transport: process.context().map(translate_transport),
        input,
        output,
        aux: &aux_bufs[..n_aux],
        params: stave_core::ParamBlock::with_dropped(&param_buf[..param_count], params_dropped),
        events: &events_buf[..event_count],
    };

    // Generic I/O level metering (read-only) for the editor's frame
    // rails, published as probes below: input pre-process, output
    // post-process.
    // SAFETY: valid for `frames` (SDK).
    let input_peaks = unsafe { crate::metrics::peaks(input_ptrs, frames) };

    let block_id = inst.debug_bus.tick_block();
    // Time strictly the plugin's own work (Instant is a wait-free
    // syscall pair — no alloc, no lock; audio-thread safe).
    let t_process = std::time::Instant::now();
    plugin.process(&mut ctx);
    let cpu_frac =
        t_process.elapsed().as_secs_f64() / (frames as f64 / f64::from(inst.shared.sample_rate()));
    // Framework bypass: crossfade plugin output with latency-matched
    // dry. One branch when fully active.
    if inst.bypass_id.is_some() {
        // SAFETY: the output channels are writable for `frames` (SDK).
        unsafe { gate.mix(bypassed, output_ptrs, frames) };
    }

    // SAFETY: valid for `frames` (SDK).
    let output_peaks = unsafe { crate::metrics::peaks(output_ptrs, frames) };
    let bus = &inst.debug_bus;
    bus.probe_f64(&inst.io_in_probe, "io_in", input_peaks.all);
    bus.probe_f64(&inst.io_out_probe, "io_out", output_peaks.all);
    bus.probe_f64(&inst.io_in_left_probe, "io_in_l", input_peaks.left);
    bus.probe_f64(&inst.io_in_right_probe, "io_in_r", input_peaks.right);
    bus.probe_f64(&inst.io_out_left_probe, "io_out_l", output_peaks.left);
    bus.probe_f64(&inst.io_out_right_probe, "io_out_r", output_peaks.right);
    inst.debug_bus.probe_f64(&inst.cpu_probe, "cpu", cpu_frac);

    // If a breakpoint or the host has flipped the bus into paused mode,
    // zero out the audio buffers so the next stage hears silence. The plugin
    // already ran, so its internal state advanced — that's what the inspector
    // wants. Mute is applied *after* the capture write below so the user sees
    // what the plugin actually produced this block.
    {
        // Capture-frame write (before mute, so the inspector sees the
        // plugin's true output even while audio is muted). Skipped when
        // the plugin owns the capture and writes its own pairs (e.g. Forge captures shaper input vs chain output for the scope).
        let plugin_owns_capture = inst.debug_bus.plugin_owns_capture();
        let chans = output_ptrs.len().min(stave_core::debug::CAPTURE_CHANNELS);
        if chans > 0 && !plugin_owns_capture {
            let mut tap_buf: [&[f32]; stave_core::debug::CAPTURE_CHANNELS] = [&[], &[]];
            for (tap, &ptr) in tap_buf.iter_mut().zip(output_ptrs).take(chans) {
                if !ptr.is_null() {
                    // SAFETY: valid for `frames` (SDK).
                    *tap = unsafe { core::slice::from_raw_parts(ptr, frames) };
                }
            }
            inst.debug_bus.write_capture(&tap_buf[..chans], block_id);
        }
        if inst.debug_bus.is_paused() {
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
