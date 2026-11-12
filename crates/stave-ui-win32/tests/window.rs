//! The face in a real child window, driven by the window messages the
//! system sends (`SendMessageW` runs the window procedure synchronously,
//! so no desktop input is needed and CI's session suffices): the view's
//! mapping from client pixels to face points at a scale, its wheel
//! units (screen coordinates, 120ths of a notch), the modifier bits and
//! click counting are tested, not assumed — and the pixels it paints
//! (`WM_PRINTCLIENT` into a DIB section) are the face's.

#![cfg(windows)]

use std::sync::Arc;

use stave_core::params::{AnyParam, BoolParam, FloatParam, ParamId, Params, Range};
use stave_core::{GestureSink, PluginController, Telemetry};
use stave_face::prelude::*;
use stave_face::Runtime;
use stave_ui_win32::sys::*;
use stave_ui_win32::FaceView;

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

struct Rig {
    parent: HWND,
    view: FaceView,
    scale: f64,
    knob: (f64, f64),
    foot: (f64, f64),
}

impl Rig {
    fn new(scale: Option<f64>) -> Rig {
        let t = table();
        let rt = Runtime::new(face(&t), &stave_font::FontText);
        let probe = controller(&t);
        let at = |id| {
            let c = rt.grabs(&probe, PartId::Param(ParamId(id)))[0].area.centre();
            (c.x, c.y)
        };
        let (knob, foot) = (at(1), at(2));
        let mut view = FaceView::new(face(&t), Box::new(controller(&t)));
        if let Some(s) = scale {
            view.set_scale(s);
        }
        let class = wide("STATIC");
        // SAFETY: a never-shown top-level window of the system's STATIC
        // class, destroyed in `drop`.
        let parent = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                core::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                800,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert!(!parent.is_null(), "a parent window");
        // SAFETY: the live parent, on this thread.
        unsafe { view.attach(parent) };
        assert!(view.hwnd().is_some(), "the view made its window");
        let scale = view.scale();
        Rig { parent, view, scale, knob, foot }
    }

    fn hwnd(&self) -> HWND {
        self.view.hwnd().unwrap()
    }

    fn lparam(&self, (x, y): (f64, f64)) -> LPARAM {
        lparam_xy((x * self.scale).round() as i32, (y * self.scale).round() as i32)
    }

    fn send(&self, msg: u32, w: WPARAM, l: LPARAM) {
        // SAFETY: the view's live window, on its thread.
        unsafe { SendMessageW(self.hwnd(), msg, w, l) };
    }

    fn press(&self, p: (f64, f64), keys: usize) {
        self.send(WM_LBUTTONDOWN, MK_LBUTTON | keys, self.lparam(p));
    }
    fn drag_to(&self, p: (f64, f64), keys: usize) {
        self.send(WM_MOUSEMOVE, MK_LBUTTON | keys, self.lparam(p));
    }
    fn release(&self, p: (f64, f64)) {
        self.send(WM_LBUTTONUP, 0, self.lparam(p));
    }
    fn click(&self, p: (f64, f64)) {
        self.press(p, 0);
        self.release(p);
    }
    fn drag(&self, from: (f64, f64), to: (f64, f64), keys: usize) {
        self.press(from, keys);
        self.drag_to(to, keys);
        self.release(to);
    }

    /// `notches` of the wheel at face point `p` (screen coordinates in
    /// the message, as the system sends them).
    fn wheel(&self, (x, y): (f64, f64), notches: i32) {
        let mut pt = POINT { x: (x * self.scale).round() as i32, y: (y * self.scale).round() as i32 };
        // SAFETY: the live window; `pt` is ours.
        unsafe { ClientToScreen(self.hwnd(), &mut pt) };
        let w = ((notches * 120) as i16 as u16 as usize) << 16;
        self.send(WM_MOUSEWHEEL, w, lparam_xy(pt.x, pt.y));
    }

    fn value(&self, id: u32) -> f64 {
        self.view.controller().normalized(ParamId(id))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.view.detach();
        // SAFETY: our parent window, on this thread.
        unsafe { DestroyWindow(self.parent) };
    }
}

/// Hold Alt (VK_MENU) in this thread's keyboard state for `f`.
fn with_alt(f: impl FnOnce()) {
    let mut keys = [0u8; 256];
    // SAFETY: a 256-byte state buffer, this thread's.
    unsafe { GetKeyboardState(keys.as_mut_ptr()) };
    keys[VK_MENU as usize] |= 0x80;
    // SAFETY: as above.
    unsafe { SetKeyboardState(keys.as_ptr()) };
    f();
    keys[VK_MENU as usize] &= !0x80;
    // SAFETY: as above.
    unsafe { SetKeyboardState(keys.as_ptr()) };
}

#[test]
fn a_click_lands_on_the_part_under_it() {
    let r = Rig::new(Some(1.0));
    r.click(r.foot);
    assert_eq!(r.value(2), 1.0, "the footswitch");
    assert_eq!(r.value(1), 0.5, "the knob is untouched");
}

#[test]
fn dragging_up_raises_the_knob() {
    let r = Rig::new(Some(1.0));
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 55.0), 0);
    assert!((r.value(1) - 0.75).abs() < 1e-9, "55 pt up of 220 travel from 0.5: {}", r.value(1));
}

#[test]
fn shift_is_fine_and_alt_resets() {
    let r = Rig::new(Some(1.0));
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 55.0), MK_SHIFT);
    assert!((r.value(1) - 0.525).abs() < 1e-9, "Shift is ten times finer: {}", r.value(1));
    with_alt(|| r.click(r.knob));
    assert_eq!(r.value(1), 0.5, "Alt-click resets");
}

#[test]
fn a_double_click_resets() {
    let r = Rig::new(Some(1.0));
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 30.0), 0);
    assert_ne!(r.value(1), 0.5);
    r.click(r.knob);
    r.click(r.knob);
    assert_eq!(r.value(1), 0.5, "two quick clicks in place count as a double click");
}

#[test]
fn a_wheel_notch_up_nudges_the_knob_up() {
    let r = Rig::new(Some(1.0));
    r.wheel(r.knob, 3);
    assert!((r.value(1) - 0.53).abs() < 1e-9, "three notches up: {}", r.value(1));
    r.wheel(r.foot, 3);
    assert!((r.value(1) - 0.53).abs() < 1e-9, "the wheel over another part leaves the knob");
}

#[test]
fn at_a_scale_the_window_is_scaled_and_points_map() {
    let r = Rig::new(Some(1.5));
    let (w, h) = r.view.size();
    let mut rect = RECT::default();
    // SAFETY: the live window; `rect` is ours.
    unsafe { GetClientRect(r.hwnd(), &mut rect) };
    assert_eq!((rect.right, rect.bottom), ((w * 1.5).round() as i32, (h * 1.5).round() as i32));
    r.click(r.foot);
    assert_eq!(r.value(2), 1.0, "a click at 1.5× the coordinates is the footswitch");
    let (x, y) = r.knob;
    r.drag(r.knob, (x, y - 55.0), 0);
    // Messages carry whole pixels: at 1.5 px/pt a point lands within
    // a pixel, so the drag is exact to 1 px of 220 pt travel.
    let px = 1.0 / 1.5 / 220.0;
    assert!((r.value(1) - 0.75).abs() <= px, "travel is in points, not pixels: {}", r.value(1));
}

#[test]
fn the_parents_dpi_sets_the_scale() {
    let r = Rig::new(None);
    // SAFETY: the live parent.
    let dpi = unsafe { GetDpiForWindow(r.parent) };
    assert_eq!(r.view.scale(), f64::from(dpi) / 96.0);
    assert_eq!(r.view.host_size(), {
        let (w, h) = r.view.size();
        ((w * r.view.scale()).round() as u32, (h * r.view.scale()).round() as u32)
    });
}

#[test]
fn the_window_paints_the_face() {
    let r = Rig::new(Some(1.0));
    r.click(r.foot);
    let (w, h) = r.view.host_size();
    let info = BITMAPINFO::top_down(w as i32, h as i32);
    let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: a memory DC and a DIB section of the face's size, selected
    // into it, released at the end; the window paints into the DC.
    let shown: Vec<u8> = unsafe {
        let dc = CreateCompatibleDC(core::ptr::null_mut());
        let dib = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, core::ptr::null_mut(), 0);
        assert!(!dib.is_null() && !bits.is_null());
        let old = SelectObject(dc, dib);
        SendMessageW(r.hwnd(), WM_PRINTCLIENT, dc as WPARAM, PRF_CLIENT);
        let shown = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
        SelectObject(dc, old);
        DeleteObject(dib);
        DeleteDC(dc);
        shown
    };
    let (cw, ch, painted) = r.view.painted();
    assert_eq!((cw, ch), (w as usize, h as usize));
    assert!(painted.chunks_exact(4).any(|p| p[3] > 0), "the face painted something");
    let differ = painted
        .chunks_exact(4)
        .zip(shown.chunks_exact(4))
        .filter(|(p, s)| [p[2], p[1], p[0]] != [s[0], s[1], s[2]])
        .count();
    assert_eq!(differ, 0, "every pixel of the window is the face's ({w}×{h})");
}

#[test]
fn a_snapshot_is_the_face_at_its_scale() {
    let r = Rig::new(Some(1.0));
    let png = r.view.snapshot(2.0);
    assert_eq!(&png[1..4], b"PNG");
    let (w, h) = r.view.size();
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as f64;
    assert_eq!((size(16), size(20)), ((w * 2.0).round(), (h * 2.0).round()));
}
