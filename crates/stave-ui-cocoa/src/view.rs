//! [`FaceView`]: a `stave_face::Face` in one flipped NSView the size of
//! the face. It renders the face runtime's canvas, forwards the pointer
//! (down, drag, up, wheel, hover, exit) with its modifiers and click
//! count, sets the cursor the runtime asks for, and repaints from its own
//! timer only when the runtime says something changed. Everything the
//! face reads and writes goes through its [`Controller`]; the view is a
//! window, a pointer, a blit and text.
//!
//! One root reaches the face from Objective-C: the NSView's `face` ivar,
//! a counted reference to the view's [`Shared`] state. Every callback
//! takes its own count for the duration of the call, so a callback that
//! (through the host) closes the editor never frees what it is using; a
//! callback that arrives while another is running (a nested event loop)
//! is dropped.

use core::cell::RefCell;
use core::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

extern crate alloc;
use alloc::boxed::Box;

use stave_core::Controller;
use stave_face::{Cursor, Face, Mods, Runtime};
use stave_raster::Canvas;
use stave_ui::scene::{pt, Pt};

use crate::*;

/// What a key press does: return a face to show instead (the view, and
/// its window, resize to it), or `None` to keep the current one.
pub type OnKey = Box<dyn FnMut(u16) -> Option<Face>>;

/// A face shown in an NSView, over its controller. Create it, read its
/// [`size`](Self::size), [`attach`](Self::attach) it to a parent view;
/// dropping it (or [`detach`](Self::detach)) removes the view.
///
/// Not `Send`: a view lives and dies on the thread that attaches it,
/// which `attach` requires to be the main thread.
pub struct FaceView {
    shared: Rc<Shared>,
    mount: Option<Mount>,
}

/// What the NSView's callbacks reach: the controller, and the face's
/// runtime behind the reentrancy guard (`try_borrow_mut`).
struct Shared {
    controller: Box<dyn Controller>,
    ui: RefCell<Ui>,
}

struct Ui {
    runtime: Runtime,
    canvas: Canvas,
    /// The clock animations read: seconds since the face was shown.
    opened: Instant,
    on_key: Option<OnKey>,
}

impl Ui {
    fn time(&self) -> f64 {
        self.opened.elapsed().as_secs_f64()
    }
}

/// The NSView in its parent, and its repaint timer.
struct Mount {
    view: Id,
    timer: CFRunLoopTimerRef,
}

impl FaceView {
    /// `face` over `controller`. Touches no Cocoa until `attach`.
    pub fn new(face: Face, controller: Box<dyn Controller>) -> Self {
        FaceView {
            shared: Rc::new(Shared {
                controller,
                ui: RefCell::new(Ui {
                    runtime: Runtime::new(face),
                    canvas: Canvas::new(0, 0),
                    opened: Instant::now(),
                    on_key: None,
                }),
            }),
            mount: None,
        }
    }

    /// Hand every key press to `on_key` (a stand-alone window's keys; a
    /// plugin window's belong to the host).
    pub fn on_key(&mut self, on_key: impl FnMut(u16) -> Option<Face> + 'static) {
        self.shared.ui.borrow_mut().on_key = Some(Box::new(on_key));
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.shared.ui.borrow().runtime.size()
    }

    /// The controller the face reads and writes.
    pub fn controller(&self) -> &dyn Controller {
        &*self.shared.controller
    }

    /// The NSView, while attached.
    pub fn ns_view(&self) -> Option<*mut c_void> {
        self.mount.as_ref().map(|m| m.view)
    }

    /// Show the face in `parent` (replacing any earlier attachment). A
    /// null parent shows nothing.
    ///
    /// # Safety
    /// Main thread; `parent` is null or a live NSView.
    pub unsafe fn attach(&mut self, parent: *mut c_void) {
        self.detach();
        if parent.is_null() {
            return;
        }
        let size = {
            let mut ui = self.shared.ui.borrow_mut();
            ui.opened = Instant::now();
            ui.runtime.size()
        };
        // SAFETY: this function's contract.
        self.mount = Some(unsafe { Mount::new(parent, &self.shared, size) });
    }

    /// Remove the view from its parent and stop its timer.
    pub fn detach(&mut self) {
        if let Some(mount) = self.mount.take() {
            // SAFETY: a mount exists only after `attach`, on the main
            // thread, and `FaceView` is not `Send`: this is that thread.
            unsafe { mount.remove() };
        }
    }
}

impl Drop for FaceView {
    fn drop(&mut self) {
        self.detach();
    }
}

impl Mount {
    /// # Safety
    /// Main thread; `parent` is a live NSView.
    unsafe fn new(parent: Id, shared: &Rc<Shared>, size: (f64, f64)) -> Self {
        unsafe {
            let raw = msg_id(face_view_class() as Id, sel(c"alloc"));
            let rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: size.0, height: size.1 },
            };
            let view = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            set_shared(view, Rc::into_raw(Rc::clone(shared)));
            install_tracking(view);
            msg_void_id(parent, sel(c"addSubview:"), view);
            let mut context = CFRunLoopTimerContext {
                version: 0,
                info: view,
                retain: None,
                release: None,
                copy_description: None,
            };
            let interval = 1.0 / 60.0;
            let timer = CFRunLoopTimerCreate(
                core::ptr::null(),
                CFAbsoluteTimeGetCurrent() + interval,
                interval,
                0,
                0,
                tick,
                &mut context,
            );
            CFRunLoopAddTimer(CFRunLoopGetCurrent(), timer, kCFRunLoopCommonModes);
            Mount { view, timer }
        }
    }

    /// # Safety
    /// Main thread.
    unsafe fn remove(self) {
        unsafe {
            CFRunLoopTimerInvalidate(self.timer);
            CFRelease(self.timer.cast_const());
            let shared = take_shared(self.view);
            msg_id(self.view, sel(c"removeFromSuperview"));
            msg_id(self.view, sel(c"release"));
            drop(shared);
        }
    }
}

unsafe extern "C" fn tick(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    let view = info as Id;
    let dirty = unsafe { with_ui(view, |shared, ui| ui.runtime.needs_paint(&*shared.controller, ui.time())) };
    if dirty == Some(true) {
        unsafe { redraw(view) };
    }
}

// ------------------------------------------------------------- the NSView

unsafe fn face_ivar(view: Id) -> *mut c_void {
    unsafe { class_getInstanceVariable(object_getClass(view), c"face".as_ptr()) }
}

unsafe fn set_shared(view: Id, shared: *const Shared) {
    unsafe { object_setIvar(view, face_ivar(view), shared.cast_mut().cast()) };
}

/// Clear the view's reference to its state, returning it.
unsafe fn take_shared(view: Id) -> Option<Rc<Shared>> {
    unsafe {
        let p = object_getIvar(view, face_ivar(view)) as *const Shared;
        set_shared(view, core::ptr::null());
        (!p.is_null()).then(|| Rc::from_raw(p))
    }
}

/// The view's state, counted for the caller: `None` once detached.
unsafe fn shared_of(view: Id) -> Option<Rc<Shared>> {
    unsafe {
        let p = object_getIvar(view, face_ivar(view)) as *const Shared;
        if p.is_null() {
            return None;
        }
        Rc::increment_strong_count(p);
        Some(Rc::from_raw(p))
    }
}

/// Run `f` on the face behind `view`: `None` once detached, or when
/// another callback is running (a reentrant event is dropped).
unsafe fn with_ui<R>(view: Id, f: impl FnOnce(&Shared, &mut Ui) -> R) -> Option<R> {
    let shared = unsafe { shared_of(view) }?;
    let mut ui = shared.ui.try_borrow_mut().ok()?;
    Some(f(&shared, &mut ui))
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
    unsafe {
        with_ui(this, |shared, ui| {
            let t = ui.time();
            let (w, h) = ui.runtime.size();
            let mut text = crate::offscreen::CoreText::new(scale);
            let Ui { runtime, canvas, .. } = ui;
            runtime.render(&*shared.controller, scale, t, canvas, &mut text);
            crate::offscreen::draw_canvas(ctx, canvas, w, h);
        });
    }
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
    let (p, mods) = unsafe { (point_in(this, event), mods_of(event)) };
    let clicks: i64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> i64 = core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"clickCount"))
    };
    unsafe {
        let win = msg_id(this, sel(c"window"));
        if !win.is_null() {
            msg_void_id(win, sel(c"makeFirstResponder:"), this);
        }
        with_ui(this, |shared, ui| {
            ui.runtime.pointer_down(&*shared.controller, p, mods, clicks.max(1) as u32);
            set_cursor(ui.runtime.cursor());
        });
        redraw(this);
    }
}

unsafe extern "C" fn mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let (p, mods) = unsafe { (point_in(this, event), mods_of(event)) };
    unsafe {
        with_ui(this, |shared, ui| ui.runtime.pointer_drag(&*shared.controller, p, mods));
        redraw(this);
    }
}

unsafe extern "C" fn mouse_up(this: Id, _cmd: Sel, event: Id) {
    let p = unsafe { point_in(this, event) };
    unsafe {
        with_ui(this, |shared, ui| {
            ui.runtime.pointer_up(&*shared.controller);
            ui.runtime.pointer_move(&*shared.controller, p);
            set_cursor(ui.runtime.cursor());
        });
        redraw(this);
    }
}

unsafe extern "C" fn mouse_moved(this: Id, _cmd: Sel, event: Id) {
    let p = unsafe { point_in(this, event) };
    let changed = unsafe {
        with_ui(this, |shared, ui| {
            let changed = ui.runtime.pointer_move(&*shared.controller, p);
            set_cursor(ui.runtime.cursor());
            changed
        })
    };
    if changed == Some(true) {
        unsafe { redraw(this) };
    }
}

unsafe extern "C" fn mouse_exited(this: Id, _cmd: Sel, _event: Id) {
    unsafe {
        with_ui(this, |_, ui| ui.runtime.pointer_exit());
        set_cursor(Cursor::Arrow);
        redraw(this);
    }
}

unsafe extern "C" fn scroll_wheel(this: Id, _cmd: Sel, event: Id) {
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
    unsafe {
        with_ui(this, |shared, ui| ui.runtime.wheel(&*shared.controller, p, lines, mods));
        redraw(this);
    }
}

unsafe extern "C" fn key_down(this: Id, _cmd: Sel, event: Id) {
    let code: u16 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> u16 = core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"keyCode"))
    };
    let resized = unsafe {
        with_ui(this, |_, ui| {
            let face = ui.on_key.as_mut().and_then(|on_key| on_key(code))?;
            ui.runtime = Runtime::new(face);
            ui.opened = Instant::now();
            Some(ui.runtime.size())
        })
    };
    if let Some(Some(size)) = resized {
        unsafe {
            let set: unsafe extern "C" fn(Id, Sel, CGSize) = core::mem::transmute(objc_msgSend as *const ());
            set(this, sel(c"setFrameSize:"), CGSize { width: size.0, height: size.1 });
            let win = msg_id(this, sel(c"window"));
            if !win.is_null() {
                set(win, sel(c"setContentSize:"), CGSize { width: size.0, height: size.1 });
            }
        }
    }
    unsafe { redraw(this) };
}

unsafe extern "C" fn yes(_this: Id, _cmd: Sel) -> bool {
    true
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
        msg_id(area, sel(c"release"));
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
            c"face".as_ptr(),
            core::mem::size_of::<*mut c_void>(),
            core::mem::align_of::<*mut c_void>().trailing_zeros() as u8,
            c"^v".as_ptr(),
        );
        let _ = class_addMethod(cls, sel(c"acceptsFirstMouse:"), yes_event as *const c_void, c"c@:@".as_ptr());
        let _ = class_addMethod(cls, sel(c"acceptsFirstResponder"), yes as *const c_void, c"c@:".as_ptr());
        let _ = class_addMethod(cls, sel(c"drawRect:"), draw_rect as *const c_void, c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr());
        for (name, imp) in [
            (c"mouseDown:", mouse_down as *const c_void),
            (c"mouseDragged:", mouse_dragged as *const c_void),
            (c"mouseUp:", mouse_up as *const c_void),
            (c"mouseMoved:", mouse_moved as *const c_void),
            (c"mouseExited:", mouse_exited as *const c_void),
            (c"scrollWheel:", scroll_wheel as *const c_void),
            (c"keyDown:", key_down as *const c_void),
        ] {
            let _ = class_addMethod(cls, sel(name), imp, c"v@:@".as_ptr());
        }
        objc_registerClassPair(cls);
        cls
    }
}
