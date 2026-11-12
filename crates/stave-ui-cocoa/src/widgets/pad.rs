use core::ffi::c_void;

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;

use stave_core::DebugBus;

use crate::theme;

use crate::*;

// ============================================================================
// StavePad — interactive X/Y operating-point pad with a LIVE dot
//
// The user drags a rest position (X = one control, Y = another; both written
// as normalized values, so knobs bound to the same controls stay in sync).
// A second, read-only dot is driven by probes the PLUGIN publishes:
//   `pad_live_x`  — live X, in the pad's 0..1 domain (clamped at the edges,
//                   so dynamics pushing past the static range pin visibly)
//   `pad_live_g`  — 0..1 glow (e.g. how hard the dynamics are working)
// The live dot shares the rest Y (only X moves dynamically today). All
// domain mapping lives in the plugin; the editor draws what it's told.
// Redrawn every timer tick via the meters list.
// ============================================================================

/// Read a probe's latest value by name (lazy per-draw resolution, so the
/// pad works even when the editor attaches before audio has published the
/// probes — it simply starts animating once they appear).
pub(crate) unsafe fn probe_value_by_name(bus: &Arc<DebugBus>, name: &str) -> Option<f64> {
    let pi = find_probe(bus, name)?;
    Some(f64::from_bits(
        bus.probes[pi]
            .latest
            .load(core::sync::atomic::Ordering::Relaxed),
    ))
}

/// Linear-interpolated sample of the bus's published transfer curve at
/// normalized position t ∈ [0, 1] across its fixed ±CURVE_X_SPAN domain.
pub(crate) fn curve_sample(bus: &Arc<DebugBus>, n: usize, t: f64) -> f64 {
    let pos = t.clamp(0.0, 1.0) * (n - 1) as f64;
    let i = pos as usize;
    let frac = pos - i as f64;
    let a = f32::from_bits(bus.curve[i].load(core::sync::atomic::Ordering::Relaxed)) as f64;
    let b =
        f32::from_bits(bus.curve[(i + 1).min(n - 1)].load(core::sync::atomic::Ordering::Relaxed))
            as f64;
    a + (b - a) * frac
}

pub(crate) unsafe extern "C" fn pad_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let cls = unsafe { objc_lookUpClass(c"StavePad".as_ptr()) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    let cx_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_x".as_ptr()) };
    let cy_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_y".as_ptr()) };
    if ed_ivar.is_null() || cx_ivar.is_null() || cy_ivar.is_null() {
        return;
    }
    let editor_ptr = unsafe { object_getIvar(this, ed_ivar) } as *const PluginEditorView;
    if editor_ptr.is_null() {
        return;
    }
    let editor = unsafe { &*editor_ptr };
    let cx = unsafe { object_getIvar(this, cx_ivar) } as i64;
    let cy = unsafe { object_getIvar(this, cy_ivar) } as i64;
    if cx < 0 || cy < 0 {
        return;
    }
    let (cx, cy) = (cx as usize, cy as usize);
    if cx >= editor.param_count() || cy >= editor.param_count() {
        return;
    }

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

    let x0 = bounds.origin.x;
    let y0 = bounds.origin.y;
    let w = bounds.size.width;
    let h = bounds.size.height;
    unsafe {
        // Recessed slate panel (same family as the meter track).
        CGContextSetRGBFillColor(ctx, 0.115, 0.135, 0.175, 0.92);
        CGContextFillRect(ctx, bounds);
        // Inset depth: dark inner shadow along the visual TOP edge (y-up:
        // top = y0+h) and a light seam along the bottom lip — the screen
        // reads as carved INTO the card, opposite of the raised knobs.
        for (k, a) in [(0.0f64, 0.16f64), (1.0, 0.11), (2.0, 0.07), (3.0, 0.035)] {
            CGContextSetRGBFillColor(ctx, 0.0, 0.0, 0.0, a);
            CGContextFillRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: x0,
                        y: y0 + h - 3.0 * (k + 1.0),
                    },
                    size: CGSize {
                        width: w,
                        height: 3.0,
                    },
                },
            );
        }
        CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 0.045);
        CGContextFillRect(
            ctx,
            CGRect {
                origin: CGPoint { x: x0, y: y0 },
                size: CGSize {
                    width: w,
                    height: 1.0,
                },
            },
        );
        // Clamped: a mismatched/hostile bus value must degrade, not
        // panic the UI (an ABI-stale harness once published 257).
        let curve_n = (editor
            .bus
            .curve_used
            .load(core::sync::atomic::Ordering::Acquire) as usize)
            .min(stave_core::debug::CURVE_POINTS);

        let inset = 8.0;
        let nx = editor.param(cx).normalized();
        let ny = editor.param(cy).normalized();
        let glow = probe_value_by_name(&editor.bus, "pad_live_g")
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);

        if curve_n >= 2 {
            // ===== TRANSFER VIEW: the plugin's true waveshape =====
            // y autoscale with headroom so the curve fills the screen.
            let mut ymax = 1.0f64;
            for i in 0..curve_n {
                ymax =
                    ymax.max(
                        (f32::from_bits(
                            editor.bus.curve[i].load(core::sync::atomic::Ordering::Relaxed),
                        ) as f64)
                            .abs(),
                    );
            }
            ymax *= 1.12;
            let to_px = |t: f64| x0 + inset + t * (w - 2.0 * inset);
            let to_py = |y: f64| y0 + h * 0.5 + (y / ymax) * (h * 0.5 - 4.0);

            // Signal reach: where the current material lives on the curve.
            let rest_t = probe_value_by_name(&editor.bus, "pad_rest_x")
                .unwrap_or(0.5)
                .clamp(0.0, 1.0);
            if let Some(reach) = probe_value_by_name(&editor.bus, "curve_reach") {
                let r = reach.clamp(0.0, 1.0);
                if r > 0.002 {
                    let xa = to_px((rest_t - r).max(0.0));
                    let xb = to_px((rest_t + r).min(1.0));
                    CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 0.045);
                    CGContextFillRect(
                        ctx,
                        CGRect {
                            origin: CGPoint { x: xa, y: y0 + 2.0 },
                            size: CGSize {
                                width: (xb - xa).max(0.0),
                                height: h - 4.0,
                            },
                        },
                    );
                }
            }
            // Axes: zero-input vertical, zero-output horizontal.
            CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.07);
            CGContextSetLineWidth(ctx, 1.0);
            CGContextBeginPath(ctx);
            CGContextMoveToPoint(ctx, to_px(0.5), y0 + 2.0);
            CGContextAddLineToPoint(ctx, to_px(0.5), y0 + h - 2.0);
            CGContextStrokePath(ctx);
            CGContextBeginPath(ctx);
            CGContextMoveToPoint(ctx, x0 + 2.0, to_py(0.0));
            CGContextAddLineToPoint(ctx, x0 + w - 2.0, to_py(0.0));
            CGContextStrokePath(ctx);

            // The curve: soft glow pass, then a crisp core.
            for (lw, a) in [(4.5f64, 0.14f64), (1.8, 0.95)] {
                CGContextSetRGBStrokeColor(
                    ctx,
                    theme::ACCENT.0,
                    theme::ACCENT.1,
                    theme::ACCENT.2,
                    a,
                );
                CGContextSetLineWidth(ctx, lw);
                CGContextBeginPath(ctx);
                for i in 0..=160 {
                    let t = i as f64 / 160.0;
                    let px = to_px(t);
                    let py = to_py(curve_sample(&editor.bus, curve_n, t));
                    if i == 0 {
                        CGContextMoveToPoint(ctx, px, py);
                    } else {
                        CGContextAddLineToPoint(ctx, px, py);
                    }
                }
                CGContextStrokePath(ctx);
            }

            // ===== PHOSPHOR CLOUD (plugin-owned capture, ABI 8) =====
            // ch0 = shaper-domain input, ch1 = chain output, drawn with
            // additive blending and age-faded trails — a CRT with
            // persistence. Memoryless settings burn a thin line along the
            // static curve; hysteresis opens a loop, slew smears it,
            // memory makes the whole figure breathe and walk.
            if editor.bus.plugin_owns_capture() {
                let span = stave_core::debug::CURVE_X_SPAN;
                {
                    let mut trail = editor.scope_trail.borrow_mut();
                    let mut bin = [0f32; stave_core::debug::CAPTURE_LEN];
                    let mut bout = [0f32; stave_core::debug::CAPTURE_LEN];
                    let (n_in, blk) = editor.bus.read_capture_channel(0, &mut bin);
                    let (n_out, _) = editor.bus.read_capture_channel(1, &mut bout);
                    let n = n_in.min(n_out) as usize;
                    if n > 1 && blk != trail.last_block {
                        trail.last_block = blk;
                        let step = (n / 256).max(1);
                        let pts: Vec<(f32, f32)> =
                            (0..n).step_by(step).map(|i| (bin[i], bout[i])).collect();
                        trail.frames.push_front(pts);
                        trail.frames.truncate(12);
                    }
                }
                let trail = editor.scope_trail.borrow();
                if !trail.frames.is_empty() {
                    CGContextSaveGState(ctx);
                    CGContextClipToRect(ctx, bounds);
                    CGContextSetBlendMode(ctx, 27); // kCGBlendModePlusLighter
                    CGContextSetLineCap(ctx, 1); // round
                    let (ar, ag, ab) = (theme::ACCENT.0, theme::ACCENT.1, theme::ACCENT.2);
                    for (age, pts) in trail.frames.iter().enumerate().rev() {
                        if pts.len() < 2 {
                            continue;
                        }
                        let fade = 0.78f64.powi(age as i32);
                        // Older frames: one faint glow pass. Newest: glow +
                        // a bright beam core.
                        let passes: &[(f64, f64)] = if age == 0 {
                            &[(3.6, 0.10), (1.4, 0.40)]
                        } else {
                            &[(2.6, 0.05)]
                        };
                        for &(lw, a) in passes {
                            CGContextSetRGBStrokeColor(ctx, ar, ag, ab, a * fade);
                            CGContextSetLineWidth(ctx, lw);
                            CGContextBeginPath(ctx);
                            let mut started = false;
                            for &(vx, vy) in pts.iter() {
                                let t = ((vx as f64 / span) + 1.0) * 0.5;
                                let px = to_px(t.clamp(-0.02, 1.02));
                                let py = to_py((vy as f64).clamp(-ymax, ymax));
                                if !started {
                                    CGContextMoveToPoint(ctx, px, py);
                                    started = true;
                                } else {
                                    CGContextAddLineToPoint(ctx, px, py);
                                }
                            }
                            CGContextStrokePath(ctx);
                        }
                    }
                    CGContextRestoreGState(ctx);
                }
            }

            // Rest ring ON the curve (the bias knob's operating point).
            let rx_px = to_px(rest_t);
            let ry_px = to_py(curve_sample(&editor.bus, curve_n, rest_t));
            CGContextSetRGBStrokeColor(ctx, 0.85, 0.88, 0.93, 0.85);
            CGContextSetLineWidth(ctx, 1.5);
            let rr = 5.5;
            CGContextStrokeEllipseInRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: rx_px - rr,
                        y: ry_px - rr,
                    },
                    size: CGSize {
                        width: 2.0 * rr,
                        height: 2.0 * rr,
                    },
                },
            );
            // Live breathing dot slides ALONG the curve.
            let live_t = probe_value_by_name(&editor.bus, "pad_live_x")
                .unwrap_or(rest_t)
                .clamp(0.0, 1.0);
            let lx_px = to_px(live_t);
            let ly_px = to_py(curve_sample(&editor.bus, curve_n, live_t));
            let dr = 4.0 + 3.0 * glow;
            CGContextSetRGBFillColor(
                ctx,
                theme::ACCENT.0 + (theme::WARN.0 - theme::ACCENT.0) * glow,
                theme::ACCENT.1 + (theme::WARN.1 - theme::ACCENT.1) * glow,
                theme::ACCENT.2 + (theme::WARN.2 - theme::ACCENT.2) * glow,
                0.7 + 0.3 * glow,
            );
            CGContextFillEllipseInRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: lx_px - dr,
                        y: ly_px - dr,
                    },
                    size: CGSize {
                        width: 2.0 * dr,
                        height: 2.0 * dr,
                    },
                },
            );
        } else {
            // ===== LEGACY X/Y PAD (no published curve) =====
            CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.05);
            CGContextSetLineWidth(ctx, 1.0);
            for i in 1..4 {
                let gx = x0 + w * i as f64 / 4.0;
                CGContextBeginPath(ctx);
                CGContextMoveToPoint(ctx, gx, y0);
                CGContextAddLineToPoint(ctx, gx, y0 + h);
                CGContextStrokePath(ctx);
                let gy = y0 + h * i as f64 / 4.0;
                CGContextBeginPath(ctx);
                CGContextMoveToPoint(ctx, x0, gy);
                CGContextAddLineToPoint(ctx, x0 + w, gy);
                CGContextStrokePath(ctx);
            }
            let px = x0 + inset + nx * (w - 2.0 * inset);
            let py = y0 + inset + ny * (h - 2.0 * inset);
            let live_nx = probe_value_by_name(&editor.bus, "pad_live_x")
                .unwrap_or(nx)
                .clamp(0.0, 1.0);
            let lx = x0 + inset + live_nx * (w - 2.0 * inset);
            if (lx - px).abs() > 0.5 {
                CGContextSetRGBStrokeColor(
                    ctx,
                    theme::ACCENT.0,
                    theme::ACCENT.1,
                    theme::ACCENT.2,
                    0.35,
                );
                CGContextSetLineWidth(ctx, 2.0);
                CGContextBeginPath(ctx);
                CGContextMoveToPoint(ctx, px, py);
                CGContextAddLineToPoint(ctx, lx, py);
                CGContextStrokePath(ctx);
            }
            CGContextSetRGBStrokeColor(ctx, 0.85, 0.88, 0.93, 0.85);
            CGContextSetLineWidth(ctx, 1.5);
            let rr = 6.0;
            CGContextStrokeEllipseInRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: px - rr,
                        y: py - rr,
                    },
                    size: CGSize {
                        width: 2.0 * rr,
                        height: 2.0 * rr,
                    },
                },
            );
            let dr = 4.0 + 3.0 * glow;
            CGContextSetRGBFillColor(
                ctx,
                theme::ACCENT.0 + (theme::WARN.0 - theme::ACCENT.0) * glow,
                theme::ACCENT.1 + (theme::WARN.1 - theme::ACCENT.1) * glow,
                theme::ACCENT.2 + (theme::WARN.2 - theme::ACCENT.2) * glow,
                0.65 + 0.35 * glow,
            );
            CGContextFillEllipseInRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: lx - dr,
                        y: py - dr,
                    },
                    size: CGSize {
                        width: 2.0 * dr,
                        height: 2.0 * dr,
                    },
                },
            );
        }
    }
}

/// Shared mouse handler: absolute positioning — the click point becomes
/// the rest position (both controls written as normalized values).
pub(crate) unsafe fn pad_apply_mouse(this: Id, event: Id) {
    let cls = unsafe { objc_lookUpClass(c"StavePad".as_ptr()) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    let cx_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_x".as_ptr()) };
    let cy_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_y".as_ptr()) };
    if ed_ivar.is_null() || cx_ivar.is_null() || cy_ivar.is_null() {
        return;
    }
    let editor_ptr = unsafe { object_getIvar(this, ed_ivar) } as *mut PluginEditorView;
    if editor_ptr.is_null() {
        return;
    }
    let editor = unsafe { &mut *editor_ptr };
    let cx = unsafe { object_getIvar(this, cx_ivar) } as i64;
    let cy = unsafe { object_getIvar(this, cy_ivar) } as i64;
    if cx < 0 || cy < 0 {
        return;
    }
    let (cx, cy) = (cx as usize, cy as usize);
    if cx >= editor.param_count() || cy >= editor.param_count() {
        return;
    }
    let loc: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"locationInWindow"))
    };
    let p: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(
            this,
            sel(c"convertPoint:fromView:"),
            loc,
            core::ptr::null_mut(),
        )
    };
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let inset = 8.0;
    let w = (bounds.size.width - 2.0 * inset).max(1.0);
    let h = (bounds.size.height - 2.0 * inset).max(1.0);
    let nx = ((p.x - bounds.origin.x - inset) / w).clamp(0.0, 1.0);
    let ny = ((p.y - bounds.origin.y - inset) / h).clamp(0.0, 1.0);
    let px = editor.param(cx).normalized_to_plain(nx.clamp(0.0, 1.0));
    let py = editor.param(cy).normalized_to_plain(ny.clamp(0.0, 1.0));
    // The pad registers no mouseUp, so each apply is bracketed; undo
    // granularity is per-move until a mouseUp handler lands.
    editor.write_control_plain(cx, px, true);
    editor.write_control_plain(cy, py, true);
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, bool) =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"setNeedsDisplay:"), true);
    }
}

pub(crate) unsafe extern "C" fn pad_mouse_down(this: Id, _cmd: Sel, event: Id) {
    unsafe { pad_apply_mouse(this, event) };
}
pub(crate) unsafe extern "C" fn pad_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    unsafe { pad_apply_mouse(this, event) };
}

/// Lazily create the StavePad NSView subclass.
pub(crate) unsafe fn pad_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StavePad".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsview = class(c"NSView");
    let cls = unsafe { objc_allocateClassPair(nsview, c"StavePad".as_ptr(), 0) };
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
            c"ctrl_x".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"ctrl_y".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let rect_ty = c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr();
    let ev_ty = c"v@:@".as_ptr();
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"drawRect:"),
            pad_draw_rect as *const c_void,
            rect_ty,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDown:"),
            pad_mouse_down as *const c_void,
            ev_ty,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDragged:"),
            pad_mouse_dragged as *const c_void,
            ev_ty,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"acceptsFirstMouse:"),
            accepts_first_mouse_yes as *const c_void,
            c"B@:@".as_ptr(),
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}

// Step 7.2 pure move from view.rs.
use crate::view::PluginEditorView;

pub(crate) unsafe fn add_pad(
    v: &mut PluginEditorView,
    parent: Id,
    ctrl_x: usize,
    ctrl_y: usize,
    rect: CGRect,
) {
    let cls = unsafe { pad_view_class() };
    let raw = unsafe { msg_id(cls, sel(c"alloc")) };
    let view = unsafe { msg_id_rect(raw, sel(c"initWithFrame:"), rect) };
    unsafe { msg_void_int(view, sel(c"setWantsLayer:"), 1) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    if !ed_ivar.is_null() {
        unsafe { object_setIvar(view, ed_ivar, v.self_ptr as Id) };
    }
    let cx_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_x".as_ptr()) };
    if !cx_ivar.is_null() {
        unsafe { object_setIvar(view, cx_ivar, ctrl_x as i64 as Id) };
    }
    let cy_ivar = unsafe { class_getInstanceVariable(cls, c"ctrl_y".as_ptr()) };
    if !cy_ivar.is_null() {
        unsafe { object_setIvar(view, cy_ivar, ctrl_y as i64 as Id) };
    }
    unsafe { msg_void_id(parent, sel(c"addSubview:"), view) };
    v.meters.push((0, view));
}
