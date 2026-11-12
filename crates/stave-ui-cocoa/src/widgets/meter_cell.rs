//! Rail level-meter builder.

extern crate alloc;

use crate::view::PluginEditorView;
use crate::*;
use stave_core::editor::MeterStyle;

impl PluginEditorView {
    /// Add a StaveMeter (read-only level bar) bound to a probe; registered
    /// for the 30 Hz redraw.
    pub(crate) unsafe fn add_meter(
        &mut self,
        parent: Id,
        probe_idx: usize,
        rect: CGRect,
        style: MeterStyle,
    ) {
        unsafe {
            let cls = meter_view_class();
            let raw = msg_id(cls, sel(c"alloc"));
            let view = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            msg_void_int(view, sel(c"setWantsLayer:"), 1);
            let ed_ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
            if !ed_ivar.is_null() {
                object_setIvar(view, ed_ivar, self.self_ptr as Id);
            }
            let pi_ivar = class_getInstanceVariable(cls, c"probe_idx".as_ptr());
            if !pi_ivar.is_null() {
                object_setIvar(view, pi_ivar, probe_idx as i64 as Id);
            }
            let style_ivar = class_getInstanceVariable(cls, c"meter_style".as_ptr());
            if !style_ivar.is_null() {
                let style_value = match style {
                    MeterStyle::Dots => 0_i64,
                    MeterStyle::Segments => 1_i64,
                };
                object_setIvar(view, style_ivar, style_value as Id);
            }
            msg_void_id(parent, sel(c"addSubview:"), view);
            self.meters.push((probe_idx, view));
        }
    }
}
