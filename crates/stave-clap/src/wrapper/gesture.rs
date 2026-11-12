//! The gesture API: the UI thread pushes, `process`/`flush` drain to the host.

use super::*;

/// UI-thread entry points the editor's knobs write through.
///
/// # Safety
/// `plugin` must be a live pointer previously returned by this
/// wrapper's `create_plugin` for the same `P` (i.e. the start of an
/// `Instance<P>`), not yet destroyed.
pub unsafe fn gesture_begin<P: ClapExport>(plugin: *const sys::ClapPlugin, id: u32) {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.gestures.push(GestureEvent::Begin { id });
}

/// # Safety
/// As [`gesture_begin`].
pub unsafe fn gesture_value<P: ClapExport>(plugin: *const sys::ClapPlugin, id: u32, plain: f64) {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    // Table updated immediately (host-visible truth); the out_event
    // tells the DAW to record it.
    if let Some(p) = inst.param_table.by_id(ParamId(id)) {
        p.set_plain(plain);
    }
    inst.gestures.push(GestureEvent::Value { id, plain });
}

/// # Safety
/// As [`gesture_begin`].
pub unsafe fn gesture_end<P: ClapExport>(plugin: *const sys::ClapPlugin, id: u32) {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.gestures.push(GestureEvent::End { id });
}

/// Drain pending gestures into the host's out_events list. Without a
/// list this call, they stay queued for the next.
pub(super) fn emit_gestures(gestures: &GestureQueue, out: Option<OutputEvents<'_>>) {
    let Some(out) = out else {
        return;
    };
    let header = |size: usize, r#type: u16| sys::ClapEventHeader {
        size: size as u32,
        time: 0,
        space_id: sys::CLAP_CORE_EVENT_SPACE_ID,
        r#type,
        flags: 0,
    };
    while let Some(ev) = gestures.pop() {
        match ev {
            GestureEvent::Begin { id } | GestureEvent::End { id } => {
                let kind = if matches!(ev, GestureEvent::Begin { .. }) {
                    sys::CLAP_EVENT_PARAM_GESTURE_BEGIN
                } else {
                    sys::CLAP_EVENT_PARAM_GESTURE_END
                };
                out.try_push(&sys::ClapEventParamGesture {
                    header: header(core::mem::size_of::<sys::ClapEventParamGesture>(), kind),
                    param_id: id,
                });
            }
            GestureEvent::Value { id, plain } => {
                out.try_push(&sys::ClapEventParamValue {
                    header: header(
                        core::mem::size_of::<sys::ClapEventParamValue>(),
                        sys::CLAP_EVENT_PARAM_VALUE,
                    ),
                    param_id: id,
                    cookie: ptr::null_mut(),
                    note_id: -1,
                    port_index: -1,
                    channel: -1,
                    key: -1,
                    value: plain,
                });
            }
        }
    }
}

/// `src`'s bytes, cut to fit, nul-terminated.
pub(crate) fn write_c_str(dst: &mut [c_char], src: &str) {
    let take = src.len().min(dst.len().saturating_sub(1));
    for (i, b) in src.as_bytes()[..take].iter().enumerate() {
        dst[i] = *b as c_char;
    }
    dst[take] = 0;
}
