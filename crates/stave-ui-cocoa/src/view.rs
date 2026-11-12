//! [`FaceView`]: a `stave_face::Face` in one NSView the size of the face.
//! It renders the face runtime's canvas, forwards the pointer (down,
//! drag, up, wheel, hover, exit) with its modifiers and click count, sets
//! the cursor the runtime asks for, and repaints from its own timer only
//! when the runtime says something changed. Everything the face reads and
//! writes goes through its [`Controller`]; the view is a window, a
//! pointer, a blit and text.
//!
//! One root reaches the face from Objective-C: the NSView's `face` ivar,
//! a counted reference to the view's [`Shared`] state. Every callback
//! takes its own count for the duration of the call, so a callback that
//! (through the host) closes the editor never frees what it is using; a
//! callback that arrives while another is running (a nested event loop)
//! is dropped; a face that panics goes still, and the host lives.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use std::boxed::Box;

use stave_core::Controller;
use stave_face::{Cursor, Face, Mods, Runtime};
use stave_raster::Canvas;
use stave_ui::scene::{pt, Pt};

use crate::cf::{CGPoint, CGRect, CGSize, CfRef};
use crate::objc::{
    class, define_class, guard, ivar_offset, send, sel, Args, ClassRef, MainThread, Obj, ObjcBool, Ret, Retained, Sel,
    YES,
};

/// What a key press does: return a face to show instead (the view, and
/// its window, resize to it), or `None` to keep the current one.
pub type OnKey = Box<dyn FnMut(u16) -> Option<Face>>;

/// A face shown in an NSView, over its controller. Create it, read its
/// [`size`](Self::size), [`attach`](Self::attach) it to a parent view;
/// dropping it (or [`detach`](Self::detach)) removes the view.
///
/// Not `Send`: a view lives and dies on the thread that attaches it,
/// which must be the main thread.
pub struct FaceView {
    shared: Rc<Shared>,
    mount: Option<Mount>,
}

/// What the NSView's callbacks reach: the controller, and the face's
/// runtime behind the reentrancy guard (`try_borrow_mut`).
struct Shared {
    controller: Box<dyn Controller>,
    ui: RefCell<Ui>,
    /// Set when face code panicked: the view stops calling it.
    broken: Cell<bool>,
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
                broken: Cell::new(false),
            }),
            mount: None,
        }
    }

    /// Hand every key press to `on_key` (a stand-alone window's keys; a
    /// plugin window's belong to the host, so without one they go on to
    /// the next responder).
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

    /// Show the face in `parent` (replacing any earlier attachment). A
    /// null parent shows nothing.
    ///
    /// # Panics
    /// Off the main thread.
    ///
    /// # Safety
    /// `parent` is null or a live NSView.
    pub unsafe fn attach(&mut self, parent: *mut c_void) {
        self.detach();
        let Some(parent) = Obj::from_ptr(parent) else {
            return;
        };
        let main = MainThread::new().expect("a face view is attached on the main thread");
        let size = {
            let mut ui = self.shared.ui.borrow_mut();
            ui.opened = Instant::now();
            ui.runtime.size()
        };
        // SAFETY: this function's contract; on the main thread.
        self.mount = Some(unsafe { Mount::new(main, parent, &self.shared, size) });
    }

    /// Remove the view from its parent and stop its timer.
    pub fn detach(&mut self) {
        if let Some(mount) = self.mount.take() {
            mount.remove();
        }
    }

    /// The NSView, while attached (a stand-alone window makes it first
    /// responder).
    pub(crate) fn ns_view(&self) -> Option<Obj> {
        self.mount.as_ref().map(|m| m.view.get())
    }
}

impl Drop for FaceView {
    fn drop(&mut self) {
        self.detach();
    }
}

// --------------------------------------------------------- the mount

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddTimer(rl: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        fire_date: f64,
        interval: f64,
        flags: u64,
        order: isize,
        callout: extern "C" fn(timer: *mut c_void, info: *mut c_void),
        context: *mut TimerContext,
    ) -> *mut c_void;
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
    fn CFAbsoluteTimeGetCurrent() -> f64;
    static kCFRunLoopCommonModes: *const c_void;
}

/// `CFRunLoopTimerContext`.
#[repr(C)]
struct TimerContext {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> *const c_void>,
}

/// The NSView in its parent, and its repaint timer.
struct Mount {
    view: Retained,
    timer: CfRef,
}

impl Mount {
    /// # Safety
    /// `parent` is a live NSView.
    unsafe fn new(_main: MainThread, parent: Obj, shared: &Rc<Shared>, (w, h): (f64, f64)) -> Self {
        let class = face_view_class();
        // SAFETY: `alloc` and `initWithFrame:` on an NSView subclass, on
        // the main thread.
        let view = unsafe {
            let raw: Option<Obj> = send(class.class.0, sel!(c"alloc"), ());
            let view: Option<Obj> = send(raw.expect("alloc"), sel!(c"initWithFrame:"), (CGRect::new(0.0, 0.0, w, h),));
            Retained::from_owned(view.expect("an NSView initializes"))
        };
        // SAFETY: a fresh instance of the face view class; the reference
        // the ivar holds is taken back in `remove`.
        unsafe { *face_slot(view.get()) = Rc::into_raw(Rc::clone(shared)) };
        // SAFETY: a live view, on the main thread.
        unsafe { track_pointer(view.get()) };
        // SAFETY: `parent` is a live NSView (this function's contract).
        unsafe { send::<_, ()>(parent, sel!(c"addSubview:"), (view.get(),)) };
        let mut context = TimerContext {
            version: 0,
            info: view.get().as_ptr(),
            retain: None,
            release: None,
            copy_description: None,
        };
        let interval = 1.0 / 60.0;
        // SAFETY: the context is copied by the call; `info` (the view)
        // outlives the timer, which `remove` invalidates before the view
        // is released. Added to this (the main) thread's run loop.
        let timer = unsafe {
            let t = CFRunLoopTimerCreate(
                core::ptr::null(),
                CFAbsoluteTimeGetCurrent() + interval,
                interval,
                0,
                0,
                tick,
                &mut context,
            );
            let timer = CfRef::from_create(t).expect("a run loop timer");
            CFRunLoopAddTimer(CFRunLoopGetCurrent(), timer.as_ptr(), kCFRunLoopCommonModes);
            timer
        };
        Mount { view, timer }
    }

    /// On the main thread: `Mount` is made there (`new` takes the token)
    /// and is not `Send`.
    fn remove(self) {
        // SAFETY: our timer, on the thread whose run loop holds it.
        unsafe { CFRunLoopTimerInvalidate(self.timer.as_ptr()) };
        // SAFETY: the view is a live instance of the class; the slot holds
        // the reference `new` gave it (or null).
        let shared = unsafe { core::mem::replace(&mut *face_slot(self.view.get()), core::ptr::null()) };
        // SAFETY: a live view in its parent, on the main thread.
        unsafe { send::<_, ()>(self.view.get(), sel!(c"removeFromSuperview"), ()) };
        drop(self.view);
        drop(self.timer);
        if !shared.is_null() {
            // SAFETY: the reference `new` leaked into the slot.
            drop(unsafe { Rc::from_raw(shared) });
        }
    }
}

extern "C" fn tick(_timer: *mut c_void, info: *mut c_void) {
    guard((), || {
        // SAFETY: `info` is the mount's view, live until the timer is
        // invalidated; run-loop timers fire on the main thread.
        let Some(view) = Obj::from_ptr(info).map(|v| unsafe { View::new(v) }) else {
            return;
        };
        let dirty = view.with_ui(|shared, ui| ui.runtime.needs_paint(&*shared.controller, ui.time()));
        if dirty == Some(true) {
            view.redraw();
        }
    });
}

// ------------------------------------------------------------ the class

/// The face view class, registered once per binary.
struct FaceViewClass {
    class: ClassRef,
    /// Byte offset of the `face` ivar: `*const Shared`, or null.
    face: isize,
}

fn face_view_class() -> &'static FaceViewClass {
    static CLASS: OnceLock<FaceViewClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let class = define_class("StaveFaceView", c"NSView", |c| {
            c.ivar(c"face");
            c.method(sel!(c"acceptsFirstMouse:"), yes_for_event as extern "C" fn(Obj, Sel, Option<Obj>) -> ObjcBool);
            c.method(sel!(c"acceptsFirstResponder"), yes as extern "C" fn(Obj, Sel) -> ObjcBool);
            c.method(sel!(c"drawRect:"), draw_rect as extern "C" fn(Obj, Sel, CGRect));
            type OnEvent = extern "C" fn(Obj, Sel, Option<Obj>);
            c.method(sel!(c"mouseDown:"), mouse_down as OnEvent);
            c.method(sel!(c"mouseDragged:"), mouse_dragged as OnEvent);
            c.method(sel!(c"mouseUp:"), mouse_up as OnEvent);
            c.method(sel!(c"mouseMoved:"), mouse_moved as OnEvent);
            c.method(sel!(c"mouseExited:"), mouse_exited as OnEvent);
            c.method(sel!(c"scrollWheel:"), scroll_wheel as OnEvent);
            c.method(sel!(c"keyDown:"), key_down as OnEvent);
        });
        FaceViewClass {
            class: ClassRef(class),
            face: ivar_offset(class, c"face"),
        }
    })
}

/// The `face` ivar of a face view.
///
/// # Safety
/// `view` is a live instance of the face view class.
unsafe fn face_slot(view: Obj) -> *mut *const Shared {
    // SAFETY: the ivar lies inside the instance (this function's contract).
    unsafe { view.as_ptr().byte_offset(face_view_class().face).cast() }
}

/// The face view a method was called on: live for the call, on the main
/// thread.
#[derive(Copy, Clone)]
struct View(Obj, MainThread);

/// The NSEvent a method was called with, live for the call.
#[derive(Copy, Clone)]
struct Event(Obj);

impl View {
    /// # Safety
    /// `this` is a live instance of the face view class, used on the main
    /// thread while it is live (`self` of an AppKit callback is).
    unsafe fn new(this: Obj) -> Self {
        // SAFETY: this function's contract.
        View(this, unsafe { MainThread::assume() })
    }

    /// A message to the view.
    fn send<A: Args, R: Ret>(self, sel: Sel, args: A) -> R {
        // SAFETY: a live NSView on the main thread; every call below
        // names an NSView/NSResponder method with its real signature.
        unsafe { send(self.0, sel, args) }
    }

    /// The view's state, counted for the caller: `None` once detached.
    fn shared(self) -> Option<Rc<Shared>> {
        // SAFETY: a live face view (constructor contract).
        let p = unsafe { *face_slot(self.0) };
        if p.is_null() {
            return None;
        }
        // SAFETY: a non-null slot holds the mount's counted reference, so
        // the count is at least one while we add ours.
        unsafe {
            Rc::increment_strong_count(p);
            Some(Rc::from_raw(p))
        }
    }

    /// Run `f` on the face: `None` once detached, while another callback
    /// is running (a reentrant event is dropped), or after face code
    /// panicked.
    fn with_ui<R>(self, f: impl FnOnce(&Shared, &mut Ui) -> R) -> Option<R> {
        let shared = self.shared()?;
        if shared.broken.get() {
            return None;
        }
        let mut ui = shared.ui.try_borrow_mut().ok()?;
        let result = guard(None, || Some(f(&shared, &mut ui)));
        shared.broken.set(result.is_none());
        result
    }

    fn redraw(self) {
        self.send::<_, ()>(sel!(c"setNeedsDisplay:"), (true,));
    }

    fn window(self) -> Option<Obj> {
        self.send(sel!(c"window"), ())
    }

    fn backing_scale(self) -> f64 {
        let Some(window) = self.window() else {
            return 2.0;
        };
        // SAFETY: the view's live window, on the main thread.
        let s: f64 = unsafe { send(window, sel!(c"backingScaleFactor"), ()) };
        if s.is_finite() && s >= 1.0 { s } else { 2.0 }
    }

    /// Where `event` happened, in face coordinates (y down).
    fn point(self, event: Event) -> Pt {
        let p: CGPoint = self.send(sel!(c"convertPoint:fromView:"), (event.location(), None::<Obj>));
        // The view is y-up (so the canvas blits upright); faces are y-down.
        let bounds: CGRect = self.send(sel!(c"bounds"), ());
        pt(p.x, bounds.size.height - p.y)
    }

    fn focus(self) {
        if let Some(window) = self.window() {
            // SAFETY: the view's live window, on the main thread.
            unsafe { send::<_, bool>(window, sel!(c"makeFirstResponder:"), (self.0,)) };
        }
    }

    /// Resize the view (and its window's content) to `size` points.
    fn resize(self, (w, h): (f64, f64)) {
        let size = CGSize { width: w, height: h };
        self.send::<_, ()>(sel!(c"setFrameSize:"), (size,));
        if let Some(window) = self.window() {
            // SAFETY: the view's live window, on the main thread.
            unsafe { send::<_, ()>(window, sel!(c"setContentSize:"), (size,)) };
        }
    }
}

impl Event {
    /// # Safety
    /// `event` is null or the live NSEvent of the current callback.
    unsafe fn new(event: Option<Obj>) -> Option<Self> {
        event.map(Event)
    }

    fn send<R: Ret>(self, sel: Sel) -> R {
        // SAFETY: a live NSEvent (constructor contract); every call below
        // names an NSEvent property with its real type.
        unsafe { send(self.0, sel, ()) }
    }

    fn location(self) -> CGPoint {
        self.send(sel!(c"locationInWindow"))
    }

    fn mods(self) -> Mods {
        let flags: u64 = self.send(sel!(c"modifierFlags"));
        const SHIFT: u64 = 1 << 17;
        const OPTION: u64 = 1 << 19;
        const COMMAND: u64 = 1 << 20;
        Mods {
            fine: flags & (SHIFT | COMMAND) != 0,
            reset: flags & OPTION != 0,
        }
    }

    fn clicks(self) -> u32 {
        let n: i64 = self.send(sel!(c"clickCount"));
        n.clamp(1, i64::from(u32::MAX)) as u32
    }

    fn key_code(self) -> u16 {
        self.send(sel!(c"keyCode"))
    }

    /// Wheel travel in lines (positive = up).
    fn scroll_lines(self) -> f64 {
        let dy: f64 = self.send(sel!(c"scrollingDeltaY"));
        let precise: bool = self.send(sel!(c"hasPreciseScrollingDeltas"));
        // Trackpads report points; a wheel notch is about ten of them.
        if precise { dy / 10.0 } else { dy }
    }
}

fn set_cursor(_main: MainThread, c: Cursor) {
    let shape = match c {
        Cursor::Arrow => sel!(c"arrowCursor"),
        Cursor::Grab => sel!(c"openHandCursor"),
        Cursor::Grabbing => sel!(c"closedHandCursor"),
        Cursor::Point => sel!(c"pointingHandCursor"),
    };
    // SAFETY: NSCursor's class properties return a shared cursor, and
    // `set` takes nothing; on the main thread (the token).
    unsafe {
        let cursor: Option<Obj> = send(class(c"NSCursor"), shape, ());
        if let Some(cursor) = cursor {
            send::<_, ()>(cursor, sel!(c"set"), ());
        }
    }
}

/// Track the pointer anywhere over the view, active even when the host
/// window is not key (plugin windows often are not).
///
/// # Safety
/// `view` is a live NSView, on the main thread.
unsafe fn track_pointer(view: Obj) {
    const MOUSE_ENTERED_EXITED: u64 = 0x01;
    const MOUSE_MOVED: u64 = 0x02;
    const ACTIVE_ALWAYS: u64 = 0x80;
    const IN_VISIBLE_RECT: u64 = 0x200;
    let options = MOUSE_ENTERED_EXITED | MOUSE_MOVED | ACTIVE_ALWAYS | IN_VISIBLE_RECT;
    // SAFETY: `alloc` + `initWithRect:options:owner:userInfo:` with their
    // real types; `addTrackingArea:` retains the area, so ours is
    // released when `area` drops.
    unsafe {
        let raw: Option<Obj> = send(class(c"NSTrackingArea"), sel!(c"alloc"), ());
        let area: Option<Obj> = send(
            raw.expect("alloc"),
            sel!(c"initWithRect:options:owner:userInfo:"),
            (CGRect::new(0.0, 0.0, 0.0, 0.0), options, view, None::<Obj>),
        );
        let area = Retained::from_owned(area.expect("a tracking area initializes"));
        send::<_, ()>(view, sel!(c"addTrackingArea:"), (area.get(),));
    }
}

// ------------------------------------------------------- the methods

/// The body of a view method: the view and its event, inside the panic
/// boundary.
fn method(this: Obj, event: Option<Obj>, body: impl FnOnce(View, Event)) {
    guard((), || {
        // SAFETY: AppKit calls a view's methods on the main thread, with
        // the live view as `self` and the live event of the call.
        let (view, event) = unsafe { (View::new(this), Event::new(event)) };
        if let Some(event) = event {
            body(view, event);
        }
    });
}

extern "C" fn yes(_this: Obj, _cmd: Sel) -> ObjcBool {
    YES
}

extern "C" fn yes_for_event(_this: Obj, _cmd: Sel, _event: Option<Obj>) -> ObjcBool {
    YES
}

extern "C" fn draw_rect(this: Obj, _cmd: Sel, _dirty: CGRect) {
    guard((), || {
        // SAFETY: as in `method`.
        let view = unsafe { View::new(this) };
        // SAFETY: NSGraphicsContext's class property and its `CGContext`,
        // with their real types, during the view's draw on the main thread.
        let ctx: *mut c_void = unsafe {
            let Some(current) = send::<_, Option<Obj>>(class(c"NSGraphicsContext"), sel!(c"currentContext"), ()) else {
                return;
            };
            send(current, sel!(c"CGContext"), ())
        };
        if ctx.is_null() {
            return;
        }
        let scale = view.backing_scale();
        view.with_ui(|shared, ui| {
            let t = ui.time();
            let (w, h) = ui.runtime.size();
            let mut text = crate::offscreen::CoreText::new(scale);
            ui.runtime.render(&*shared.controller, scale, t, &mut ui.canvas, &mut text);
            // SAFETY: the current drawing pass's live context.
            unsafe { crate::offscreen::draw_canvas(ctx, &ui.canvas, w, h) };
        });
    });
}

extern "C" fn mouse_down(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let (p, mods, clicks) = (view.point(event), event.mods(), event.clicks());
        view.focus();
        view.with_ui(|shared, ui| {
            ui.runtime.pointer_down(&*shared.controller, p, mods, clicks);
            set_cursor(view.1, ui.runtime.cursor());
        });
        view.redraw();
    });
}

extern "C" fn mouse_dragged(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let (p, mods) = (view.point(event), event.mods());
        view.with_ui(|shared, ui| ui.runtime.pointer_drag(&*shared.controller, p, mods));
        view.redraw();
    });
}

extern "C" fn mouse_up(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let p = view.point(event);
        view.with_ui(|shared, ui| {
            ui.runtime.pointer_up(&*shared.controller);
            ui.runtime.pointer_move(&*shared.controller, p);
            set_cursor(view.1, ui.runtime.cursor());
        });
        view.redraw();
    });
}

extern "C" fn mouse_moved(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let p = view.point(event);
        let changed = view.with_ui(|shared, ui| {
            let changed = ui.runtime.pointer_move(&*shared.controller, p);
            set_cursor(view.1, ui.runtime.cursor());
            changed
        });
        if changed == Some(true) {
            view.redraw();
        }
    });
}

extern "C" fn mouse_exited(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, _| {
        view.with_ui(|_, ui| ui.runtime.pointer_exit());
        set_cursor(view.1, Cursor::Arrow);
        view.redraw();
    });
}

extern "C" fn scroll_wheel(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let (p, mods, lines) = (view.point(event), event.mods(), event.scroll_lines());
        if lines == 0.0 {
            return;
        }
        view.with_ui(|shared, ui| ui.runtime.wheel(&*shared.controller, p, lines, mods));
        view.redraw();
    });
}

extern "C" fn key_down(this: Obj, _cmd: Sel, event: Option<Obj>) {
    method(this, event, |view, event| {
        let code = event.key_code();
        // `None`: no key handler, so the key belongs to the host.
        let handled = view.with_ui(|_, ui| {
            let on_key = ui.on_key.as_mut()?;
            let face = on_key(code);
            if let Some(face) = face {
                ui.runtime = Runtime::new(face);
                ui.opened = Instant::now();
            }
            Some(ui.runtime.size())
        });
        match handled {
            Some(Some(size)) => {
                view.resize(size);
                view.redraw();
            }
            _ => {
                let next: Option<Obj> = view.send(sel!(c"nextResponder"), ());
                if let Some(next) = next {
                    // SAFETY: an NSResponder's `keyDown:` with the event
                    // AppKit gave us, on the main thread.
                    unsafe { send::<_, ()>(next, sel!(c"keyDown:"), (event.0,)) };
                }
            }
        }
    });
}
