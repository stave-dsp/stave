//! The `clap.params` extension.

use super::*;

pub(super) unsafe extern "C" fn params_count<P: ClapExport>(plugin: *const sys::ClapPlugin) -> u32 {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guarded("params.count", || 0, || inst.param_table.count() as u32)
}

pub(super) unsafe extern "C" fn params_get_info<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    param_index: u32,
    info: *mut sys::ClapParamInfo,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, info) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(info, "param_info"),
        )
    };
    inst.call("params.get_info", || {
        info.and_then(|info| get_info(inst, param_index, info))
    })
}

fn get_info<P: ClapExport>(
    inst: &Instance<P>,
    param_index: u32,
    out: &mut sys::ClapParamInfo,
) -> Result<(), CallError> {
    let p = inst
        .param_table
        .by_index(param_index as usize)
        .ok_or(CallError::IndexOutOfRange(param_index.into()))?;
    let meta = p.meta();
    let mut flags = 0u32;
    if !meta.flags.non_automatable {
        flags |= sys::CLAP_PARAM_IS_AUTOMATABLE;
    }
    if meta.flags.bypass {
        flags |= sys::CLAP_PARAM_IS_BYPASS;
    }
    if meta.flags.hidden {
        flags |= sys::CLAP_PARAM_IS_HIDDEN;
    }
    if p.step_count() > 0 {
        flags |= sys::CLAP_PARAM_IS_STEPPED;
    }
    *out = sys::ClapParamInfo {
        id: meta.id.0,
        flags,
        cookie: ptr::null_mut(),
        name: [0; sys::CLAP_NAME_SIZE],
        module: [0; sys::CLAP_PATH_SIZE],
        // CLAP is plain-valued on the wire: min/max/default in plain units.
        min_value: p.normalized_to_plain(0.0),
        max_value: p.normalized_to_plain(1.0),
        default_value: p.normalized_to_plain(p.default_normalized()),
    };
    write_c_str(&mut out.name, meta.name);
    write_c_str(&mut out.module, meta.group);
    Ok(())
}

pub(super) unsafe extern "C" fn params_get_value<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    param_id: sys::ClapId,
    out: *mut f64,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, out) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(out, "out_value"),
        )
    };
    inst.call("params.get_value", || {
        let result = out.and_then(|out| {
            *out = param(inst, param_id)?.plain();
            Ok(())
        });
        result
    })
}

pub(super) unsafe extern "C" fn params_value_to_text<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    param_id: sys::ClapId,
    value: f64,
    out: *mut c_char,
    size: u32,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, out) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_text(out, size, "out_buffer"),
        )
    };
    inst.call("params.value_to_text", || {
        let result = out.and_then(|out| {
            write_c_str(out, &param(inst, param_id)?.format_plain(value));
            Ok(())
        });
        result
    })
}

/// Text that does not parse is an answer (`false`), not an error.
pub(super) unsafe extern "C" fn params_text_to_value<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    param_id: sys::ClapId,
    text: *const c_char,
    out: *mut f64,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, text, out) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            in_text(text, "param_value_text"),
            out_param(out, "out_value"),
        )
    };
    let name = "params.text_to_value";
    inst.guarded(
        name,
        || false,
        || {
            let result = text.and_then(|text| {
                let out = out?;
                let p = param(inst, param_id)?;
                Ok(text
                    .to_str()
                    .ok()
                    .and_then(|s| p.parse(s))
                    .map(|plain| *out = plain)
                    .is_some())
            });
            match result {
                Ok(parsed) => parsed,
                Err(error) => inst.answer(name, Err(error)),
            }
        },
    )
}

fn param<P: ClapExport>(
    inst: &Instance<P>,
    id: sys::ClapId,
) -> Result<&dyn stave_core::params::AnyParam, CallError> {
    inst.param_table
        .by_id(ParamId(id))
        .ok_or(CallError::UnknownParamId(id))
}

/// Host applies param changes outside processing (transport stopped).
pub(super) unsafe extern "C" fn params_flush<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    in_events: *const sys::ClapInputEvents,
    out_events: *const sys::ClapOutputEvents,
) {
    // SAFETY: host contract (see `host`).
    let (inst, in_events, out_events) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            InputEvents::new(in_events),
            OutputEvents::new(out_events),
        )
    };
    inst.guarded(
        "params.flush",
        || (),
        || {
            if let Some(events) = in_events {
                let _ = apply_param_events(&events, inst.param_table.as_ref(), &mut []);
            }
            emit_gestures(&inst.gestures, out_events);
        },
    );
}

/// CLAP param events (plain values) into `dst`, the block's timeline
/// that processor-owned controls render sample-accurately; the last
/// value per param becomes the table's target. Returns (written into
/// `dst`, dropped for want of room).
pub(super) fn apply_param_events(
    events: &InputEvents<'_>,
    table: &dyn Params,
    dst: &mut [stave_core::ParamChange],
) -> (usize, u32) {
    let mut written = 0;
    let mut dropped = 0u32;
    for event in events.iter() {
        let CoreEvent::Param(kind, ev) = event else {
            continue;
        };
        let id = ParamId(ev.param_id);
        let Some(p) = table.by_id(id) else {
            continue;
        };
        if let Some(slot) = dst.get_mut(written) {
            *slot = stave_core::ParamChange {
                id,
                sample_offset: ev.header.time,
                plain_value: ev.value,
                kind,
            };
            written += 1;
        } else {
            dropped = dropped.saturating_add(1);
        }
        if kind == stave_core::ParamChangeKind::Value {
            p.set_plain(ev.value);
        }
    }
    (written, dropped)
}
