//! The macOS host for a `stave_face::Face`: one flipped NSView the size of
//! the face. It renders the face runtime's canvas, forwards the pointer
//! (down, drag, up, wheel, hover, exit) with its modifiers and click
//! count, sets the cursor the runtime asks for, and repaints from the
//! editor's 30 Hz tick only when the runtime says something changed.
//!
//! Parameters flow through [`Bridge`]: reads from the plugin's typed
//! table, edits through the host's gesture path (so the DAW records and
//! undoes them), telemetry from the debug bus.

use core::ffi::c_void;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::time::Instant;

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::sync::Arc;

use stave_core::params::ParamId;
use stave_core::DebugBus;
use stave_face::{Cursor, Face, Host, Mods, Runtime};
use stave_raster::Canvas;
use stave_ui::scene::{pt, Pt};

use crate::binding::EditorBinding;
use crate::view::PluginEditorView;
use crate::*;

/// Param ids and probe names resolved to table and bus slots.
pub(crate) struct Slots {
    ids: HashMap<u32, usize>,
    probes: HashMap<&'static str, usize>,
}

pub(crate) struct FaceEditor {
    pub(crate) runtime: Runtime,
    slots: Slots,
    view: Id,
    canvas: Canvas,
    opened: Instant,
}

/// `stave_face::Host` over one editor's table, gesture path and bus.
struct Bridge<'a> {
    binding: &'a EditorBinding,
    bus: &'a Arc<DebugBus>,
    slots: &'a Slots,
}

impl Bridge<'_> {
    fn ci(&self, id: u32) -> Option<usize> {
        self.slots.ids.get(&id).copied()
    }
}

impl Host for Bridge<'_> {
    fn normalized(&self, id: u32) -> f64 {
        self.ci(id).map_or(0.0, |ci| self.binding.param(ci).normalized())
    }
    fn default_normalized(&self, id: u32) -> f64 {
        self.ci(id).map_or(0.0, |ci| self.binding.param(ci).default_normalized())
    }
    fn steps(&self, id: u32) -> u32 {
        self.ci(id).map_or(0, |ci| self.binding.param(ci).step_count())
    }
    fn step_name(&self, id: u32, index: u32) -> Option<String> {
        let ci = self.ci(id)?;
        self.binding.param(ci).step_name(index).map(ToString::to_string)
    }
    fn value_text(&self, id: u32) -> String {
        self.ci(id).map_or_else(String::new, |ci| {
            let p = self.binding.param(ci);
            p.format_plain(p.plain()).as_str().to_string()
        })
    }
    fn probe(&self, name: &str) -> f64 {
        self.slots
            .probes
            .get(name)
            .map_or(0.0, |&pi| f64::from_bits(self.bus.probes[pi].latest.load(Ordering::Relaxed)))
    }
    fn begin(&mut self, id: u32) {
        if self.ci(id).is_some() {
            self.binding.host.begin_edit(id);
        }
    }
    fn set(&mut self, id: u32, normalized: f64) {
        if let Some(ci) = self.ci(id) {
            let p = self.binding.param(ci);
            self.binding.host.perform_edit(id, p.normalized_to_plain(normalized.clamp(0.0, 1.0)));
        }
    }
    fn end(&mut self, id: u32) {
        if self.ci(id).is_some() {
            self.binding.host.end_edit(id);
        }
    }
}

/// Split borrows: the runtime mutably, everything the bridge reads
/// immutably.
fn parts(editor: &mut PluginEditorView) -> Option<(&mut FaceEditor, Bridge<'_>)> {
    let PluginEditorView { face, binding, bus, .. } = editor;
    let face = face.as_deref_mut()?;
    let FaceEditor { slots, .. } = face;
    let slots: *const Slots = slots;
    // SAFETY: `slots` is never mutated after `set_face` builds it, and
    // the bridge only reads it; the runtime borrow touches other fields.
    let bridge = Bridge {
        binding,
        bus,
        slots: unsafe { &*slots },
    };
    Some((face, bridge))
}

impl PluginEditorView {
    /// Use `face` as this editor (instead of the declared `ui!` tree).
    pub fn set_face(&mut self, face: Option<Face>) {
        self.face = face.map(|f| {
            let runtime = Runtime::new(f);
            let mut ids = HashMap::new();
            for b in runtime.binds() {
                if let Some(ci) = self.param_index_of(ParamId(b.id)) {
                    ids.insert(b.id, ci);
                }
            }
            let mut probes = HashMap::new();
            for name in runtime.probes() {
                if let Some(pi) = crate::mapping::find_or_reserve_probe(&self.bus, name) {
                    probes.insert(name, pi);
                }
            }
            Box::new(FaceEditor {
                runtime,
                slots: Slots { ids, probes },
                view: core::ptr::null_mut(),
                canvas: Canvas::new(0, 0),
                opened: Instant::now(),
            })
        });
    }

    pub(crate) fn face_size(&self) -> Option<(f64, f64)> {
        self.face.as_ref().map(|f| f.runtime.size())
    }

    /// Mount the face view in `parent`. False when there is no face.
    pub(crate) unsafe fn attach_face(&mut self, parent: Id) -> bool {
        let me: *mut PluginEditorView = self;
        let Some(face) = self.face.as_deref_mut() else {
            return false;
        };
        let (w, h) = face.runtime.size();
        unsafe {
            let cls = face_view_class();
            let raw = msg_id(cls as Id, sel(c"alloc"));
            let rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: w, height: h },
            };
            let view = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            set_editor(view, me);
            install_tracking(view);
            msg_void_id(parent, sel(c"addSubview:"), view);
            face.view = view;
            face.opened = Instant::now();
        }
        true
    }

    pub(crate) unsafe fn detach_face(&mut self) {
        if let Some(face) = self.face.as_deref_mut() {
            if !face.view.is_null() {
                unsafe {
                    set_editor(face.view, core::ptr::null_mut());
                    msg_id(face.view, sel(c"removeFromSuperview"));
                }
                face.view = core::ptr::null_mut();
            }
        }
    }

    /// The 30 Hz tick: ask for a paint only when the face changed.
    pub(crate) fn face_tick(&mut self) {
        let Some((face, bridge)) = parts(self) else {
            return;
        };
        if face.view.is_null() {
            return;
        }
        let t = face.opened.elapsed().as_secs_f64();
        if face.runtime.needs_paint(&bridge, t) {
            unsafe { msg_void_int(face.view, sel(c"setNeedsDisplay:"), 1) };
        }
    }
}

// ------------------------------------------------------------- the NSView

unsafe fn set_editor(view: Id, editor: *mut PluginEditorView) {
    unsafe {
        let ivar = class_getInstanceVariable(object_getClass(view), c"editor_ptr".as_ptr());
        object_setIvar(view, ivar, editor as Id);
    }
}

unsafe fn editor_of(view: Id) -> Option<&'static mut PluginEditorView> {
    unsafe {
        let ivar = class_getInstanceVariable(object_getClass(view), c"editor_ptr".as_ptr());
        let p = object_getIvar(view, ivar) as *mut PluginEditorView;
        p.as_mut()
    }
}

unsafe fn point_in(view: Id, event: Id) -> Pt {
    unsafe {
        let loc: unsafe extern "C" fn(Id, Sel) -> CGPoint = core::mem::transmute(objc_msgSend as *const ());
        let win = loc(event, sel(c"locationInWindow"));
        let conv: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint = core::mem::transmute(objc_msgSend as *const ());
        let p = conv(view, sel(c"convertPoint:fromView:"), win, core::ptr::null_mut());
        // The view is y-up (so the canvas blits upright); faces are y-down.
        let bounds: unsafe extern "C" fn(Id, Sel) -> CGRect = core::mem::transmute(objc_msgSend as *const ());
        let h = bounds(view, sel(c"bounds")).size.height;
        pt(p.x, h - p.y)
    }
}

unsafe fn mods_of(event: Id) -> Mods {
    let flags: u64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> u64 = core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"modifierFlags"))
    };
    const SHIFT: u64 = 1 << 17;
    const OPTION: u64 = 1 << 19;
    const COMMAND: u64 = 1 << 20;
    Mods {
        fine: flags & (SHIFT | COMMAND) != 0,
        reset: flags & OPTION != 0,
    }
}

unsafe fn set_cursor(c: Cursor) {
    let name: &core::ffi::CStr = match c {
        Cursor::Arrow => c"arrowCursor",
        Cursor::Grab => c"openHandCursor",
        Cursor::Grabbing => c"closedHandCursor",
        Cursor::Point => c"pointingHandCursor",
    };
    unsafe {
        let cur = msg_id(class(c"NSCursor") as Id, sel(name));
        msg_id(cur, sel(c"set"));
    }
}

unsafe fn redraw(view: Id) {
    unsafe { msg_void_int(view, sel(c"setNeedsDisplay:"), 1) };
}

unsafe extern "C" fn draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let ctx: *mut c_void = unsafe {
        let cur: Id = msg_id(class(c"NSGraphicsContext") as Id, sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void = core::mem::transmute(objc_msgSend as *const ());
        f(cur, sel(c"CGContext"))
    };
    if ctx.is_null() {
        return;
    }
    let scale = unsafe { backing_scale(this) };
    let Some((face, bridge)) = parts(editor) else {
        return;
    };
    let t = face.opened.elapsed().as_secs_f64();
    let (w, h) = face.runtime.size();
    let mut text = crate::offscreen::CoreText::new(scale);
    face.runtime.render(&bridge, scale, t, &mut face.canvas, &mut text);
    unsafe { crate::offscreen::draw_canvas(ctx, &face.canvas, w, h) };
}

unsafe fn backing_scale(view: Id) -> f64 {
    unsafe {
        let win: Id = msg_id(view, sel(c"window"));
        if win.is_null() {
            return 2.0;
        }
        let f: unsafe extern "C" fn(Id, Sel) -> f64 = core::mem::transmute(objc_msgSend as *const ());
        let s = f(win, sel(c"backingScaleFactor"));
        if s.is_finite() && s >= 1.0 { s } else { 2.0 }
    }
}

unsafe extern "C" fn mouse_down(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let (p, mods) = unsafe { (point_in(this, event), mods_of(event)) };
    let clicks: i64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> i64 = core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"clickCount"))
    };
    let Some((face, mut bridge)) = parts(editor) else {
        return;
    };
    face.runtime.pointer_down(&mut bridge, p, mods, clicks.max(1) as u32);
    unsafe {
        set_cursor(face.runtime.cursor());
        redraw(this);
    }
}

unsafe extern "C" fn mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let (p, mods) = unsafe { (point_in(this, event), mods_of(event)) };
    let Some((face, mut bridge)) = parts(editor) else {
        return;
    };
    face.runtime.pointer_drag(&mut bridge, p, mods);
    unsafe { redraw(this) };
}

unsafe extern "C" fn mouse_up(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let p = unsafe { point_in(this, event) };
    let Some((face, mut bridge)) = parts(editor) else {
        return;
    };
    face.runtime.pointer_up(&mut bridge);
    face.runtime.pointer_move(p);
    unsafe {
        set_cursor(face.runtime.cursor());
        redraw(this);
    }
}

unsafe extern "C" fn mouse_moved(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let p = unsafe { point_in(this, event) };
    let Some(face) = editor.face.as_deref_mut() else {
        return;
    };
    if face.runtime.pointer_move(p) {
        unsafe { redraw(this) };
    }
    unsafe { set_cursor(face.runtime.cursor()) };
}

unsafe extern "C" fn mouse_exited(this: Id, _cmd: Sel, _event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let Some(face) = editor.face.as_deref_mut() else {
        return;
    };
    face.runtime.pointer_exit();
    unsafe {
        set_cursor(Cursor::Arrow);
        redraw(this);
    }
}

unsafe extern "C" fn scroll_wheel(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { editor_of(this) }) else {
        return;
    };
    let (p, mods) = unsafe { (point_in(this, event), mods_of(event)) };
    let (dy, precise) = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> f64 = core::mem::transmute(objc_msgSend as *const ());
        let b: unsafe extern "C" fn(Id, Sel) -> bool = core::mem::transmute(objc_msgSend as *const ());
        (f(event, sel(c"scrollingDeltaY")), b(event, sel(c"hasPreciseScrollingDeltas")))
    };
    // Trackpads report points; a wheel notch is about ten of them.
    let lines = if precise { dy / 10.0 } else { dy };
    if lines == 0.0 {
        return;
    }
    let Some((face, mut bridge)) = parts(editor) else {
        return;
    };
    face.runtime.wheel(&mut bridge, p, lines, mods);
    unsafe { redraw(this) };
}

unsafe extern "C" fn yes_event(_this: Id, _cmd: Sel, _e: Id) -> bool {
    true
}

/// Track the pointer anywhere over the view, active even when the host
/// window is not key (plugin windows often are not).
unsafe fn install_tracking(view: Id) {
    const MOUSE_ENTERED_EXITED: u64 = 0x01;
    const MOUSE_MOVED: u64 = 0x02;
    const ACTIVE_ALWAYS: u64 = 0x80;
    const IN_VISIBLE_RECT: u64 = 0x200;
    unsafe {
        let raw = msg_id(class(c"NSTrackingArea") as Id, sel(c"alloc"));
        let init: unsafe extern "C" fn(Id, Sel, CGRect, u64, Id, Id) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let zero = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: 0.0, height: 0.0 },
        };
        let area = init(
            raw,
            sel(c"initWithRect:options:owner:userInfo:"),
            zero,
            MOUSE_ENTERED_EXITED | MOUSE_MOVED | ACTIVE_ALWAYS | IN_VISIBLE_RECT,
            view,
            core::ptr::null_mut(),
        );
        msg_void_id(view, sel(c"addTrackingArea:"), area);
    }
}

unsafe fn face_view_class() -> Class {
    unsafe {
        let existing = objc_lookUpClass(c"StaveFaceView".as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let cls = objc_allocateClassPair(class(c"NSView"), c"StaveFaceView".as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*mut c_void>(),
            core::mem::align_of::<*mut c_void>().trailing_zeros() as u8,
            c"^v".as_ptr(),
        );
        let _ = class_addMethod(cls, sel(c"acceptsFirstMouse:"), yes_event as *const c_void, c"c@:@".as_ptr());
        let _ = class_addMethod(cls, sel(c"drawRect:"), draw_rect as *const c_void, c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr());
        for (name, imp) in [
            (c"mouseDown:", mouse_down as *const c_void),
            (c"mouseDragged:", mouse_dragged as *const c_void),
            (c"mouseUp:", mouse_up as *const c_void),
            (c"mouseMoved:", mouse_moved as *const c_void),
            (c"mouseExited:", mouse_exited as *const c_void),
            (c"scrollWheel:", scroll_wheel as *const c_void),
        ] {
            let _ = class_addMethod(cls, sel(name), imp, c"v@:@".as_ptr());
        }
        objc_registerClassPair(cls);
        cls
    }
}
