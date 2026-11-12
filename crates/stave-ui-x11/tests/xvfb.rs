//! The face in a real X11 window on a real X server (Xvfb in CI),
//! driven through the server's own input path (XTEST): the view's
//! mapping from window pixels to face points, its scale, its wheel
//! buttons, modifier bits and click counting are tested, not assumed —
//! and the window's pixels, read back, are the face's.
//!
//! Without `DISPLAY` the tests pass vacuously, unless
//! `STAVE_REQUIRE_DISPLAY` is set (CI sets it, so a missing server
//! fails). XTEST moves the one pointer, so the tests take a lock (and
//! nextest runs this binary one test at a time: `.config/nextest.toml`).

#![cfg(unix)]

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use stave_core::params::{AnyParam, BoolParam, FloatParam, ParamId, Params, Range};
use stave_core::{GestureSink, PluginController, Telemetry};
use stave_face::prelude::*;
use stave_face::Runtime;
use stave_ui_x11::pixels::Format;
use stave_ui_x11::FaceView;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, ImageFormat, Window, WindowClass};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct Table {
    knob: FloatParam,
    foot: BoolParam,
}

impl Params for Table {
    fn count(&self) -> usize {
        2
    }
    fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
        [&self.knob as &dyn AnyParam, &self.foot].get(i).copied()
    }
}

struct Nowhere;
impl GestureSink for Nowhere {
    fn begin(&self, _: ParamId) {}
    fn perform(&self, _: ParamId, _: f64) {}
    fn end(&self, _: ParamId) {}
}

fn table() -> Arc<Table> {
    Arc::new(Table {
        knob: FloatParam::new(ParamId(1), "Knob", Range::linear(0.0, 1.0)).default(0.5),
        foot: BoolParam::new(ParamId(2), "Foot", false),
    })
}

/// A knob at the top, a footswitch below it.
fn face(t: &Table) -> Face {
    Face::new(Theme::graphite(), column(vec![Knob::new(t.knob.bind()).node(), Footswitch::new(t.foot.bind()).node()]).gap(24.0))
}

fn controller(t: &Arc<Table>) -> PluginController {
    PluginController::new(t.clone(), Telemetry::new(), Vec::new().into(), Box::new(Nowhere))
}

static POINTER: Mutex<()> = Mutex::new(());

// XTEST event types.
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const MOTION: u8 = 6;

struct Rig {
    _lock: MutexGuard<'static, ()>,
    conn: RustConnection,
    root: Window,
    parent: Window,
    view: FaceView,
    scale: f64,
    /// The parent's origin on the root window, in pixels: fake input is
    /// in root coordinates (and away from the screen's edges, where the
    /// server clamps the pointer).
    origin: (i16, i16),
    knob: (f64, f64),
    foot: (f64, f64),
}

impl Rig {
    fn new(scale: f64) -> Option<Rig> {
        if std::env::var_os("DISPLAY").is_none() {
            assert!(std::env::var_os("STAVE_REQUIRE_DISPLAY").is_none(), "STAVE_REQUIRE_DISPLAY is set but DISPLAY is not");
            eprintln!("no DISPLAY: the X11 window tests pass vacuously");
            return None;
        }
        let lock = POINTER.lock().unwrap_or_else(|e| e.into_inner());
        let (conn, screen) = x11rb::connect(None).expect("the X display");
        let root = conn.setup().roots[screen].root;
        let t = table();
        // Where the parts are, from the headless runtime (face points).
        let rt = Runtime::new(face(&t));
        let probe = controller(&t);
        let at = |id| {
            let c = rt.grabs(&probe, PartId::Param(ParamId(id)))[0].area.centre();
            (c.x, c.y)
        };
        let (knob, foot) = (at(1), at(2));
        let mut view = FaceView::new(face(&t), Box::new(controller(&t)));
        view.set_scale(scale);
        let (w, h) = view.host_size();
        let parent = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            parent,
            root,
            200,
            300,
            w as u16,
            h as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap();
        conn.map_window(parent).unwrap();
        conn.sync().unwrap();
        view.attach(parent).expect("the view attaches");
        let at = conn.translate_coordinates(parent, root, 0, 0).unwrap().reply().unwrap();
        let origin = (at.dst_x, at.dst_y);
        let mut rig = Rig { _lock: lock, conn, root, parent, view, scale, origin, knob, foot };
        // Mapped and painted (the first Expose arrives).
        rig.settle();
        Some(rig)
    }

    /// Pump the view until the server has delivered everything sent so
    /// far (a round trip on our connection, then the view's queue).
    fn settle(&mut self) {
        self.conn.sync().unwrap();
        let until = Instant::now() + Duration::from_millis(150);
        while Instant::now() < until {
            assert!(self.view.pump(), "the view's connection is alive");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn fake(&self, kind: u8, detail: u8, (x, y): (f64, f64)) {
        let (x, y) = ((x * self.scale).round() as i16 + self.origin.0, (y * self.scale).round() as i16 + self.origin.1);
        self.conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0).unwrap();
    }

    fn move_to(&mut self, p: (f64, f64)) {
        self.fake(MOTION, 0, p);
        self.settle();
    }

    fn button(&mut self, kind: u8, button: u8, p: (f64, f64)) {
        self.fake(MOTION, 0, p);
        self.fake(kind, button, p);
        self.settle();
    }

    fn click(&mut self, p: (f64, f64)) {
        self.button(BUTTON_PRESS, 1, p);
        self.button(BUTTON_RELEASE, 1, p);
    }

    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        self.button(BUTTON_PRESS, 1, from);
        self.move_to(to);
        self.button(BUTTON_RELEASE, 1, to);
    }

    /// The keycode of `keysym`.
    fn keycode(&self, keysym: u32) -> u8 {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self.conn.get_keyboard_mapping(min, max - min + 1).unwrap().reply().unwrap();
        let per = usize::from(map.keysyms_per_keycode);
        let i = map.keysyms.iter().position(|&k| k == keysym).expect("the keysym is mapped");
        min + (i / per) as u8
    }

    fn key(&mut self, kind: u8, keysym: u32) {
        let code = self.keycode(keysym);
        self.fake(kind, code, (0.0, 0.0));
        self.conn.sync().unwrap();
    }

    fn value(&self, id: u32) -> f64 {
        self.view.controller().normalized(ParamId(id))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.view.detach();
        let _ = self.conn.destroy_window(self.parent);
        let _ = self.conn.sync();
    }
}

const SHIFT_L: u32 = 0xffe1;
const ALT_L: u32 = 0xffe9;

#[test]
fn a_click_lands_on_the_part_under_it() {
    let Some(mut r) = Rig::new(1.0) else { return };
    r.click(r.foot);
    assert_eq!(r.value(2), 1.0, "the footswitch");
    assert_eq!(r.value(1), 0.5, "the knob is untouched");
}

#[test]
fn dragging_up_raises_the_knob() {
    let Some(mut r) = Rig::new(1.0) else { return };
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 55.0));
    assert!((r.value(1) - 0.75).abs() < 1e-9, "55 pt up of 220 travel from 0.5: {}", r.value(1));
}

#[test]
fn shift_is_fine_and_alt_resets() {
    let Some(mut r) = Rig::new(1.0) else { return };
    let (x, y) = r.knob;
    r.key(KEY_PRESS, SHIFT_L);
    r.drag(r.knob, (x, y - 55.0));
    r.key(KEY_RELEASE, SHIFT_L);
    assert!((r.value(1) - 0.525).abs() < 1e-9, "Shift is ten times finer: {}", r.value(1));
    r.key(KEY_PRESS, ALT_L);
    r.click(r.knob);
    r.key(KEY_RELEASE, ALT_L);
    assert_eq!(r.value(1), 0.5, "Alt-click resets");
}

#[test]
fn a_double_click_resets() {
    let Some(mut r) = Rig::new(1.0) else { return };
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 30.0));
    assert_ne!(r.value(1), 0.5);
    // Two quick presses in place: the view counts the second as 2.
    r.fake(MOTION, 0, r.knob);
    for kind in [BUTTON_PRESS, BUTTON_RELEASE, BUTTON_PRESS, BUTTON_RELEASE] {
        r.fake(kind, 1, r.knob);
    }
    r.settle();
    assert_eq!(r.value(1), 0.5, "a double click resets");
}

#[test]
fn a_wheel_notch_up_nudges_the_knob_up() {
    let Some(mut r) = Rig::new(1.0) else { return };
    for _ in 0..3 {
        r.button(BUTTON_PRESS, 4, r.knob);
        r.button(BUTTON_RELEASE, 4, r.knob);
    }
    assert!((r.value(1) - 0.53).abs() < 1e-9, "three notches up (button 4): {}", r.value(1));
    r.button(BUTTON_PRESS, 5, r.foot);
    r.button(BUTTON_RELEASE, 5, r.foot);
    assert!((r.value(1) - 0.53).abs() < 1e-9, "the wheel over another part leaves the knob");
}

#[test]
fn at_scale_two_the_window_is_twice_the_size_and_points_map() {
    let Some(mut r) = Rig::new(2.0) else { return };
    let (w, h) = r.view.size();
    let g = r.conn.get_geometry(r.view.window().unwrap()).unwrap().reply().unwrap();
    assert_eq!((u32::from(g.width), u32::from(g.height)), ((w * 2.0).round() as u32, (h * 2.0).round() as u32));
    r.click(r.foot);
    assert_eq!(r.value(2), 1.0, "a click at twice the coordinates is the footswitch");
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 55.0));
    assert!((r.value(1) - 0.75).abs() < 1e-9, "travel is in points, not pixels: {}", r.value(1));
}

#[test]
fn the_window_shows_the_face() {
    let Some(mut r) = Rig::new(1.0) else { return };
    r.click(r.foot);
    let window = r.view.window().unwrap();
    let (w, h) = r.view.host_size();
    let image = r.conn.get_image(ImageFormat::Z_PIXMAP, window, 0, 0, w as u16, h as u16, !0).unwrap().reply().unwrap();
    // The face as the view last painted it.
    let (cw, ch, pixels) = r.view.painted();
    assert_eq!((cw, ch), (w as usize, h as usize));
    assert!(pixels.chunks_exact(4).any(|p| p[3] > 0), "the face painted something");
    let setup = r.conn.setup();
    let format = Format::of(setup, &setup.roots[0]).expect("a TrueColor screen");
    let mut differ = 0;
    for (i, px) in pixels.chunks_exact(4).enumerate() {
        let shown = format.rgb(image.data[i * 4..i * 4 + 4].try_into().unwrap());
        if shown != [px[0], px[1], px[2]] {
            differ += 1;
        }
    }
    assert_eq!(differ, 0, "every pixel of the window is the face's ({w}×{h})");
}
