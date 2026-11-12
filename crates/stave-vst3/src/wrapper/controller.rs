//! IEditController: the parameter bridge, state on the controller side, and
//! the gesture API the editor writes through.

use super::*;

pub(super) unsafe extern "C" fn ec_initialize(
    _this: *mut FUnknown,
    _context: *mut FUnknown,
) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn ec_terminate(_this: *mut FUnknown) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn ec_set_component_state<P: Vst3Export>(
    this: *mut FUnknown,
    state: *mut FUnknown,
) -> Tresult {
    // Fused component+controller: the same chunk drives both sides;
    // re-applying is idempotent (atomics + snap).
    // SAFETY: the same host contract `setState` has.
    unsafe { comp_set_state::<P>(this, state) }
}

pub(super) unsafe extern "C" fn ec_set_state(
    _this: *mut FUnknown,
    _state: *mut FUnknown,
) -> Tresult {
    // Controller-SPECIFIC state (view prefs etc.) is empty by design;
    // succeed with nothing so preset containers don't abort
    // (K_NOT_IMPLEMENTED made PresetFile::savePreset fail wholesale).
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn ec_get_state(
    _this: *mut FUnknown,
    _state: *mut FUnknown,
) -> Tresult {
    K_RESULT_OK
}

// VST3 ParameterInfo flag bits.
pub(super) const PARAM_CAN_AUTOMATE: i32 = 1 << 0;
pub(super) const PARAM_IS_BYPASS: i32 = 1 << 16;

pub(super) unsafe extern "C" fn ec_get_parameter_count<P: Vst3Export>(this: *mut FUnknown) -> i32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "getParameterCount",
        || 0,
        || (inst.param_table.count() + usize::from(inst.programs.exposed())) as i32,
    )
}

pub(super) unsafe extern "C" fn ec_get_parameter_info<P: Vst3Export>(
    this: *mut FUnknown,
    index: i32,
    info: *mut ParameterInfo,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, dest) = unsafe { (parent_ref::<P>(this), out_param(info, "info")) };
    inst.guarded(
        "getParameterInfo",
        || K_INTERNAL_ERROR,
        || {
            let dest = match dest {
                Ok(dest) => dest,
                Err(error) => return inst.answer("getParameterInfo", Err(error)),
            };
            // SAFETY: all-zero is a valid `ParameterInfo` (integers, a float,
            // UTF-16 arrays).
            *dest = unsafe { mem::zeroed() };
            let table = &inst.param_table;
            if index as usize == table.count() && inst.programs.exposed() {
                inst.programs.info(dest);
                return K_RESULT_OK;
            }
            let Some(p) = usize::try_from(index).ok().and_then(|i| table.by_index(i)) else {
                return inst.answer(
                    "getParameterInfo",
                    Err(CallError::IndexOutOfRange(index.into())),
                );
            };
            let meta = p.meta();
            // Wire id = the author's STABLE id — never the index. Saved
            // automation lanes survive param reordering across versions.
            dest.id = meta.id.0;
            let combined = if meta.group.is_empty() {
                meta.name.to_string()
            } else {
                alloc::format!("{} · {}", meta.group, meta.name)
            };
            write_utf16_name(&mut dest.title, &combined);
            write_utf16_name(&mut dest.short_title, meta.name);
            write_utf16_name(&mut dest.units, meta.unit.label());
            dest.default_normalized_value = p.default_normalized();
            dest.unit_id = inst.units.unit_of(meta.group);
            // VST3 step_count = positions − 1; 0 = continuous.
            dest.step_count = p.step_count().saturating_sub(1) as i32;
            dest.flags = if meta.flags.bypass {
                PARAM_CAN_AUTOMATE | PARAM_IS_BYPASS
            } else if meta.flags.non_automatable {
                0
            } else {
                PARAM_CAN_AUTOMATE
            };
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn ec_get_param_string_by_value<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
    value_normalized: ParamValue,
    string: *mut TChar,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, dst) = unsafe { (parent_ref::<P>(this), string128(string, "string")) };
    inst.guarded(
        "getParamStringByValue",
        || K_INTERNAL_ERROR,
        || {
            let dst = match dst {
                Ok(dst) => dst,
                Err(error) => return inst.answer("getParamStringByValue", Err(error)),
            };
            let table = &inst.param_table;
            if id == stave_core::ParamId::PROGRAM.0 && inst.programs.exposed() {
                write_utf16_name(dst, inst.programs.name(value_normalized));
                return K_RESULT_OK;
            }
            let Some(p) = table.by_id(stave_core::ParamId(id)) else {
                return inst.answer("getParamStringByValue", Err(CallError::UnknownParamId(id)));
            };
            write_utf16_name(
                dst,
                &p.format_plain(p.normalized_to_plain(value_normalized)),
            );
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn ec_get_param_value_by_string<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
    string: *mut TChar,
    value_normalized: *mut ParamValue,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, string, out) = unsafe {
        (
            parent_ref::<P>(this),
            tchar_str(string, 127, "string"),
            out_param(value_normalized, "valueNormalized"),
        )
    };
    inst.guarded(
        "getParamValueByString",
        || K_INTERNAL_ERROR,
        || {
            let call = "getParamValueByString";
            let (string, out) = match (string, out) {
                (Ok(string), Ok(out)) => (string, out),
                (Err(error), _) | (_, Err(error)) => return inst.answer(call, Err(error)),
            };
            let table = &inst.param_table;
            let program = id == stave_core::ParamId::PROGRAM.0 && inst.programs.exposed();
            let p = table.by_id(stave_core::ParamId(id));
            if p.is_none() && !program {
                return inst.answer(call, Err(CallError::UnknownParamId(id)));
            }
            // Parse targets are ASCII ("2.4k", "-6 dB"); non-ASCII can't match
            // any Unit grammar, so reject the whole string.
            let mut buf = [0u8; 127];
            for (dst, &unit) in buf.iter_mut().zip(string) {
                if unit > 0x7f {
                    return K_RESULT_FALSE;
                }
                *dst = unit as u8;
            }
            let Ok(text) = core::str::from_utf8(&buf[..string.len()]) else {
                return K_RESULT_FALSE;
            };
            let parsed = match p {
                Some(p) => p.parse(text).map(|plain| p.plain_to_normalized(plain)),
                None => inst.programs.find(text),
            };
            match parsed {
                Some(normalized) => {
                    *out = normalized;
                    K_RESULT_OK
                }
                None => K_RESULT_FALSE,
            }
        },
    )
}

pub(super) unsafe extern "C" fn ec_normalized_param_to_plain<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
    value_normalized: ParamValue,
) -> ParamValue {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "normalizedParamToPlain",
        || value_normalized,
        || {
            let table = &inst.param_table;
            match table.by_id(stave_core::ParamId(id)) {
                Some(p) => p.normalized_to_plain(value_normalized),
                None if id == stave_core::ParamId::PROGRAM.0 && inst.programs.exposed() => {
                    inst.programs.index_of(value_normalized) as f64
                }
                None => value_normalized,
            }
        },
    )
}

pub(super) unsafe extern "C" fn ec_plain_param_to_normalized<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
    plain_value: ParamValue,
) -> ParamValue {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "plainParamToNormalized",
        || plain_value,
        || {
            let table = &inst.param_table;
            match table.by_id(stave_core::ParamId(id)) {
                Some(p) => p.plain_to_normalized(plain_value),
                None if id == stave_core::ParamId::PROGRAM.0 && inst.programs.exposed() => {
                    inst.programs.normalized_of(plain_value)
                }
                None => plain_value,
            }
        },
    )
}

pub(super) unsafe extern "C" fn ec_get_param_normalized<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
) -> ParamValue {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "getParamNormalized",
        || 0.0,
        || {
            let table = &inst.param_table;
            match table.by_id(stave_core::ParamId(id)) {
                Some(p) => p.normalized(),
                None if id == stave_core::ParamId::PROGRAM.0 => inst.programs.normalized(),
                None => 0.0,
            }
        },
    )
}

pub(super) unsafe extern "C" fn ec_set_param_normalized<P: Vst3Export>(
    this: *mut FUnknown,
    id: ParamId,
    value: ParamValue,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.guarded(
        "setParamNormalized",
        || K_INTERNAL_ERROR,
        || {
            let table = &inst.param_table;
            match table.by_id(stave_core::ParamId(id)) {
                Some(p) => {
                    p.set_normalized(value);
                    K_RESULT_OK
                }
                None if id == stave_core::ParamId::PROGRAM.0 && inst.programs.exposed() => {
                    inst.programs.select(value, table.as_ref(), &inst.handler);
                    K_RESULT_OK
                }
                None => inst.answer("setParamNormalized", Err(CallError::UnknownParamId(id))),
            }
        },
    )
}

pub(super) unsafe extern "C" fn ec_set_component_handler<P: Vst3Export>(
    this: *mut FUnknown,
    handler: *mut FUnknown,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    // Not the instance guard: host-object bookkeeping touches no plugin
    // state, and a disabled instance must still let the host swap or
    // clear its handler.
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            // SAFETY: host contract: `handler` is null or the host's
            // IComponentHandler.
            unsafe { inst.handler.replace(handler.cast()) };
            K_RESULT_OK
        },
    )
}

// ===========================================================================
// Gesture API — the editor's ONLY sanctioned write path. The
// host round-trips performEdit back through setParamNormalized, which
// updates the table; writing the table here as well keeps headless /
// handler-less operation (bench, tests) live without double-apply
// concerns (set_normalized is idempotent for equal values).
// ===========================================================================

pub(super) unsafe extern "C" fn ec_create_view<P: Vst3Export>(
    this: *mut FUnknown,
    name: FidString,
) -> *mut FUnknown {
    if name.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: a non-null `FIDString` is nul-terminated (SDK).
    if unsafe { core::ffi::CStr::from_ptr(name) } != c"editor" {
        return ptr::null_mut();
    }
    // SAFETY: host contract (see `crate::host`).
    let (inst, inst_ptr) = unsafe { (parent_ref::<P>(this), parent_of::<P>(this)) };
    // A fresh view per call, returned with one reference the host owns
    // (SDK `createView` contract); it holds its own instance reference.
    #[cfg(target_os = "macos")]
    return inst.guarded("createView", ptr::null_mut, || new_view(inst, inst_ptr));
    // Faces show in an NSView; elsewhere the host shows its own editor.
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (inst, inst_ptr);
        ptr::null_mut()
    }
}
