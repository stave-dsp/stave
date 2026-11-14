//! A stand-alone app around one [`FaceView`]: a regular application with
//! a Quit menu (⌘Q) and one window the size of the face. The storybook
//! runs its faces here.

use std::ffi::CString;

use crate::cf::CGRect;
use crate::objc::{class, send, sel, MainThread, Obj, Retained};
use crate::view::FaceView;

/// Show `view` in a window titled `title` and run the app until it quits.
///
/// # Panics
/// Off the main thread.
pub fn run_app(title: &str, mut view: FaceView) {
    let _main = MainThread::new().expect("the app runs on the main thread");
    let (w, h) = view.size();
    // SAFETY: every send below names an AppKit or Foundation method with
    // its real signature, on the main thread; objects made with `alloc` +
    // `init…` are `Retained` (their owners retain what they keep).
    unsafe {
        let app: Option<Obj> = send(class(c"NSApplication"), sel!(c"sharedApplication"), ());
        let app = app.expect("the shared application");
        // NSApplicationActivationPolicyRegular: a Dock icon and a menu bar.
        send::<_, bool>(app, sel!(c"setActivationPolicy:"), (0i64,));

        let bar = new(c"NSMenu");
        let app_item = new(c"NSMenuItem");
        send::<_, ()>(bar.get(), sel!(c"addItem:"), (app_item.get(),));
        let app_menu = new(c"NSMenu");
        let raw: Option<Obj> = send(class(c"NSMenuItem"), sel!(c"alloc"), ());
        let quit: Option<Obj> = send(
            raw.expect("alloc"),
            sel!(c"initWithTitle:action:keyEquivalent:"),
            (ns_string("Quit"), sel!(c"terminate:"), ns_string("q")),
        );
        let quit = Retained::from_owned(quit.expect("a menu item"));
        send::<_, ()>(app_menu.get(), sel!(c"addItem:"), (quit.get(),));
        send::<_, ()>(app_item.get(), sel!(c"setSubmenu:"), (app_menu.get(),));
        send::<_, ()>(app, sel!(c"setMainMenu:"), (bar.get(),));

        const TITLED_CLOSABLE_MINIATURIZABLE: u64 = 1 | 2 | 4;
        const BUFFERED: u64 = 2;
        let raw: Option<Obj> = send(class(c"NSWindow"), sel!(c"alloc"), ());
        let window: Option<Obj> = send(
            raw.expect("alloc"),
            sel!(c"initWithContentRect:styleMask:backing:defer:"),
            (CGRect::new(0.0, 0.0, w, h), TITLED_CLOSABLE_MINIATURIZABLE, BUFFERED, false),
        );
        let window = Retained::from_owned(window.expect("a window"));
        // Ours to release (below), not the close button's.
        send::<_, ()>(window.get(), sel!(c"setReleasedWhenClosed:"), (false,));
        send::<_, ()>(window.get(), sel!(c"setTitle:"), (ns_string(title),));
        send::<_, ()>(window.get(), sel!(c"center"), ());
        let content: Option<Obj> = send(window.get(), sel!(c"contentView"), ());
        // A stand-alone app has no host to refuse to: a window that cannot
        // be shown is the end of it.
        view.attach(content.expect("a content view").as_ptr()).expect("the face attaches");
        if let Some(face) = view.ns_view() {
            send::<_, bool>(window.get(), sel!(c"makeFirstResponder:"), (face,));
        }
        send::<_, ()>(window.get(), sel!(c"makeKeyAndOrderFront:"), (None::<Obj>,));
        send::<_, ()>(app, sel!(c"activateIgnoringOtherApps:"), (true,));
        send::<_, ()>(app, sel!(c"run"), ());
        drop(view);
    }
}

/// `[[class alloc] init]`.
///
/// # Safety
/// Main thread; `class` is an AppKit class whose `init` takes nothing.
unsafe fn new(name: &core::ffi::CStr) -> Retained {
    // SAFETY: this function's contract.
    unsafe {
        let raw: Option<Obj> = send(class(name), sel!(c"alloc"), ());
        let obj: Option<Obj> = send(raw.expect("alloc"), sel!(c"init"), ());
        Retained::from_owned(obj.expect("init"))
    }
}

/// An autoreleased NSString with `s`'s text (a handful made before the
/// app's run loop has a pool simply stay).
///
/// # Safety
/// Main thread.
unsafe fn ns_string(s: &str) -> Obj {
    let c = CString::new(s.replace('\0', "")).expect("no nul left");
    // SAFETY: `stringWithUTF8String:` copies the nul-terminated bytes.
    let string: Option<Obj> = unsafe { send(class(c"NSString"), sel!(c"stringWithUTF8String:"), (c.as_ptr(),)) };
    string.expect("an NSString")
}
