//! Off macOS the player shows no plugin editor: the editor embed is an
//! NSView floated over the egui surface (`editor_view.rs`). Plugin faces
//! run natively on Windows and Linux inside a DAW; embedding them here
//! waits on a native child window of the egui surface.

use std::ffi::c_void;

use stave_vst3_sys::FUnknown;

/// Never constructed off macOS.
pub struct EditorView {
    pub width: f64,
    pub height: f64,
}

impl EditorView {
    /// No editor off macOS.
    ///
    /// # Safety
    /// None needed; the signature matches the macOS view's.
    pub unsafe fn attach(_owner: *mut FUnknown, _parent: *mut c_void) -> Option<Self> {
        None
    }

    pub fn place(&self, _x: f64, _y_top: f64, _w: f64, _h: f64, _window_h: f64, _hidden: bool) {}

    pub fn screen_rect(&self) -> Option<[f64; 4]> {
        None
    }

    /// # Safety
    /// None needed; the signature matches the macOS view's.
    pub unsafe fn detach(self) {}
}
