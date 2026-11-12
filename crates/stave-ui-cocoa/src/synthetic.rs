//! Real AppKit events into a real (offscreen) window, for tests (ledger
//! U8): the pointer and wheel reach a [`FaceView`] exactly as AppKit
//! delivers them — `NSEvent`s in window coordinates (y up), with their
//! modifier flags, click counts and scroll units — so the view's own
//! mapping (the y-flip, the units, the modifier bits) is what is tested.
//! The window is never ordered on screen.
//!
//! AppKit is main-thread only: drive this from a `harness = false` test
//! (stave-ui-cocoa's `tests/real_window.rs`).

use core::ffi::c_void;

use stave_face::Mods;

use crate::cf::{CGPoint, CGRect, CfRef};
use crate::objc::{class, send, sel, MainThread, Obj, Retained};
use crate::view::FaceView;

/// A mouse event kind (`NSEventType`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mouse {
    Down,
    Up,
    Dragged,
    Moved,
}

impl Mouse {
    fn event_type(self) -> u64 {
        match self {
            Mouse::Down => 1,
            Mouse::Up => 2,
            Mouse::Moved => 5,
            Mouse::Dragged => 6,
        }
    }
    fn method(self) -> crate::objc::Sel {
        match self {
            Mouse::Down => sel!(c"mouseDown:"),
            Mouse::Up => sel!(c"mouseUp:"),
            Mouse::Moved => sel!(c"mouseMoved:"),
            Mouse::Dragged => sel!(c"mouseDragged:"),
        }
    }
}

/// NSEvent modifier flags for `mods`: Shift for fine, Option for reset.
fn flags(mods: Mods) -> u64 {
    const SHIFT: u64 = 1 << 17;
    const OPTION: u64 = 1 << 19;
    (if mods.fine { SHIFT } else { 0 }) | (if mods.reset { OPTION } else { 0 })
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateScrollWheelEvent(source: *const c_void, units: u32, wheel_count: u32, wheel1: i32, ...) -> *mut c_void;
    fn CGEventSetLocation(event: *mut c_void, at: CGPoint);
}

/// A borderless window that is never shown, holding one face view.
pub struct OffscreenWindow {
    window: Retained,
    view: FaceView,
    _main: MainThread,
}

impl OffscreenWindow {
    /// `view` in a window its size, at the screen's origin.
    ///
    /// # Panics
    /// Off the main thread.
    pub fn new(mut view: FaceView) -> Self {
        let main = MainThread::new().expect("AppKit runs on the main thread");
        let (w, h) = view.size();
        // SAFETY: NSApplication / NSWindow messages with their real
        // signatures, on the main thread; the window is `Retained`.
        let window = unsafe {
            let _app: Option<Obj> = send(class(c"NSApplication"), sel!(c"sharedApplication"), ());
            const BORDERLESS: u64 = 0;
            const BUFFERED: u64 = 2;
            let raw: Option<Obj> = send(class(c"NSWindow"), sel!(c"alloc"), ());
            let window: Option<Obj> = send(
                raw.expect("alloc"),
                sel!(c"initWithContentRect:styleMask:backing:defer:"),
                (CGRect::new(0.0, 0.0, w, h), BORDERLESS, BUFFERED, false),
            );
            let window = Retained::from_owned(window.expect("a window"));
            send::<_, ()>(window.get(), sel!(c"setReleasedWhenClosed:"), (false,));
            let content: Option<Obj> = send(window.get(), sel!(c"contentView"), ());
            view.attach(content.expect("a content view").as_ptr());
            window
        };
        OffscreenWindow { window, view, _main: main }
    }

    /// The view (its controller holds the values the events moved).
    pub fn view(&self) -> &FaceView {
        &self.view
    }

    fn ns_view(&self) -> Obj {
        self.view.ns_view().expect("attached")
    }

    /// Send a mouse event at `at` in WINDOW coordinates (points, origin
    /// bottom-left, y up — what AppKit hands a view).
    pub fn mouse(&self, kind: Mouse, at: (f64, f64), mods: Mods, clicks: i64) {
        // SAFETY: the NSEvent factory with its real nine-argument
        // signature, then the view's own handler with the event, on the
        // main thread; the window and view are live.
        unsafe {
            let number: i64 = send(self.window.get(), sel!(c"windowNumber"), ());
            let event: Option<Obj> = send(
                class(c"NSEvent"),
                sel!(c"mouseEventWithType:location:modifierFlags:timestamp:windowNumber:context:eventNumber:clickCount:pressure:"),
                (kind.event_type(), CGPoint { x: at.0, y: at.1 }, flags(mods), 0.0f64, number, None::<Obj>, 0i64, clicks, 1.0f32),
            );
            send::<_, ()>(self.ns_view(), kind.method(), (event.expect("an event"),));
        }
    }

    /// Scroll `lines` wheel notches (a mouse wheel, not a trackpad) with
    /// the pointer at `at` in window coordinates.
    pub fn wheel(&self, at: (f64, f64), lines: i32) {
        // SAFETY: CoreGraphics makes a line-unit scroll event (released
        // when `cg` drops, after NSEvent took its own reference); its location is
        // the window point in global display coordinates (the window sits
        // at the screen origin, so only the y axis flips); then the
        // view's handler, on the main thread.
        unsafe {
            let screen: CGRect = send(self.window.get(), sel!(c"convertRectToScreen:"), (CGRect::new(at.0, at.1, 0.0, 0.0),));
            let screens: Option<Obj> = send(class(c"NSScreen"), sel!(c"screens"), ());
            let primary: Option<Obj> = send(screens.expect("screens"), sel!(c"firstObject"), ());
            let display: CGRect = primary.map_or(CGRect::new(0.0, 0.0, 0.0, 0.0), |p| send(p, sel!(c"frame"), ()));
            let cg = CfRef::from_create(CGEventCreateScrollWheelEvent(core::ptr::null(), 1, 1, lines)).expect("a scroll event");
            CGEventSetLocation(cg.as_ptr(), CGPoint { x: screen.origin.x, y: display.size.height - screen.origin.y });
            let event: Option<Obj> = send(class(c"NSEvent"), sel!(c"eventWithCGEvent:"), (Obj::from_ptr(cg.as_ptr()).expect("non-null"),));
            drop(cg);
            send::<_, ()>(self.ns_view(), sel!(c"scrollWheel:"), (event.expect("an event"),));
        }
    }
}

impl Drop for OffscreenWindow {
    fn drop(&mut self) {
        self.view.detach();
        // SAFETY: our window, on the main thread.
        unsafe { send::<_, ()>(self.window.get(), sel!(c"close"), ()) };
    }
}
