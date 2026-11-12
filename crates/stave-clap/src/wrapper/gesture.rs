//! A face's edits on their way to the host: the GUI thread queues
//! them, `process`/`flush` drain them into the host's out-events (so the
//! DAW records and undoes them).

use super::*;

/// The controller a face of this instance edits through (what `create`
/// gives its view): the table, the telemetry and the factory presets,
/// every edit through the host's event queue.
///
/// # Safety
/// `plugin` is a live pointer `create_plugin` returned for this `P`.
pub unsafe fn controller<P: ClapExport>(plugin: *const sys::ClapPlugin) -> PluginController {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    controller_of(inst)
}

pub(super) fn controller_of<P: ClapExport>(inst: &Instance<P>) -> PluginController {
    let gestures = ClapGestures {
        queue: inst.gestures.clone(),
        host: inst.host,
    };
    PluginController::new(
        inst.param_table.clone(),
        inst.telemetry.clone(),
        inst.presets.clone(),
        Box::new(gestures),
    )
}

/// The [`GestureSink`] of a CLAP face: the instance's queue, and a flush
/// request so a stopped transport still hears the edit. The
/// `PluginController` has already written the table.
pub(super) struct ClapGestures {
    pub(super) queue: Arc<GestureQueue>,
    pub(super) host: Option<Host>,
}

impl ClapGestures {
    fn push(&self, event: GestureEvent) {
        self.queue.push(event);
        if let Some(host) = self.host {
            host.request_flush();
        }
    }
}

impl GestureSink for ClapGestures {
    fn begin(&self, id: ParamId) {
        self.push(GestureEvent::Begin { id });
    }
    fn perform(&self, id: ParamId, value: f64) {
        self.push(GestureEvent::Value { id, value });
    }
    fn end(&self, id: ParamId) {
        self.push(GestureEvent::End { id });
    }
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
                    param_id: id.0,
                });
            }
            GestureEvent::Value { id, value } => {
                out.try_push(&sys::ClapEventParamValue {
                    header: header(
                        core::mem::size_of::<sys::ClapEventParamValue>(),
                        sys::CLAP_EVENT_PARAM_VALUE,
                    ),
                    param_id: id.0,
                    cookie: ptr::null_mut(),
                    note_id: -1,
                    port_index: -1,
                    channel: -1,
                    key: -1,
                    value,
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
