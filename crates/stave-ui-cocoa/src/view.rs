//! The plugin editor: the thin host of a face. A wrapper creates one per
//! view, hands it the plugin's face and presets, then attaches it to the
//! host's NSView and asks it for its size.

extern crate alloc;
use alloc::{boxed::Box, sync::Arc};

use stave_core::Telemetry;

use crate::Id;

pub struct PluginEditorView {
    pub(crate) telemetry: Arc<Telemetry>,
    pub(crate) binding: crate::binding::EditorBinding,
    /// The plugin's face, when it declares one.
    pub(crate) face: Option<Box<crate::face::FaceEditor>>,
}

impl PluginEditorView {
    pub fn new(
        telemetry: Arc<Telemetry>,
        table: Arc<dyn stave_core::params::Params>,
        host: Box<dyn crate::binding::EditorHost>,
    ) -> Box<Self> {
        Box::new(Self {
            telemetry,
            binding: crate::binding::EditorBinding { table, host },
            face: None,
        })
    }

    /// The editor's size in points: the face's, if there is one.
    pub fn size(&self) -> Option<(f64, f64)> {
        self.face_size()
    }

    /// Mount the face in the host's view. A plugin without a face (or
    /// without parameters) shows nothing.
    ///
    /// # Safety
    /// Main thread; `parent_ns_view` is null or a live NSView.
    pub unsafe fn attach(&mut self, parent_ns_view: Id) {
        if parent_ns_view.is_null() || self.param_count() == 0 {
            return;
        }
        unsafe { self.attach_face(parent_ns_view) };
    }

    /// Remove the face view (and stop its timer).
    ///
    /// # Safety
    /// Main thread.
    pub unsafe fn detach(&mut self) {
        unsafe { self.detach_face() };
    }
}

impl Drop for PluginEditorView {
    fn drop(&mut self) {
        unsafe { self.detach() };
    }
}
