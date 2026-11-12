//! The CABLE INSPECTOR — click a patch cable and its
//! route's SHAPING chain opens in a small overlay panel: VIA (the
//! depth scaled by a second source), MODE (bipolar/unipolar), QUANT
//! (step count) as themed dropdowns, and LAG as a mini slider. The
//! bindings come from the `ModRings` declaration's per-slot extras;
//! every edit is an ordinary param gesture. The overlay-primitive
//! pattern's second consumer (the dropdown menu was the first).

use core::ffi::c_void;

extern crate alloc;

use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use crate::objc::*;
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

const PANEL_W: f64 = 190.0;
const ROW_H: f64 = 30.0;
const PAD: f64 = 10.0;
const TITLE_H: f64 = 22.0;

pub(crate) struct InspectorState {
    pub(crate) scrim: Id,
    /// Dropdown fields registered while the panel is open — the
    /// tail of `v.dropdowns.fields` past this mark is OURS and gets
    /// truncated at dismissal (the views die with the scrim).
    pub(crate) dropdown_mark: usize,
    /// (lag ci, slider view) — the mini slider's binding.
    pub(crate) lag: Option<(usize, Id)>,
    /// A lag drag in flight.
    pub(crate) dragging: bool,
}

fn col(c: theme::Rgba) -> Color {
    Color::rgba(c.0, c.1, c.2, c.3)
}

/// Open the inspector for a route slot. No-op when the project
/// declared no shaping extras.
pub(crate) unsafe fn open(v: &mut PluginEditorView, slot: usize, at: CGPoint) {
    let host = v.dropdowns.host;
    if host.is_null() {
        return;
    }
    let Some(stave_core::editor::SlotShaping {
        via: via_ci,
        mode: mode_ci,
        lag: lag_ci,
        quant: quant_ci,
    }) = v
        .rings
        .as_ref()
        .and_then(|r| r.extras.get(slot).copied().flatten())
    else {
        return;
    };
    let (src_ci, tgt_ci) = match v.rings.as_ref() {
        Some(r) => (r.slots[slot].0, r.slots[slot].1),
        None => return,
    };
    unsafe {
        close(v);
        let host_bounds: CGRect = msg_rect(host, sel(c"bounds"));
        let panel_h = TITLE_H + 4.0 * ROW_H + PAD * 2.0;
        let x = (at.x - PANEL_W * 0.5).clamp(8.0, host_bounds.size.width - PANEL_W - 8.0);
        let y = (at.y + 10.0)
            .min(host_bounds.size.height - panel_h - 8.0)
            .max(8.0);

        let scrim = {
            let raw = msg_id(scrim_class() as Id, sel(c"alloc"));
            let s = msg_id_rect(raw, sel(c"initWithFrame:"), host_bounds);
            set_editor_ivar(scrim_class(), s, v.self_ptr);
            msg_void_id(host, sel(c"addSubview:"), s);
            s
        };
        let panel = {
            let raw = msg_id(panel_class() as Id, sel(c"alloc"));
            let rect = CGRect {
                origin: CGPoint { x, y },
                size: CGSize {
                    width: PANEL_W,
                    height: panel_h,
                },
            };
            let p = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            set_editor_ivar(panel_class(), p, v.self_ptr);
            msg_void_id(scrim, sel(c"addSubview:"), p);
            p
        };
        // Title: "LFO → LEVEL", from the params' own step names.
        // The panel is y-up inside; rows lay out from the top.
        let row_y = |i: f64| panel_h - TITLE_H - PAD - (i + 1.0) * ROW_H + (ROW_H - 26.0) * 0.5;
        let title = {
            let src_k = plain_usize(v, src_ci);
            let tgt_t = plain_usize(v, tgt_ci);
            let s = v
                .param(src_ci)
                .step_name(src_k as u32)
                .unwrap_or("")
                .to_uppercase();
            let t = v
                .param(tgt_ci)
                .step_name(tgt_t as u32)
                .unwrap_or("")
                .to_uppercase();
            alloc::format!("{s} \u{2192} {t}")
        };
        let tl = make_label(CGRect {
            origin: CGPoint {
                x: PAD,
                y: panel_h - TITLE_H - 2.0,
            },
            size: CGSize {
                width: PANEL_W - 2.0 * PAD,
                height: 16.0,
            },
        });
        label_set_kerned(
            tl,
            &title,
            ns_font_role(theme::role_group()),
            ns_color_token(theme::INK_HI),
            0.8,
        );
        msg_void_id(panel, sel(c"addSubview:"), tl);

        let mark = v.dropdowns.fields.len();
        let mut row = 0.0;
        for (name, ci) in [("VIA", via_ci), ("MODE", mode_ci), ("QUANT", quant_ci)] {
            let lbl = make_label(CGRect {
                origin: CGPoint {
                    x: PAD,
                    y: row_y(row) + 5.0,
                },
                size: CGSize {
                    width: 48.0,
                    height: 14.0,
                },
            });
            label_set_kerned(
                lbl,
                name,
                ns_font_role(theme::role_tick()),
                ns_color_token(theme::INK_LO),
                0.6,
            );
            msg_void_id(panel, sel(c"addSubview:"), lbl);
            crate::widgets::dropdown::add_dropdown_field(
                v,
                panel,
                ci,
                CGRect {
                    origin: CGPoint {
                        x: PAD + 52.0,
                        y: row_y(row),
                    },
                    size: CGSize {
                        width: PANEL_W - PAD * 2.0 - 52.0,
                        height: 26.0,
                    },
                },
            );
            row += 1.0;
        }
        // LAG: label + mini slider.
        let lbl = make_label(CGRect {
            origin: CGPoint {
                x: PAD,
                y: row_y(row) + 5.0,
            },
            size: CGSize {
                width: 48.0,
                height: 14.0,
            },
        });
        label_set_kerned(
            lbl,
            "LAG",
            ns_font_role(theme::role_tick()),
            ns_color_token(theme::INK_LO),
            0.6,
        );
        msg_void_id(panel, sel(c"addSubview:"), lbl);
        let slider = {
            let raw = msg_id(slider_class() as Id, sel(c"alloc"));
            let rect = CGRect {
                origin: CGPoint {
                    x: PAD + 52.0,
                    y: row_y(row),
                },
                size: CGSize {
                    width: PANEL_W - PAD * 2.0 - 52.0,
                    height: 26.0,
                },
            };
            let sl = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            set_editor_ivar(slider_class(), sl, v.self_ptr);
            msg_void_int(sl, sel(c"setTag:"), lag_ci as i64);
            msg_void_id(panel, sel(c"addSubview:"), sl);
            sl
        };
        v.inspector = Some(InspectorState {
            scrim,
            dropdown_mark: mark,
            lag: Some((lag_ci, slider)),
            dragging: false,
        });
    }
}

pub(crate) unsafe fn close(v: &mut PluginEditorView) {
    if let Some(ins) = v.inspector.take() {
        unsafe {
            // The panel's dropdowns die with the scrim; unregister
            // their tick entries.
            crate::widgets::dropdown::dismiss(v);
            v.dropdowns.fields.truncate(ins.dropdown_mark);
            let _ = msg_id(ins.scrim, sel(c"removeFromSuperview"));
        }
    }
}

// ------------------------------------------------------------ helpers
fn plain_usize(v: &PluginEditorView, ci: usize) -> usize {
    v.plain(ci) as usize
}

unsafe fn msg_rect(obj: Id, s: Sel) -> CGRect {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
}

unsafe fn make_label(rect: CGRect) -> Id {
    unsafe {
        let raw = msg_id(class(c"NSTextField"), sel(c"alloc"));
        let lbl = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(lbl, sel(c"setBezeled:"), 0);
        msg_void_int(lbl, sel(c"setDrawsBackground:"), 0);
        msg_void_int(lbl, sel(c"setEditable:"), 0);
        msg_void_int(lbl, sel(c"setSelectable:"), 0);
        lbl
    }
}

unsafe fn editor_of(cls: Class, this: Id) -> Option<&'static mut PluginEditorView> {
    unsafe {
        let ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
        if ivar.is_null() {
            return None;
        }
        (object_getIvar(this, ivar) as *mut PluginEditorView).as_mut()
    }
}

unsafe fn set_editor_ivar(cls: Class, this: Id, editor: *mut PluginEditorView) {
    unsafe {
        let ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
        if !ivar.is_null() {
            object_setIvar(this, ivar, editor as Id);
        }
    }
}

unsafe fn draw_scene_here(scene: &Scene) {
    unsafe {
        let cur: Id = msg_id(class(c"NSGraphicsContext"), sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        let ctx = f(cur, sel(c"CGContext"));
        if !ctx.is_null() {
            super::scene_render::render(ctx, scene);
        }
    }
}

fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> Path {
    Path::new()
        .move_to(pt(x + r, y))
        .line_to(pt(x + w - r, y))
        .quad_to(pt(x + w, y), pt(x + w, y + r))
        .line_to(pt(x + w, y + h - r))
        .quad_to(pt(x + w, y + h), pt(x + w - r, y + h))
        .line_to(pt(x + r, y + h))
        .quad_to(pt(x, y + h), pt(x, y + h - r))
        .line_to(pt(x, y + r))
        .quad_to(pt(x, y), pt(x + r, y))
        .close()
}

// ------------------------------------------------------ objc methods
unsafe extern "C" fn panel_draw(this: Id, _cmd: Sel, _dirty: CGRect) {
    let b = unsafe { msg_rect(this, sel(c"bounds")) };
    let (w, h) = (b.size.width, b.size.height);
    let mut sc = Scene::new();
    sc.solid(rounded(0.5, 0.5, w - 1.0, h - 1.0, 7.0), col(theme::BG_1));
    sc.stroke(
        rounded(0.5, 0.5, w - 1.0, h - 1.0, 7.0),
        Stroke::new(col(theme::LINE), 1.0),
    );
    unsafe { draw_scene_here(&sc) };
}

/// The panel swallows body clicks (else they'd bubble to the scrim
/// and dismiss).
unsafe extern "C" fn panel_mouse_down(_this: Id, _cmd: Sel, _event: Id) {}

unsafe extern "C" fn scrim_mouse_down(this: Id, _cmd: Sel, _event: Id) {
    if let Some(v) = unsafe { editor_of(scrim_class(), this) } {
        unsafe { close(v) };
    }
}

unsafe extern "C" fn scrim_is_flipped(_this: Id, _cmd: Sel) -> bool {
    true
}

// ---- the mini slider: absolute-position, declaration-clamped ----
unsafe fn slider_write(v: &mut PluginEditorView, this: Id, event: Id, ci: usize) {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let win = f(event, sel(c"locationInWindow"));
        let g: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let p = g(
            this,
            sel(c"convertPoint:fromView:"),
            win,
            core::ptr::null_mut(),
        );
        let b = msg_rect(this, sel(c"bounds"));
        let t = ((p.x - 4.0) / (b.size.width - 8.0)).clamp(0.0, 1.0);
        let (min, max) = v.param_range(ci);
        v.write_control_plain(ci, min + t * (max - min), false);
        msg_void_int(this, sel(c"setNeedsDisplay:"), 1);
    }
}

unsafe extern "C" fn slider_mouse_down(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(slider_class(), this) }) else {
        return;
    };
    let ci = unsafe { view_tag(this) };
    if ci < 0 {
        return;
    }
    v.begin_control(ci as usize);
    if let Some(ins) = v.inspector.as_mut() {
        ins.dragging = true;
    }
    unsafe { slider_write(v, this, event, ci as usize) };
}

unsafe extern "C" fn slider_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(slider_class(), this) }) else {
        return;
    };
    let ci = unsafe { view_tag(this) };
    if ci >= 0 && v.inspector.as_ref().is_some_and(|i| i.dragging) {
        unsafe { slider_write(v, this, event, ci as usize) };
    }
}

unsafe extern "C" fn slider_mouse_up(this: Id, _cmd: Sel, _event: Id) {
    let Some(v) = (unsafe { editor_of(slider_class(), this) }) else {
        return;
    };
    let ci = unsafe { view_tag(this) };
    if ci >= 0 {
        v.end_control(ci as usize);
    }
    if let Some(ins) = v.inspector.as_mut() {
        ins.dragging = false;
    }
}

unsafe extern "C" fn slider_draw(this: Id, _cmd: Sel, _dirty: CGRect) {
    let Some(v) = (unsafe { editor_of(slider_class(), this) }) else {
        return;
    };
    let ci = unsafe { view_tag(this) };
    if ci < 0 {
        return;
    }
    let (min, max) = v.param_range(ci as usize);
    let val = v.plain(ci as usize);
    let t = if max > min {
        ((val - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let b = unsafe { msg_rect(this, sel(c"bounds")) };
    let (w, cy) = (b.size.width, b.size.height * 0.5);
    let a = theme::ACCENT;
    let mut sc = Scene::new();
    sc.stroke(
        Path::polyline(&[pt(4.0, cy), pt(w - 4.0, cy)]),
        Stroke::new(Color::rgba(1.0, 1.0, 1.0, 0.18), 2.0),
    );
    let tx = 4.0 + t * (w - 8.0);
    sc.stroke(
        Path::polyline(&[pt(4.0, cy), pt(tx, cy)]),
        Stroke::round(Color::rgba(a.0, a.1, a.2, 0.9), 2.0),
    );
    sc.solid(Path::circle(pt(tx, cy), 3.5), col(theme::ACCENT));
    unsafe { draw_scene_here(&sc) };
}

/// The tick half: keep the lag slider honest against the bus.
pub(crate) fn update_inspector(v: &mut PluginEditorView) {
    if let Some(ins) = &v.inspector {
        if let Some((_, slider)) = ins.lag {
            unsafe { msg_void_int(slider, sel(c"setNeedsDisplay:"), 1) };
        }
    }
}

unsafe extern "C" fn view_tag(this: Id) -> i64 {
    unsafe {
        let cls = object_getClass(this);
        let ivar = class_getInstanceVariable(cls, c"ci_tag".as_ptr());
        if ivar.is_null() {
            return -1;
        }
        object_getIvar(this, ivar) as i64
    }
}

unsafe extern "C" fn view_set_tag(this: Id, _cmd: Sel, value: i64) {
    unsafe {
        let cls = object_getClass(this);
        let ivar = class_getInstanceVariable(cls, c"ci_tag".as_ptr());
        if !ivar.is_null() {
            object_setIvar(this, ivar, value as Id);
        }
    }
}

unsafe extern "C" fn view_tag_method(this: Id, _cmd: Sel) -> i64 {
    unsafe { view_tag(this) }
}

// ------------------------------------------------------ class setup
unsafe fn get_or_make(name: &core::ffi::CStr, build: fn(Class)) -> Class {
    unsafe {
        let existing = objc_lookUpClass(name.as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let cls = objc_allocateClassPair(class(c"NSView"), name.as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let _ = class_addIvar(
            cls,
            c"ci_tag".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        );
        build(cls);
        objc_registerClassPair(cls);
        cls
    }
}

unsafe fn scrim_class() -> Class {
    unsafe {
        get_or_make(c"StaveInspectorScrim", |cls| {
            let _ = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                scrim_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
            let _ = class_addMethod(
                cls,
                sel(c"isFlipped"),
                scrim_is_flipped as *const c_void,
                c"B@:".as_ptr(),
            );
        })
    }
}

unsafe fn panel_class() -> Class {
    unsafe {
        get_or_make(c"StaveInspectorPanel", |cls| {
            let _ = class_addMethod(
                cls,
                sel(c"drawRect:"),
                panel_draw as *const c_void,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let _ = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                panel_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
        })
    }
}

unsafe fn slider_class() -> Class {
    unsafe {
        get_or_make(c"StaveMiniSlider", |cls| {
            for (name, imp, ty) in [
                (
                    c"drawRect:",
                    slider_draw as *const c_void,
                    c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}",
                ),
                (c"mouseDown:", slider_mouse_down as *const c_void, c"v@:@"),
                (
                    c"mouseDragged:",
                    slider_mouse_dragged as *const c_void,
                    c"v@:@",
                ),
                (c"mouseUp:", slider_mouse_up as *const c_void, c"v@:@"),
                (c"tag", view_tag_method as *const c_void, c"q@:"),
                (c"setTag:", view_set_tag as *const c_void, c"v@:q"),
            ] {
                let _ = class_addMethod(cls, sel(name), imp, ty.as_ptr());
            }
        })
    }
}
