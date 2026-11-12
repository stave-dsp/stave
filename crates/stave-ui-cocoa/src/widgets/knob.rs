use core::ffi::{c_char, c_void};

extern crate alloc;

use crate::theme;

use crate::*;

// ============================================================================
// StaveKnob — custom NSView circular knob primitive
//
// Renders a minimalist rotary knob: a flat dark face, a recessed track
// groove, and a band-tinted value arc with a soft additive glow (drawn
// as widening low-alpha passes — a cheap CoreGraphics bloom). Drag
// up/down to change value; shift to fine-tune.
// ============================================================================

pub(crate) use super::cg::*;

/// drawRect: callback for StaveKnob. Reads the ivar `editor_ptr` for
/// the bus pointer and the view's `tag` for the control index, then
/// draws the knob into the current NSGraphicsContext.
pub(crate) unsafe extern "C" fn knob_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    // bounds rect of self
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let cls = unsafe { objc_lookUpClass(c"StaveKnob".as_ptr()) };
    let bi_ivar = unsafe { class_getInstanceVariable(cls, c"band_idx".as_ptr()) };
    let accent_ivar = unsafe { class_getInstanceVariable(cls, c"accent_mode".as_ptr()) };
    let Some(editor) = (unsafe { knob_editor(this) }) else {
        return;
    };
    let editor = &*editor;
    let ci: i64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> i64 =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"tag"))
    };
    if ci < 0 || (ci as usize) >= editor.param_count() {
        return;
    }
    let ci = ci as usize;
    let normalized = editor.param(ci).normalized();
    let band_idx: i64 = if !bi_ivar.is_null() {
        let v = unsafe { object_getIvar(this, bi_ivar) } as i64;
        v - 1 // we store band+1 so 0 means "not set"
    } else {
        -1
    };
    let neutral_accent =
        !accent_ivar.is_null() && (unsafe { object_getIvar(this, accent_ivar) } as i64) == 1;
    let band_rgb = if (0..=2).contains(&band_idx) {
        theme::BAND_PALETTE[band_idx as usize]
    } else if neutral_accent {
        (theme::INK_MD.0, theme::INK_MD.1, theme::INK_MD.2)
    } else {
        (theme::ACCENT.0, theme::ACCENT.1, theme::ACCENT.2)
    };

    // Acquire CGContext from the current NSGraphicsContext.
    let ngc_cls = class(c"NSGraphicsContext");
    let cur: Id = unsafe { msg_id(ngc_cls, sel(c"currentContext")) };
    if cur.is_null() {
        return;
    }
    let ctx: *mut c_void = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        f(cur, sel(c"CGContext"))
    };
    if ctx.is_null() {
        return;
    }

    // The picture is a PURE FUNCTION (stave_ui::scenes::knob): geometry, passes, and the
    // value-angle equation live there under headless scene invariants; this
    // callback only gathers state and hands the scene to the renderer.
    let cx = bounds.origin.x + bounds.size.width * 0.5;
    let cy = bounds.origin.y + bounds.size.height * 0.5;
    // Knob diameter: smaller of width/height, with breathing room so the
    // glow halo has space to bleed without clipping at the bounds.
    let dia = bounds.size.width.min(bounds.size.height) - 8.0;
    let scene = stave_ui::scenes::knob::scene(
        &stave_ui::scenes::knob::KnobState {
            center: stave_ui::scene::pt(cx, cy),
            radius: dia * 0.5,
            value01: normalized.clamp(0.0, 1.0),
            mod_ring: super::rings::ring_for(editor, ci), // lit
        },
        &stave_ui::scenes::knob::KnobStyle {
            band: stave_ui::style::Color::rgba(band_rgb.0, band_rgb.1, band_rgb.2, 1.0),
            track: stave_ui::style::Color::rgba(theme::AXIS.0, theme::AXIS.1, theme::AXIS.2, 0.45),
            ink_hi: stave_ui::style::Color::rgba(
                theme::INK_HI.0,
                theme::INK_HI.1,
                theme::INK_HI.2,
                1.0,
            ),
        },
    );
    unsafe { super::scene_render::render(ctx, &scene) };
}

/// Fetch the owning editor from a StaveKnob instance's ivar.
pub(crate) unsafe fn knob_editor(this: Id) -> Option<&'static mut PluginEditorView> {
    let cls = unsafe { objc_lookUpClass(c"StaveKnob".as_ptr()) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    if ed_ivar.is_null() {
        return None;
    }
    let editor_ptr = unsafe { object_getIvar(this, ed_ivar) } as *mut PluginEditorView;
    if editor_ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *editor_ptr })
    }
}

pub(crate) unsafe extern "C" fn knob_mouse_down(this: Id, _cmd: Sel, event: Id) {
    // location in window
    let loc: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"locationInWindow"))
    };
    let Some(editor) = (unsafe { knob_editor(this) }) else {
        return;
    };
    let ci: i64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> i64 =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"tag"))
    };
    if ci < 0 || (ci as usize) >= editor.param_count() {
        return;
    }
    let ci = ci as usize;
    let v = editor.param(ci).normalized() as f64;
    editor.knob_drag_anchor = Some((ci, loc.y, v));
    // Gesture bracket opens here; knob_mouse_up closes it.
    editor.begin_control(ci);
}

/// mouseUp: close the gesture bracket and drop the drag anchor.
pub(crate) unsafe extern "C" fn knob_mouse_up(this: Id, _cmd: Sel, _event: Id) {
    let Some(editor) = (unsafe { knob_editor(this) }) else {
        return;
    };
    if let Some((ci, _, _)) = editor.knob_drag_anchor.take() {
        editor.end_control(ci);
    }
}

/// mouseDragged: compute Δy, convert to value delta, write to bus.
pub(crate) unsafe extern "C" fn knob_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let loc: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"locationInWindow"))
    };
    let Some(editor) = (unsafe { knob_editor(this) }) else {
        return;
    };
    let (ci, anchor_y, anchor_v) = match editor.knob_drag_anchor {
        Some(t) => t,
        None => return,
    };
    if ci >= editor.param_count() {
        return;
    }
    // 1 pixel of drag = 0.4% of the range. Shift to fine-tune (1/4).
    let mods: u64 = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> u64 =
            core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"modifierFlags"))
    };
    let shift_held = (mods & (1 << 17)) != 0; // NSEventModifierFlagShift
    let scale = if shift_held { 0.001 } else { 0.004 };
    let dy = loc.y - anchor_y;
    let new_v = (anchor_v + dy * scale).clamp(0.0, 1.0);
    let plain = editor.param(ci).normalized_to_plain(new_v.clamp(0.0, 1.0));
    // Drag path: begin/end bracketed at mouseDown/mouseUp.
    editor.write_control_plain(ci, plain, false);
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, bool) =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"setNeedsDisplay:"), true);
    }
}

/// Lazily create the StaveKnob class. NSView subclass with three
/// custom methods (drawRect:, mouseDown:, mouseDragged:) and two ivars
/// (editor_ptr, band_idx). `tag` is used as the control index, set via
/// the standard NSControl `setTag:`-like trick (we store on NSView via
/// a custom getter/setter? — actually NSView doesn't have a tag, so we
/// add one too).
pub(crate) unsafe fn knob_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveKnob".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsview = class(c"NSView");
    let cls = unsafe { objc_allocateClassPair(nsview, c"StaveKnob".as_ptr(), 0) };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"band_idx".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"accent_mode".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"ci_tag".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    // drawRect: takes a CGRect (NSRect)
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"drawRect:"),
            knob_draw_rect as *const c_void,
            c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
        )
    };
    let v_at_sel_at = c"v@:@";
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDown:"),
            knob_mouse_down as *const c_void,
            v_at_sel_at.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDragged:"),
            knob_mouse_dragged as *const c_void,
            v_at_sel_at.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseUp:"),
            knob_mouse_up as *const c_void,
            v_at_sel_at.as_ptr() as *const c_char,
        )
    };
    // Custom tag getter using the ci_tag ivar.
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"acceptsFirstMouse:"),
            accepts_first_mouse_yes as *const c_void,
            c"B@:@".as_ptr(),
        )
    };
    unsafe extern "C" fn knob_tag(this: Id, _cmd: Sel) -> i64 {
        let cls = unsafe { objc_lookUpClass(c"StaveKnob".as_ptr()) };
        let ivar = unsafe { class_getInstanceVariable(cls, c"ci_tag".as_ptr()) };
        if ivar.is_null() {
            return -1;
        }
        unsafe { object_getIvar(this, ivar) as i64 }
    }
    unsafe extern "C" fn knob_set_tag(this: Id, _cmd: Sel, value: i64) {
        let cls = unsafe { objc_lookUpClass(c"StaveKnob".as_ptr()) };
        let ivar = unsafe { class_getInstanceVariable(cls, c"ci_tag".as_ptr()) };
        if ivar.is_null() {
            return;
        }
        unsafe { object_setIvar(this, ivar, value as Id) };
    }
    let _ =
        unsafe { class_addMethod(cls, sel(c"tag"), knob_tag as *const c_void, c"q@:".as_ptr()) };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"setTag:"),
            knob_set_tag as *const c_void,
            c"v@:q".as_ptr(),
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}
