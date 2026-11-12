//! IComponent: activation, buses, IO mode, state save/load.

use super::*;

pub(super) unsafe extern "C" fn comp_initialize(
    _this: *mut FUnknown,
    _context: *mut FUnknown,
) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn comp_terminate(_this: *mut FUnknown) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn comp_get_controller_class_id<P: Vst3Export>(
    this: *mut FUnknown,
    class_id: *mut Tuid,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, class_id) = unsafe { (parent_ref::<P>(this), out_param(class_id, "classId")) };
    inst.answer(
        "getControllerClassId",
        class_id.map(|out| *out = P::CONTROLLER_CID),
    )
}

pub(super) unsafe extern "C" fn comp_set_io_mode(_this: *mut FUnknown, _mode: i32) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn comp_set_active<P: Vst3Export>(
    this: *mut FUnknown,
    state: TBool,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "setActive",
        || K_INTERNAL_ERROR,
        || {
            inst.diagnostics.report_faults();
            // A compliant host never overlaps setActive with process; if one
            // does, refuse rather than race.
            let Some(mut proc) = inst.plugin.claim() else {
                return inst.answer("setActive", Err(CallError::ProcessorBusy));
            };
            if state != 0 {
                let max_frames = inst.shared.max_frames() as usize;
                proc.plugin.activate(inst.shared.sample_rate(), max_frames);
                // Fresh transport start: stale DSP state must not ring in.
                proc.plugin.reset();
                proc.prepare_scratch(max_in_channels::<P>(), max_frames.max(1));
                let new_latency = proc.plugin.latency_samples();
                proc.bypass.prepare(
                    max_in_channels::<P>(),
                    new_latency as usize,
                    max_frames.max(1),
                    f64::from(inst.shared.sample_rate()),
                );
                // Latency changed across activations: tell the host to re-query.
                if new_latency != inst.shared.latency() {
                    inst.handler.restart_component(RESTART_LATENCY_CHANGED);
                }
                inst.shared.set_latency(new_latency);
                inst.tail
                    .store(proc.plugin.tail().to_samples(), Ordering::Release);
                inst.shared.set_activated(true);
            } else if inst.shared.activated() {
                proc.plugin.deactivate();
                inst.shared.set_activated(false);
            }
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn comp_set_state<P: Vst3Export>(
    this: *mut FUnknown,
    state: *mut FUnknown,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, stream) = unsafe { (parent_ref::<P>(this), Stream::new(state)) };
    inst.guarded(
        "setState",
        || K_INTERNAL_ERROR,
        || {
            inst.diagnostics.report_faults();
            let result = stream.and_then(|stream| {
                wrapper_state::load(
                    &stream.read_to_end(MAX_STATE_BYTES)?,
                    &inst.plugin,
                    inst.param_table.as_ref(),
                    &inst.state_store,
                )
            });
            inst.answer("setState", result)
        },
    )
}

pub(super) unsafe extern "C" fn comp_get_state<P: Vst3Export>(
    this: *mut FUnknown,
    state: *mut FUnknown,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, stream) = unsafe { (parent_ref::<P>(this), Stream::new(state)) };
    inst.guarded(
        "getState",
        || K_INTERNAL_ERROR,
        || {
            inst.diagnostics.report_faults();
            let result = stream.and_then(|stream| {
                stream.write_all(&wrapper_state::save(
                    &inst.plugin,
                    inst.param_table.as_ref(),
                    &inst.state_store,
                    &inst.diagnostics,
                ))
            });
            inst.answer("getState", result)
        },
    )
}
