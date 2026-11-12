//! stave-storybook — the design system's NATIVE storybook: every
//! component, live and draggable, with no plugin behind it. The
//! editor stack proves its own layering here: a demo param table, a
//! story EditorHost that writes gestures back to it, a driver thread
//! animating the telemetry probes on a bare DebugBus, and the
//! REAL PluginEditorView mounts the stories tree. Navigation is the
//! `ViewTabs` — the design system browses itself.
//!
//! Usage:
//!   stave-storybook                       # live, interactive
//!   stave-storybook --screenshot DIR      # one PNG per story page
//!
//! The screenshot mode is the agent's eyes on the design system —
//! every page captured, no human at the keyboard.

// Unsafe here is Objective-C runtime FFI, documented with the UI overhaul
// (PLAN S6), not block by block before it.
#![allow(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc)]

mod params;
mod stories;

use std::ffi::{c_char, c_void};
use std::sync::atomic::AtomicU16;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stave_core::params::Params;
use stave_core::ui::tokens;
use stave_core::DebugBus;
use stave_ui_cocoa::{EditorHost, PluginEditorView};

// ------------------------------------------------------------------- objc
type Id = *mut c_void;
type Sel = *const c_void;

#[expect(
    clippy::duplicated_attributes,
    reason = "one #[link] per framework repeats kind"
)]
#[link(name = "objc")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "Foundation", kind = "framework")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
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

unsafe fn cls(name: &core::ffi::CStr) -> Id {
    unsafe { objc_getClass(name.as_ptr()) }
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
unsafe fn msg_i64(obj: Id, s: Sel) -> i64 {
    let f: unsafe extern "C" fn(Id, Sel) -> i64 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(obj, s) }
}
unsafe fn ns_string(s: &str) -> Id {
    let c = std::ffi::CString::new(s).unwrap();
    let f: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(cls(c"NSString"), sel(c"stringWithUTF8String:"), c.as_ptr()) }
}

// -------------------------------------------------------------- the host
/// Gestures write straight back to the demo table (the storybook IS
/// the host), which the editor reads, so a drag is live end to end.
struct StoryHost {
    table: Arc<params::StoryParams>,
}
impl EditorHost for StoryHost {
    fn begin_edit(&self, _id: u32) {}
    fn perform_edit(&self, id: u32, plain: f64) {
        if let Some(p) = self.table.by_id(stave_core::ParamId(id)) {
            p.set_plain(plain);
        }
    }
    fn end_edit(&self, _id: u32) {}
}

// ---------------------------------------------------------- the driver
/// Animates the TELEMETRY page: a breathing meter, a wandering
/// envelope, a sweeping wave cursor. Registered before mount so the
/// widgets resolve their probes.
fn drive_probes(bus: Arc<DebugBus>) {
    static LEVEL: AtomicU16 = AtomicU16::new(u16::MAX);
    static INPUT_LEFT_LEVEL: AtomicU16 = AtomicU16::new(u16::MAX);
    static INPUT_RIGHT_LEVEL: AtomicU16 = AtomicU16::new(u16::MAX);
    static OUTPUT_LEFT_LEVEL: AtomicU16 = AtomicU16::new(u16::MAX);
    static OUTPUT_RIGHT_LEVEL: AtomicU16 = AtomicU16::new(u16::MAX);
    static PHASE: AtomicU16 = AtomicU16::new(u16::MAX);
    static ENV: AtomicU16 = AtomicU16::new(u16::MAX);
    let mut t = 0.0f64;
    loop {
        t += 1.0 / 30.0;
        let input_left = -38.0 + (t * 1.3).sin().abs() * 34.0;
        let input_right = -40.0 + (t * 1.3 + 0.15).sin().abs() * 35.0;
        let output_left = -34.0 + (t * 1.3 + 0.22).sin().abs() * 29.0;
        let output_right = -36.0 + (t * 1.3 + 0.36).sin().abs() * 30.0;
        bus.probe_f64(&LEVEL, "level", input_left);
        bus.probe_f64(&INPUT_LEFT_LEVEL, "io_in_l", input_left);
        bus.probe_f64(&INPUT_RIGHT_LEVEL, "io_in_r", input_right);
        bus.probe_f64(&OUTPUT_LEFT_LEVEL, "io_out_l", output_left);
        bus.probe_f64(&OUTPUT_RIGHT_LEVEL, "io_out_r", output_right);
        bus.probe_f64(&PHASE, "phase", (t * 0.4).fract());
        bus.probe_f64(&ENV, "env", ((t * 0.7).sin() * 0.5 + 0.5).powi(2));
        std::thread::sleep(Duration::from_millis(33));
    }
}

fn pump(app: Id, ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    unsafe {
        let mode = ns_string("kCFRunLoopDefaultMode");
        let next: unsafe extern "C" fn(Id, Sel, u64, Id, Id, bool) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let date_f: unsafe extern "C" fn(Id, Sel, f64) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        while Instant::now() < deadline {
            let date = date_f(cls(c"NSDate"), sel(c"dateWithTimeIntervalSinceNow:"), 0.02);
            let ev = next(
                app,
                sel(c"nextEventMatchingMask:untilDate:inMode:dequeue:"),
                u64::MAX,
                date,
                mode,
                true,
            );
            if !ev.is_null() {
                msg_void_id(app, sel(c"sendEvent:"), ev);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shot_dir = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // The plugin-less stack: table, probes on a bare bus.
    let table = params::StoryParams::new();
    let bus = DebugBus::new();
    {
        // Register the probes (and one wave cycle) before mount.
        static BOOT: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&BOOT, "level", 0.0);
        static INPUT_LEFT_BOOT: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&INPUT_LEFT_BOOT, "io_in_l", -18.0);
        static INPUT_RIGHT_BOOT: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&INPUT_RIGHT_BOOT, "io_in_r", -20.0);
        static OUTPUT_LEFT_BOOT: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&OUTPUT_LEFT_BOOT, "io_out_l", -16.0);
        static OUTPUT_RIGHT_BOOT: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&OUTPUT_RIGHT_BOOT, "io_out_r", -18.0);
        static B2: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&B2, "phase", 0.0);
        static B3: AtomicU16 = AtomicU16::new(u16::MAX);
        bus.probe_f64(&B3, "env", 0.0);
        let cycle: Vec<f32> = (0..256)
            .map(|i| (core::f32::consts::TAU * i as f32 / 256.0).sin())
            .collect();
        bus.publish_curve(&cycle);
    }
    {
        let bus = bus.clone();
        std::thread::spawn(move || drive_probes(bus));
    }

    let mut view = PluginEditorView::new(
        bus.clone(),
        table.clone(),
        Box::new(StoryHost {
            table: table.clone(),
        }),
    );
    view.set_ui(Some(stories::tree(&table)));

    // ------- the window -------
    unsafe {
        let app = msg_id(cls(c"NSApplication"), sel(c"sharedApplication"));
        msg_void_int(app, sel(c"setActivationPolicy:"), 0);
        let (w, h) = (tokens::EDITOR_CANVAS_WIDTH, tokens::EDITOR_CANVAS_HEIGHT);
        let frame = CGRect {
            origin: CGPoint { x: 160.0, y: 140.0 },
            size: CGSize { w, h },
        };
        let init: unsafe extern "C" fn(Id, Sel, CGRect, u64, u64, bool) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let window = init(
            msg_id(cls(c"NSWindow"), sel(c"alloc")),
            sel(c"initWithContentRect:styleMask:backing:defer:"),
            frame,
            15,
            2,
            false,
        );
        msg_void_id(window, sel(c"setTitle:"), ns_string("stave · storybook"));
        msg_void_id(window, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
        msg_void_id(
            app,
            sel(c"activateIgnoringOtherApps:"),
            std::ptr::null_mut(),
        );
        let content = msg_id(window, sel(c"contentView"));
        view.attach(content);

        if let Some(dir) = shot_dir {
            std::fs::create_dir_all(&dir).ok();
            pump(app, 900); // first layout + tick settle
            let win_num = msg_i64(window, sel(c"windowNumber"));
            for (i, name) in stories::NAV_PAGES.iter().enumerate() {
                view.set_view_page(stories::NAV.0, i as i64);
                pump(app, 400);
                let out = format!("{dir}/story_{}.png", name.to_lowercase());
                let ok = std::process::Command::new("screencapture")
                    .args(["-x", "-o", "-l", &win_num.to_string(), &out])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                println!("{}: {out}", if ok { "captured" } else { "FAILED" });
            }
            return;
        }

        println!("storybook up — drag things. ^C to quit.");
        let run: unsafe extern "C" fn(Id, Sel) = std::mem::transmute(objc_msgSend as *const ());
        run(app, sel(c"run"));
    }
}
