//! The plugin's OWN editor as stave-play's primary view: the LIVE
//! component (the one the audio thread is processing) is queried for
//! `IEditController` — stave plugins are single-object, so the
//! editor binds to the playing instance and its meters/waves/tails
//! draw from the real bus. The IPlugView attaches to a container
//! NSView we float over the egui surface and position every frame
//! to the rect the Plugin tab reserves.
//!
//! Teardown order matters: `detach` MUST run before the audio
//! stream (which owns the plugin) drops — reload_plugin does.

// Unsafe here is Objective-C runtime FFI, documented with the UI overhaul
// (PLAN S6), not block by block before it.
#![allow(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc)]

use std::ffi::c_void;
use std::ptr;

use stave_vst3_sys::{
    FUnknown, IEditControllerVtbl, IPlugViewVtbl, Tresult, Tuid, ViewRect, IID_IEDIT_CONTROLLER,
    K_RESULT_OK,
};

/// The COM prefix every VST3 vtbl starts with (sys has no standalone
/// FUnknown vtbl type; this layout-matches the leading triple).
#[repr(C)]
struct UnknownVtbl {
    query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
}

// ---------------------------------------------------------------- objc
type Id = *mut c_void;
type Sel = *const c_void;
extern "C" {
    fn objc_msgSend();
    fn sel_registerName(name: *const i8) -> Sel;
    fn objc_getClass(name: *const i8) -> Id;
}
#[repr(C)]
#[derive(Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
struct CGSize {
    w: f64,
    h: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
unsafe fn sel(name: &core::ffi::CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}
unsafe fn msg_id(obj: Id, s: Sel) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s) }
}
unsafe fn msg_void_id(obj: Id, s: Sel, a: Id) {
    let f: unsafe extern "C" fn(Id, Sel, Id) =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, a) }
}
unsafe fn msg_void_int(obj: Id, s: Sel, a: i64) {
    let f: unsafe extern "C" fn(Id, Sel, i64) =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, a) }
}
unsafe fn msg_id_rect(obj: Id, s: Sel, r: CGRect) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, CGRect) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, r) }
}
unsafe fn msg_void_rect(obj: Id, s: Sel, r: CGRect) {
    let f: unsafe extern "C" fn(Id, Sel, CGRect) =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, r) }
}
unsafe fn msg_void_size(obj: Id, s: Sel, sz: CGSize) {
    let f: unsafe extern "C" fn(Id, Sel, CGSize) =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s, sz) }
}

// ------------------------------------------------------------ the view
#[repr(C)]
struct Shape {
    vtbl: *const c_void,
}

pub struct EditorView {
    controller: *mut FUnknown,
    view: *mut FUnknown,
    view_vtbl: *const IPlugViewVtbl,
    container: Id,
    pub width: f64,
    pub height: f64,
}

impl EditorView {
    /// Bind the live instance's editor into `parent` (the window's
    /// root NSView). `owner` is the playing instance's controller (or a
    /// single-object component, which IS its controller — a second
    /// createInstance would edit a ghost). None when there is no editor.
    ///
    /// SAFETY: `owner` must be a live VST3 object; `parent` a live
    /// NSView; call on the main thread only.
    pub unsafe fn attach(owner: *mut FUnknown, parent: *mut c_void) -> Option<Self> {
        if owner.is_null() || parent.is_null() {
            return None;
        }
        let unk_vtbl = unsafe { (*(owner as *const Shape)).vtbl as *const UnknownVtbl };
        let mut ctrl: *mut c_void = ptr::null_mut();
        let res = unsafe {
            ((*unk_vtbl).query_interface)(owner, &IID_IEDIT_CONTROLLER as *const Tuid, &mut ctrl)
        };
        if res != K_RESULT_OK || ctrl.is_null() {
            return None;
        }
        let controller = ctrl as *mut FUnknown;
        let ctrl_vtbl =
            unsafe { (*(controller as *const Shape)).vtbl as *const IEditControllerVtbl };
        let view = unsafe { ((*ctrl_vtbl).create_view)(controller, c"editor".as_ptr()) };
        if view.is_null() {
            unsafe { release(controller) };
            return None;
        }
        let view_vtbl = unsafe { (*(view as *const Shape)).vtbl as *const IPlugViewVtbl };
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe { ((*view_vtbl).get_size)(view, &mut rect) };
        let width = f64::from(rect.right - rect.left);
        let height = f64::from(rect.bottom - rect.top);
        if width <= 0.0 || height <= 0.0 {
            unsafe { release(view) };
            unsafe { release(controller) };
            return None;
        }
        let container = unsafe {
            let cls = objc_getClass(c"NSView".as_ptr());
            let raw = msg_id(cls, sel(c"alloc"));
            let frame = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    w: width,
                    h: height,
                },
            };
            let v = msg_id_rect(raw, sel(c"initWithFrame:"), frame);
            msg_void_id(parent as Id, sel(c"addSubview:"), v);
            msg_void_int(v, sel(c"setHidden:"), 1);
            v
        };
        let res = unsafe { ((*view_vtbl).attached)(view, container, c"NSView".as_ptr()) };
        if res != K_RESULT_OK {
            unsafe {
                msg_void_id(container, sel(c"removeFromSuperview"), ptr::null_mut());
                release(view);
                release(controller);
            }
            return None;
        }
        Some(Self {
            controller,
            view,
            view_vtbl,
            container,
            width,
            height,
        })
    }

    /// Position the editor centered in a top-left-origin rect (egui
    /// points), given the window content height (AppKit's origin is
    /// the bottom-left). When the rect is smaller than the editor's
    /// natural size, the whole view SCALES to fit (frame carries the
    /// fitted size, bounds keep the natural size — AppKit scales the
    /// drawing and the mouse coordinates together), so the editor
    /// never overflows onto the keyboard or the panels.
    pub fn place(&self, x: f64, y_top: f64, w: f64, h: f64, window_h: f64, hidden: bool) {
        unsafe {
            msg_void_int(self.container, sel(c"setHidden:"), i64::from(hidden));
            if hidden {
                return;
            }
            let scale = (w / self.width).min(h / self.height).clamp(0.05, 1.0);
            let fw = self.width * scale;
            let fh = self.height * scale;
            let ex = x + ((w - fw) * 0.5).max(0.0);
            let ey_top = y_top + ((h - fh) * 0.5).max(0.0);
            let frame = CGRect {
                origin: CGPoint {
                    x: ex,
                    y: window_h - ey_top - fh,
                },
                size: CGSize { w: fw, h: fh },
            };
            msg_void_rect(self.container, sel(c"setFrame:"), frame);
            msg_void_size(
                self.container,
                sel(c"setBoundsSize:"),
                CGSize {
                    w: self.width,
                    h: self.height,
                },
            );
        }
    }

    /// The editor's on-screen rectangle as `[x, y, w, h]` in global
    /// points, origin top-left of the primary display (what
    /// `screencapture -R` takes).
    pub fn screen_rect(&self) -> Option<[f64; 4]> {
        // SAFETY: objc_msgSend cast to each message's exact signature
        // (arm64: struct returns need no _stret); `container` is live
        // until `detach`.
        unsafe {
            let window = msg_id(self.container, sel(c"window"));
            if window.is_null() {
                return None;
            }
            let rect_of: unsafe extern "C" fn(Id, Sel) -> CGRect =
                std::mem::transmute(objc_msgSend as *const ());
            let to_view: unsafe extern "C" fn(Id, Sel, CGRect, Id) -> CGRect =
                std::mem::transmute(objc_msgSend as *const ());
            let to_screen: unsafe extern "C" fn(Id, Sel, CGRect) -> CGRect =
                std::mem::transmute(objc_msgSend as *const ());
            let bounds = rect_of(self.container, sel(c"bounds"));
            let in_window = to_view(
                self.container,
                sel(c"convertRect:toView:"),
                bounds,
                ptr::null_mut(),
            );
            let on_screen = to_screen(window, sel(c"convertRectToScreen:"), in_window);
            let screens = msg_id(objc_getClass(c"NSScreen".as_ptr()), sel(c"screens"));
            let primary = msg_id(screens, sel(c"firstObject"));
            if primary.is_null() {
                return None;
            }
            let display = rect_of(primary, sel(c"frame"));
            Some([
                on_screen.origin.x,
                display.size.h - on_screen.origin.y - on_screen.size.h,
                on_screen.size.w,
                on_screen.size.h,
            ])
        }
    }

    /// Tear down in spec order. MUST run before the plugin instance
    /// (owned by the audio stream) is dropped.
    pub unsafe fn detach(self) {
        unsafe {
            ((*self.view_vtbl).removed)(self.view);
            release(self.view);
            release(self.controller);
            msg_void_id(self.container, sel(c"removeFromSuperview"), ptr::null_mut());
            msg_void_id(self.container, sel(c"release"), ptr::null_mut());
        }
    }
}

unsafe fn release(obj: *mut FUnknown) {
    let vtbl = unsafe { (*(obj as *const Shape)).vtbl as *const UnknownVtbl };
    unsafe { ((*vtbl).release)(obj) };
}
