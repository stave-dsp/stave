//! The Cocoa editor's gesture sink: editor edits go to the table and
//! to the host's IComponentHandler, so the DAW records and undoes them.

use crate::wrapper::{Instance, Vst3Export};

/// The editor's gesture sink: forwards knob gestures to the
/// Step-3 handler path. Holds the instance pointer — valid for the
/// editor's lifetime because the `EditorBox` owning the editor holds a
/// reference on the instance until the editor is dropped.
pub(crate) struct Vst3EditorHost<P: Vst3Export> {
    pub(crate) inst: *mut Instance<P>,
}

impl<P: Vst3Export> crate::cocoa_view::EditorHost for Vst3EditorHost<P> {
    fn begin_edit(&self, id: u32) {
        gesture_begin(self.instance(), id);
    }
    fn perform_edit(&self, id: u32, plain: f64) {
        gesture_perform(self.instance(), id, plain);
    }
    fn end_edit(&self, id: u32) {
        gesture_end(self.instance(), id);
    }
}

impl<P: Vst3Export> Vst3EditorHost<P> {
    fn instance(&self) -> &Instance<P> {
        // SAFETY: see the struct docs — the instance outlives its editor
        // (the owning view's reference).
        unsafe { &*self.inst }
    }
}

pub fn gesture_begin<P: Vst3Export>(inst: &Instance<P>, id: u32) {
    inst.handler.begin_edit(id);
}

pub fn gesture_perform<P: Vst3Export>(inst: &Instance<P>, id: u32, plain: f64) {
    let Some(p) = inst.param_table.by_id(stave_core::ParamId(id)) else {
        return;
    };
    p.set_plain(plain);
    inst.handler.perform_edit(id, p.plain_to_normalized(plain));
}

pub fn gesture_end<P: Vst3Export>(inst: &Instance<P>, id: u32) {
    inst.handler.end_edit(id);
}
