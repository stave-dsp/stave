//! `StaveScene` — ONE NSView class hosting every `Widget::Scene`
//! (the OPEN display kind): the project (or the kit) supplies
//! a pure picture fn; this host owns the bindings, reads the LIVE
//! values off the bus, resolves the theme PALETTE, and repaints ONLY
//! on value edges (`stave_ui::signal::Changed`). Bespoke visuals
//! never touch the framework again — they are functions.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::scene::{pt, Pt, SceneBuild, SceneDrag, SceneDragPair, SceneInputs, ScenePalette};
use stave_ui::signal::Changed;
use stave_ui::style::Color;

use crate::objc::*;
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

pub(crate) struct SceneView {
    pub(crate) view: Id,
    pub(crate) cis: Vec<usize>,
    pub(crate) pis: Vec<usize>,
    pub(crate) build: SceneBuild,
    pub(crate) drag: Option<SceneDrag>,
    pub(crate) drag_pair: Option<SceneDragPair>,
    pub(crate) gesture: Option<SceneGesture>,
    pub(crate) memo: Changed<Vec<u64>>,
    /// The software renderer's pixels for this view (reused per paint).
    pub(crate) canvas: stave_raster::Canvas,
}

/// A drag in flight: everything the pure map needs, SNAPSHOTTED at
/// mouseDown (no accumulation drift), plus which
/// param the gesture bracket opened on.
pub(crate) struct SceneGesture {
    from: Pt,
    params: Vec<f64>,
    ranges: Vec<(f64, f64)>,
    probes: Vec<f64>,
    ci: usize,
    /// The second param of an XY grab (None for a one-param grab).
    ci2: Option<usize>,
}

#[derive(Default)]
pub(crate) struct SceneWidgets {
    pub(crate) views: Vec<SceneView>,
}

fn role(c: (f64, f64, f64, f64)) -> Color {
    Color::rgba(c.0, c.1, c.2, c.3)
}

/// The theme roles a picture may use, resolved once per paint.
fn palette() -> ScenePalette {
    ScenePalette {
        accent: role(theme::ACCENT),
        ink_hi: role(theme::INK_HI),
        ink_muted: role(theme::INK_MUTED),
        axis: role(theme::AXIS),
        bg: role(theme::BG_1),
    }
}

/// Which renderer paints scenes: the portable software renderer
/// (`stave-raster`, the one every platform shares) unless
/// `STAVE_RENDER=cg` asks for the CoreGraphics reference.
fn use_software() -> bool {
    static CHOICE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHOICE.get_or_init(|| std::env::var("STAVE_RENDER").map(|v| v != "cg").unwrap_or(true))
}

/// The window's backing scale (2 on Retina), 2 if unknown.
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

/// drawRect: — look up the registry entry, gather inputs, build.
unsafe extern "C" fn scene_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    let Some(editor) = (unsafe { scene_editor_mut(this) }) else {
        return;
    };
    let idx = unsafe { ivar_i64(this, c"idx_tag") };
    let Some(sv) = editor.scene_widgets.views.get(idx as usize) else {
        return;
    };
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    // Acquire CGContext from the current NSGraphicsContext (the
    // knob's established pattern).
    let ctx: *mut c_void = unsafe {
        let cur: Id = msg_id(class(c"NSGraphicsContext"), sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        f(cur, sel(c"CGContext"))
    };
    if ctx.is_null() {
        return;
    }
    let params: Vec<f64> = sv.cis.iter().map(|&ci| editor.plain(ci)).collect();
    let ranges: Vec<(f64, f64)> = sv.cis.iter().map(|&ci| editor.param_range(ci)).collect();
    let probes: Vec<f64> = sv
        .pis
        .iter()
        .map(|&pi| f64::from_bits(editor.bus.probes[pi].latest.load(Ordering::Relaxed)))
        .collect();
    let scene = (sv.build)(&SceneInputs {
        params: &params,
        ranges: &ranges,
        probes: &probes,
        w: bounds.size.width,
        h: bounds.size.height,
        palette: palette(),
    });
    if use_software() {
        let scale = unsafe { backing_scale(this) };
        let (w, h) = (bounds.size.width, bounds.size.height);
        if let Some(sv) = editor.scene_widgets.views.get_mut(idx as usize) {
            crate::offscreen::render_raster_into(&scene, w, h, scale, &mut sv.canvas);
            unsafe { crate::offscreen::draw_canvas(ctx, &sv.canvas, w, h) };
        }
        return;
    }
    // The CoreGraphics reference. Scenes draw y-down (widget-local);
    // NSViews are y-up.
    let flipped = flip_y(scene, bounds.size.height);
    unsafe { super::scene_render::render(ctx, &flipped) };
}

/// Flip a widget-local (y-down) scene into the view's y-up space.
pub(crate) fn flip_y(mut scene: stave_ui::scene::Scene, h: f64) -> stave_ui::scene::Scene {
    use stave_ui::scene::{Op, Seg};
    let fy = |p: stave_ui::scene::Pt| pt(p.x, h - p.y);
    for op in scene.ops.iter_mut() {
        let path = match op {
            Op::Fill { path, .. } | Op::Stroke { path, .. } => path,
            Op::Text { at, .. } => {
                *at = fy(*at);
                continue;
            }
            Op::Image { at, h: ih, .. } => {
                // The rect's y-up origin is its BOTTOM-left corner.
                *at = pt(at.x, h - at.y - *ih);
                continue;
            }
        };
        for seg in path.segs.iter_mut() {
            *seg = match *seg {
                Seg::MoveTo(p) => Seg::MoveTo(fy(p)),
                Seg::LineTo(p) => Seg::LineTo(fy(p)),
                Seg::QuadTo(c, p) => Seg::QuadTo(fy(c), fy(p)),
                Seg::CubicTo(c1, c2, p) => Seg::CubicTo(fy(c1), fy(c2), fy(p)),
                Seg::Arc { c, r, a0, a1, cw } => Seg::Arc {
                    c: fy(c),
                    r,
                    a0: -a0,
                    a1: -a1,
                    cw: !cw,
                },
                Seg::Close => Seg::Close,
            };
        }
    }
    scene
}

/// The tick half: repaint only when any bound value moved.
pub(crate) fn update_scene_widgets(v: &mut PluginEditorView) {
    for i in 0..v.scene_widgets.views.len() {
        let now: Vec<u64> = {
            let sv = &v.scene_widgets.views[i];
            sv.cis
                .iter()
                .map(|&ci| v.plain(ci).to_bits())
                .chain(
                    sv.pis
                        .iter()
                        .map(|&pi| v.bus.probes[pi].latest.load(Ordering::Relaxed)),
                )
                .collect()
        };
        let sv = &mut v.scene_widgets.views[i];
        if sv.memo.changed(now) {
            unsafe { msg_void_int(sv.view, sel(c"setNeedsDisplay:"), 1) };
        }
    }
}

// ------------------------------------------------------------ mounting
/// Create a StaveScene view for `kind`/`idx` inside `parent`.
pub(crate) unsafe fn add_scene_view(
    v: &mut PluginEditorView,
    parent: Id,
    rect: CGRect,
    kind: i64,
    idx: i64,
) -> Id {
    unsafe {
        let cls = scene_view_class();
        let raw = msg_id(cls as Id, sel(c"alloc"));
        let view = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        set_ivar_ptr(
            view,
            c"editor_ptr",
            v as *mut PluginEditorView as *mut c_void,
        );
        set_ivar_i64(view, c"kind_tag", kind);
        set_ivar_i64(view, c"idx_tag", idx);
        msg_void_id(parent, sel(c"addSubview:"), view);
        view
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn mount_scene(
    v: &mut PluginEditorView,
    parent: Id,
    rect: CGRect,
    cis: Vec<usize>,
    pis: Vec<usize>,
    build: SceneBuild,
    drag: Option<SceneDrag>,
    drag_pair: Option<SceneDragPair>,
) {
    let idx = v.scene_widgets.views.len() as i64;
    let view = unsafe { add_scene_view(v, parent, rect, 0, idx) };
    v.scene_widgets.views.push(SceneView {
        view,
        cis,
        pis,
        build,
        drag,
        drag_pair,
        gesture: None,
        memo: Changed::new(),
        canvas: stave_raster::Canvas::new(0, 0),
    });
}

// ------------------------------------------------------- interaction
/// Event location in the SCENE's y-down space (views are y-up).
unsafe fn scene_point(this: Id, event: Id) -> Pt {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let win = f(event, sel(c"locationInWindow"));
        let g: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let local = g(
            this,
            sel(c"convertPoint:fromView:"),
            win,
            core::ptr::null_mut(),
        );
        let b: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        let bounds = b(this, sel(c"bounds"));
        pt(local.x, bounds.size.height - local.y)
    }
}

/// mouseDown: snapshot the declared bindings, ask the pure map which
/// param this grab addresses, and open the gesture bracket on it.
unsafe extern "C" fn scene_mouse_down(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { scene_editor_mut(this) }) else {
        return;
    };
    let idx = unsafe { ivar_i64(this, c"idx_tag") } as usize;
    let p = unsafe { scene_point(this, event) };
    let Some(sv) = editor.scene_widgets.views.get(idx) else {
        return;
    };
    if sv.drag.is_none() && sv.drag_pair.is_none() {
        return;
    }
    let params: Vec<f64> = sv.cis.iter().map(|&ci| editor.plain(ci)).collect();
    let ranges: Vec<(f64, f64)> = sv.cis.iter().map(|&ci| editor.param_range(ci)).collect();
    let probes: Vec<f64> = sv
        .pis
        .iter()
        .map(|&pi| f64::from_bits(editor.bus.probes[pi].latest.load(Ordering::Relaxed)))
        .collect();
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let inputs = SceneInputs {
        params: &params,
        ranges: &ranges,
        probes: &probes,
        w: bounds.size.width,
        h: bounds.size.height,
        palette: palette(),
    };
    // An XY grab first: both params open their gesture brackets.
    if let Some(pair) = sv.drag_pair {
        if let Some([(pa, va), (pb, vb)]) = pair(&inputs, p, p) {
            let (Some(&ca), Some(&cb)) = (sv.cis.get(pa), sv.cis.get(pb)) else { return };
            let ci2 = (cb != ca).then_some(cb);
            editor.begin_control(ca);
            if let Some(c2) = ci2 {
                editor.begin_control(c2);
            }
            if va != params[pa] {
                editor.write_control_plain(ca, va, false);
            }
            if ci2.is_some() && vb != params[pb] {
                editor.write_control_plain(cb, vb, false);
            }
            editor.scene_widgets.views[idx].gesture = Some(SceneGesture { from: p, params, ranges, probes, ci: ca, ci2 });
            return;
        }
    }
    let Some(drag) = sv.drag else { return };
    // A zero-length drag reveals the grab's target (the segment is
    // decided by `from` alone) without moving anything yet.
    let Some((pi, _)) = drag(&inputs, p, p) else {
        return;
    };
    let Some(&ci) = sv.cis.get(pi) else { return };
    editor.begin_control(ci);
    // A grab that already moves its param is a CLICK target (a
    // toggle, a footswitch): the press itself is the edit.
    if let Some((_, plain)) = drag(&inputs, p, p) {
        if plain != params[pi] {
            editor.write_control_plain(ci, plain, false);
        }
    }
    editor.scene_widgets.views[idx].gesture = Some(SceneGesture {
        from: p,
        params,
        ranges,
        probes,
        ci,
        ci2: None,
    });
}

/// mouseDragged: replay the pure map from the GRAB snapshot to the
/// current point; write through the single editor write path.
unsafe extern "C" fn scene_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let Some(editor) = (unsafe { scene_editor_mut(this) }) else {
        return;
    };
    let idx = unsafe { ivar_i64(this, c"idx_tag") } as usize;
    let p = unsafe { scene_point(this, event) };
    let Some(sv) = editor.scene_widgets.views.get(idx) else {
        return;
    };
    let Some(g) = sv.gesture.as_ref() else {
        return;
    };
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let inputs = SceneInputs {
        params: &g.params,
        ranges: &g.ranges,
        probes: &g.probes,
        w: bounds.size.width,
        h: bounds.size.height,
        palette: palette(),
    };
    if let (Some(pair), Some(ci2)) = (sv.drag_pair, g.ci2) {
        if let Some([(pa, va), (pb, vb)]) = pair(&inputs, g.from, p) {
            if sv.cis.get(pa) == Some(&g.ci) && sv.cis.get(pb) == Some(&ci2) {
                editor.write_control_plain(g.ci, va, false);
                editor.write_control_plain(ci2, vb, false);
            }
        }
        return;
    }
    if let Some(pair) = sv.drag_pair {
        if let Some([(pa, va), _]) = pair(&inputs, g.from, p) {
            if sv.cis.get(pa) == Some(&g.ci) {
                editor.write_control_plain(g.ci, va, false);
                return;
            }
        }
    }
    let Some(drag) = sv.drag else { return };
    if let Some((pi, plain)) = drag(&inputs, g.from, p) {
        if sv.cis.get(pi) == Some(&g.ci) {
            editor.write_control_plain(g.ci, plain, false);
        }
    }
}

/// mouseUp: close the bracket, drop the snapshot.
unsafe extern "C" fn scene_mouse_up(this: Id, _cmd: Sel, _event: Id) {
    let Some(editor) = (unsafe { scene_editor_mut(this) }) else {
        return;
    };
    let idx = unsafe { ivar_i64(this, c"idx_tag") } as usize;
    let Some(sv) = editor.scene_widgets.views.get_mut(idx) else {
        return;
    };
    if let Some(g) = sv.gesture.take() {
        editor.end_control(g.ci);
        if let Some(c2) = g.ci2 {
            editor.end_control(c2);
        }
    }
}

// ----------------------------------------------------- objc plumbing
unsafe fn scene_editor_mut(this: Id) -> Option<&'static mut PluginEditorView> {
    let ptr = unsafe { ivar_ptr(this, c"editor_ptr") } as *mut PluginEditorView;
    unsafe { ptr.as_mut() }
}

unsafe fn ivar_of(_this: Id, name: &core::ffi::CStr) -> *mut c_void {
    unsafe {
        let cls = objc_lookUpClass(c"StaveScene".as_ptr());
        class_getInstanceVariable(cls, name.as_ptr())
    }
}
unsafe fn ivar_i64(this: Id, name: &core::ffi::CStr) -> i64 {
    let ivar = unsafe { ivar_of(this, name) };
    if ivar.is_null() {
        return -1;
    }
    unsafe { object_getIvar(this, ivar) as i64 }
}
unsafe fn ivar_ptr(this: Id, name: &core::ffi::CStr) -> *mut c_void {
    let ivar = unsafe { ivar_of(this, name) };
    if ivar.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { object_getIvar(this, ivar) }
}
unsafe fn set_ivar_i64(this: Id, name: &core::ffi::CStr, value: i64) {
    let ivar = unsafe { ivar_of(this, name) };
    if !ivar.is_null() {
        unsafe { object_setIvar(this, ivar, value as *mut c_void as Id) };
    }
}
unsafe fn set_ivar_ptr(this: Id, name: &core::ffi::CStr, value: *mut c_void) {
    let ivar = unsafe { ivar_of(this, name) };
    if !ivar.is_null() {
        unsafe { object_setIvar(this, ivar, value as Id) };
    }
}

unsafe fn scene_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveScene".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    unsafe {
        let cls = objc_allocateClassPair(class(c"NSView"), c"StaveScene".as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let _ = class_addIvar(
            cls,
            c"kind_tag".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        );
        let _ = class_addIvar(
            cls,
            c"idx_tag".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        );
        let _ = class_addMethod(
            cls,
            sel(c"drawRect:"),
            scene_draw_rect as *const c_void,
            c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
        );
        for (name, imp) in [
            (c"mouseDown:", scene_mouse_down as *const c_void),
            (c"mouseDragged:", scene_mouse_dragged as *const c_void),
            (c"mouseUp:", scene_mouse_up as *const c_void),
        ] {
            let _ = class_addMethod(cls, sel(name), imp, c"v@:@".as_ptr());
        }
        objc_registerClassPair(cls);
        cls
    }
}
