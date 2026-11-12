//! The live storybook: one window showing the showcase face, driven by
//! the stand-in host. Everything drags; ←/→ step through the themes;
//! Esc or ⌘Q quits.

// Objective-C runtime calls for a minimal AppKit app.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void, CString};

use stave_face::Host;
use stave_ui_cocoa::face::{FaceMount, FaceState, FaceTarget};

use crate::host::StoryHost;
use crate::stories;

type Id = *mut c_void;
type Sel = *mut c_void;

#[link(name = "objc")]
#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn class(name: &str) -> Id {
    let c = CString::new(name).expect("class name");
    unsafe { objc_getClass(c.as_ptr()) }
}

fn sel(name: &str) -> Sel {
    let c = CString::new(name).expect("selector");
    unsafe { sel_registerName(c.as_ptr()) }
}

unsafe fn send(obj: Id, s: &str) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, sel(s)) }
}

unsafe fn send1(obj: Id, s: &str, a: Id) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, Id) -> Id = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, sel(s), a) }
}

unsafe fn ns_string(s: &str) -> Id {
    let c = CString::new(s).expect("string");
    let f: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(class("NSString"), sel("stringWithUTF8String:"), c.as_ptr()) }
}

/// The story behind the window: the stand-in host and the current theme.
struct Story {
    host: StoryHost,
    count: usize,
    at: usize,
    state: FaceState,
    window: Id,
}

impl Story {
    fn load(&mut self) {
        let (name, face) = stories::all(&self.host).swap_remove(self.at);
        self.state = FaceState::new(face);
        if !self.window.is_null() {
            unsafe {
                send1(self.window, "setTitle:", ns_string(&format!("stave storybook — {name}  (←/→ story)")));
            }
        }
    }
}

impl FaceTarget for Story {
    fn with(&mut self, f: &mut dyn FnMut(&mut FaceState, &mut dyn Host)) {
        self.host.time.set(self.state.opened.elapsed().as_secs_f64());
        f(&mut self.state, &mut self.host);
    }
    fn key(&mut self, code: u16) -> bool {
        let n = self.count;
        match code {
            53 => std::process::exit(0),
            123 => self.at = (self.at + n - 1) % n,
            124 => self.at = (self.at + 1) % n,
            _ => return false,
        }
        self.load();
        true
    }
}

pub fn run() {
    unsafe {
        let app = send(class("NSApplication"), "sharedApplication");
        let policy: unsafe extern "C" fn(Id, Sel, i64) -> bool = std::mem::transmute(objc_msgSend as *const ());
        policy(app, sel("setActivationPolicy:"), 0);

        // A menu bar with Quit (⌘Q).
        let bar = send(send(class("NSMenu"), "alloc"), "init");
        let app_item = send(send(class("NSMenuItem"), "alloc"), "init");
        send1(bar, "addItem:", app_item);
        let app_menu = send(send(class("NSMenu"), "alloc"), "init");
        let item: unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id = std::mem::transmute(objc_msgSend as *const ());
        let quit = item(
            send(class("NSMenuItem"), "alloc"),
            sel("initWithTitle:action:keyEquivalent:"),
            ns_string("Quit stave storybook"),
            sel("terminate:"),
            ns_string("q"),
        );
        send1(app_menu, "addItem:", quit);
        send1(app_item, "setSubmenu:", app_menu);
        send1(app, "setMainMenu:", bar);

        let host = StoryHost::new();
        let count = stories::all(&host).len();
        let state = FaceState::new(stories::all(&host).swap_remove(0).1);
        let (w, h) = state.runtime.size();
        let mut story = Box::new(Story {
            host,
            count,
            at: 0,
            state,
            window: std::ptr::null_mut(),
        });

        let init: unsafe extern "C" fn(Id, Sel, Rect, u64, u64, bool) -> Id = std::mem::transmute(objc_msgSend as *const ());
        let window = init(
            send(class("NSWindow"), "alloc"),
            sel("initWithContentRect:styleMask:backing:defer:"),
            Rect { x: 0.0, y: 0.0, w, h },
            1 | 2 | 4, // titled, closable, miniaturizable
            2,         // buffered
            false,
        );
        story.window = window;
        story.load();
        send(window, "center");
        let content = send(window, "contentView");
        let mount = FaceMount::new(content, story, (w, h));
        send1(window, "makeFirstResponder:", mount.view());
        send1(window, "makeKeyAndOrderFront:", std::ptr::null_mut());
        let activate: unsafe extern "C" fn(Id, Sel, bool) = std::mem::transmute(objc_msgSend as *const ());
        activate(app, sel("activateIgnoringOtherApps:"), true);
        send(app, "run");
        drop(mount);
    }
}
