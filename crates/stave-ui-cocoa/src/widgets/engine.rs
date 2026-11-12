use core::ffi::{c_char, c_void};

extern crate alloc;
use alloc::vec::Vec;

use crate::theme;

use crate::*;

// ============================================================================
// Engine view — shared state + the two companion views to the phosphor pad:
//   StaveHarm — H1..H8 harmonic ladder computed from the published curve
//               at the plugin's `viz_amp` reference amplitude (odd = teal,
//               even = amber, decaying max-hold ghosts).
//   StaveTopo — live topology strip built from `tapN:LABEL` probes: one
//               pill per stage the plugin probed THIS block, glowing with
//               its tap level. Stages the current STACK bypasses simply
//               drop out of the strip.
// ============================================================================

pub(crate) struct ScopeTrail {
    pub last_block: u64,
    pub frames: alloc::collections::VecDeque<Vec<(f32, f32)>>,
}

/// Recessed slate panel — the shared backdrop of the engine screens
/// (same carved-in read as the transfer pad).
pub(crate) unsafe fn engine_panel_bg(ctx: *mut c_void, bounds: CGRect) {
    unsafe {
        let (x0, y0, w, h) = (
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        );
        CGContextSetRGBFillColor(ctx, 0.115, 0.135, 0.175, 0.92);
        CGContextFillRect(ctx, bounds);
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
    }
}

/// Rounded-rect path (assumes r ≤ min(w, h)/2).
pub(crate) unsafe fn engine_rounded_path(ctx: *mut c_void, rect: CGRect, r: f64) {
    unsafe {
        let (x, y, w, h) = (
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        );
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, x + r, y);
        CGContextAddArcToPoint(ctx, x + w, y, x + w, y + h, r);
        CGContextAddArcToPoint(ctx, x + w, y + h, x, y + h, r);
        CGContextAddArcToPoint(ctx, x, y + h, x, y, r);
        CGContextAddArcToPoint(ctx, x, y, x + w, y, r);
        CGContextClosePath(ctx);
    }
}

/// Draw `text` centered on (cx, cy) with the given font/color/kern —
/// CoreGraphics-side text for the custom views (autorelease pool is the
/// enclosing drawRect's).
pub(crate) unsafe fn engine_text_centered(
    text: &str,
    cx: f64,
    cy: f64,
    font: Id,
    color: Id,
    kern: f64,
) {
    unsafe {
        let nsstr = ns_string(text);
        let kern_num = {
            let cls = class(c"NSNumber");
            let f: unsafe extern "C" fn(Id, Sel, f64) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            f(cls, sel(c"numberWithDouble:"), kern)
        };
        let objs: [Id; 3] = [font, color, kern_num];
        let keys: [Id; 3] = [
            ns_string("NSFont"),
            ns_string("NSColor"),
            ns_string("NSKern"),
        ];
        let dict = {
            let f: unsafe extern "C" fn(Id, Sel, *const Id, *const Id, usize) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            f(
                class(c"NSDictionary"),
                sel(c"dictionaryWithObjects:forKeys:count:"),
                objs.as_ptr(),
                keys.as_ptr(),
                3,
            )
        };
        let astr = {
            let raw = msg_id(class(c"NSAttributedString"), sel(c"alloc"));
            let f: unsafe extern "C" fn(Id, Sel, Id, Id) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            f(raw, sel(c"initWithString:attributes:"), nsstr, dict)
        };
        if astr.is_null() {
            return;
        }
        let size: CGSize = {
            let f: unsafe extern "C" fn(Id, Sel) -> CGSize =
                core::mem::transmute(objc_msgSend as *const ());
            f(astr, sel(c"size"))
        };
        let f: unsafe extern "C" fn(Id, Sel, CGPoint) =
            core::mem::transmute(objc_msgSend as *const ());
        f(
            astr,
            sel(c"drawAtPoint:"),
            CGPoint {
                x: cx - size.width * 0.5,
                y: cy - size.height * 0.5,
            },
        );
    }
}

pub(crate) unsafe fn engine_editor_of(
    this: Id,
    cls_name: &core::ffi::CStr,
) -> Option<&'static PluginEditorView> {
    unsafe {
        let cls = objc_lookUpClass(cls_name.as_ptr() as *const c_char);
        let ed_ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
        if ed_ivar.is_null() {
            return None;
        }
        let editor_ptr = object_getIvar(this, ed_ivar) as *const PluginEditorView;
        if editor_ptr.is_null() {
            None
        } else {
            Some(&*editor_ptr)
        }
    }
}

pub(crate) unsafe fn engine_cgctx() -> *mut c_void {
    unsafe {
        let cur: Id = msg_id(class(c"NSGraphicsContext"), sel(c"currentContext"));
        if cur.is_null() {
            return core::ptr::null_mut();
        }
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        f(cur, sel(c"CGContext"))
    }
}

// ---- StaveHarm -------------------------------------------------------------

pub(crate) unsafe extern "C" fn harm_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    unsafe {
        let bounds: CGRect = {
            let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
                core::mem::transmute(objc_msgSend as *const ());
            f(this, sel(c"bounds"))
        };
        let Some(editor) = engine_editor_of(this, c"StaveHarm") else {
            return;
        };
        let ctx = engine_cgctx();
        if ctx.is_null() {
            return;
        }
        engine_panel_bg(ctx, bounds);
        let curve_n = editor
            .bus
            .curve_used
            .load(core::sync::atomic::Ordering::Acquire) as usize;
        if curve_n < 2 {
            return;
        }
        let span = stave_core::debug::CURVE_X_SPAN;
        let amp = probe_value_by_name(&editor.bus, "viz_amp")
            .unwrap_or(1.0)
            .clamp(0.05, span);

        // One cycle through the published curve → coherent Goertzel H1..H8.
        const N: usize = 256;
        let mut y = [0f64; N];
        let mut mean = 0.0f64;
        for (i, yi) in y.iter_mut().enumerate() {
            let x = amp * (2.0 * core::f64::consts::PI * i as f64 / N as f64).sin();
            *yi = curve_sample(&editor.bus, curve_n, (x / span + 1.0) * 0.5);
            mean += *yi;
        }
        mean /= N as f64;
        let mut mags = [0f64; 8];
        for (k, m) in mags.iter_mut().enumerate() {
            let w = 2.0 * core::f64::consts::PI * (k + 1) as f64 / N as f64;
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for &yi in y.iter() {
                let s0 = (yi - mean) + 2.0 * w.cos() * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            *m = ((s1 - s2 * w.cos()).powi(2) + (s2 * w.sin()).powi(2)).sqrt() / (N as f64 * 0.5);
        }
        let h1 = mags[0].max(1e-9);

        let (x0, y0, w, h) = (
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        );
        let inset = 12.0;
        let base = y0 + 16.0;
        let top = y0 + h - 10.0;
        let floor_db = -48.0f64;
        let slot = (w - 2.0 * inset) / 8.0;
        let bw = slot * 0.54;
        let mut ghost = editor.harm_ghost.borrow_mut();

        // Baseline.
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.08);
        CGContextSetLineWidth(ctx, 1.0);
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, x0 + inset - 2.0, base);
        CGContextAddLineToPoint(ctx, x0 + w - inset + 2.0, base);
        CGContextStrokePath(ctx);

        let mono = ns_font_role_mono((8.0, theme::W_MEDIUM));
        let ink = ns_color_token(theme::INK_MUTED);
        for k in 0..8 {
            let db = (20.0 * (mags[k] / h1).max(1e-9).log10()).max(floor_db);
            let norm = ((db - floor_db) / -floor_db).clamp(0.0, 1.0);
            // Ghost: decaying max-hold tick above the live bar.
            ghost[k] = (ghost[k] * 0.94).max(norm);
            let bx = x0 + inset + k as f64 * slot + (slot - bw) * 0.5;
            let bh = (norm * (top - base)).max(1.5);
            // Odd harmonics teal, evens amber — the even/odd balance reads
            // at a glance (H1 = the reference, always full).
            let c: theme::Rgba = if (k + 1) % 2 == 1 {
                theme::ACCENT
            } else {
                theme::WARN_SOFT
            };
            CGContextSetRGBFillColor(ctx, c.0, c.1, c.2, if k == 0 { 0.55 } else { 0.85 });
            CGContextFillRect(
                ctx,
                CGRect {
                    origin: CGPoint { x: bx, y: base },
                    size: CGSize {
                        width: bw,
                        height: bh,
                    },
                },
            );
            // Bright cap.
            CGContextSetRGBFillColor(ctx, c.0, c.1, c.2, 1.0);
            CGContextFillRect(
                ctx,
                CGRect {
                    origin: CGPoint {
                        x: bx,
                        y: base + bh - 1.5,
                    },
                    size: CGSize {
                        width: bw,
                        height: 1.5,
                    },
                },
            );
            // Ghost tick.
            let gy = base + ghost[k] * (top - base);
            if ghost[k] > norm + 0.01 {
                CGContextSetRGBFillColor(ctx, c.0, c.1, c.2, 0.35);
                CGContextFillRect(
                    ctx,
                    CGRect {
                        origin: CGPoint { x: bx, y: gy },
                        size: CGSize {
                            width: bw,
                            height: 1.0,
                        },
                    },
                );
            }
            // Harmonic number under the bar.
            engine_text_centered(
                &alloc::format!("{}", k + 1),
                bx + bw * 0.5,
                y0 + 8.0,
                mono,
                ink,
                0.0,
            );
        }
    }
}

pub(crate) unsafe fn harm_view_class() -> Class {
    unsafe {
        let existing = objc_lookUpClass(c"StaveHarm".as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let nsview = class(c"NSView");
        let cls = objc_allocateClassPair(nsview, c"StaveHarm".as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let rect_ty = c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr();
        let _ = class_addMethod(
            cls,
            sel(c"drawRect:"),
            harm_draw_rect as *const c_void,
            rect_ty,
        );
        objc_registerClassPair(cls);
        cls
    }
}

// ---- StaveTopo -------------------------------------------------------------

pub(crate) unsafe extern "C" fn topo_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    unsafe {
        let bounds: CGRect = {
            let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
                core::mem::transmute(objc_msgSend as *const ());
            f(this, sel(c"bounds"))
        };
        let Some(editor) = engine_editor_of(this, c"StaveTopo") else {
            return;
        };
        let ctx = engine_cgctx();
        if ctx.is_null() {
            return;
        }
        let (x0, y0, w, h) = (
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        );

        // pills come from the DECLARATION (owned tree), levels from
        // stable tap wires, labels from the params' own step names. Aging still gates visibility (an engine
        // that stops publishing a stage — SINGLE's second nonlinearity —
        // fades its pill).
        let pills = editor.topo_pills.borrow();
        if pills.is_empty() {
            return;
        }
        let used = editor
            .bus
            .probes_used
            .load(core::sync::atomic::Ordering::Acquire) as usize;
        let ctl_of = |pid: stave_core::ParamId| editor.param_index_of(pid);
        let step_name = |pid: stave_core::ParamId| -> Option<alloc::string::String> {
            let p = editor.param(ctl_of(pid)?);
            let idx = (p.plain().round().max(0.0) as u32).min(p.step_count().saturating_sub(1));
            Some(p.step_name(idx).unwrap_or("").into())
        };
        let step_index = |pid: stave_core::ParamId| -> Option<u32> {
            let i = ctl_of(pid)?;
            let v = editor.plain(i);
            Some(v.round().max(0.0) as u32)
        };
        let resolve = |l: &stave_core::TapLabel| -> alloc::string::String {
            match l {
                stave_core::TapLabel::Fixed(t) => (*t).into(),
                stave_core::TapLabel::StepOf(pid) => step_name(*pid).unwrap_or_default(),
                stave_core::TapLabel::StepSuffixed(pid, suffix) => {
                    let mut t = step_name(*pid).unwrap_or_default();
                    t.push_str(suffix);
                    t
                }
            }
        };
        let mut items: Vec<(usize, alloc::string::String, f64, bool, u32)> = Vec::new();
        {
            let mut ages = editor.topo_age.borrow_mut();
            for (order, pill) in pills.iter().enumerate() {
                let Some(pi) = (0..used.min(stave_core::debug::MAX_PROBES))
                    .find(|&i| editor.bus.probes[i].name_str() == pill.tap)
                else {
                    continue;
                };
                let hits = editor.bus.probes[pi]
                    .hits
                    .load(core::sync::atomic::Ordering::Relaxed);
                let (last, age) = ages[pi];
                let fresh = hits != last && last != 0;
                let age_new = if hits != last {
                    0
                } else {
                    age.saturating_add(1)
                };
                ages[pi] = (hits, age_new);
                if hits == 0 || age_new > 90 {
                    continue; // never published, or stale ≈3 s
                }
                let lvl = f64::from_bits(
                    editor.bus.probes[pi]
                        .latest
                        .load(core::sync::atomic::Ordering::Relaxed),
                );
                let mut label = &pill.label;
                for (pid, step, over) in &pill.overrides {
                    if step_index(*pid) == Some(*step) {
                        label = over;
                    }
                }
                items.push((order, resolve(label), lvl, fresh, age_new));
            }
        }
        if items.is_empty() {
            return;
        }

        let cy = y0 + h * 0.5;
        let end_w = 24.0;
        let gap = 12.0;
        let m = items.len() as f64;
        let pw = ((w - 2.0 * end_w - (m + 1.0) * gap) / m).clamp(44.0, 118.0);
        let ph = (h - 8.0).min(26.0);
        let total = m * pw + (m - 1.0) * gap;
        let start = x0 + (w - total) * 0.5;

        // Route line under everything.
        CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.10);
        CGContextSetLineWidth(ctx, 1.0);
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, x0 + 2.0, cy);
        CGContextAddLineToPoint(ctx, x0 + w - 2.0, cy);
        CGContextStrokePath(ctx);

        let tick_font = ns_font_role((8.0, theme::W_HEAVY));
        engine_text_centered(
            "IN",
            start - end_w * 0.5 - gap * 0.5,
            cy,
            tick_font,
            ns_color_token(theme::INK_MUTED),
            1.0,
        );
        engine_text_centered(
            "OUT",
            start + total + end_w * 0.5 + gap * 0.5,
            cy,
            tick_font,
            ns_color_token(theme::INK_MUTED),
            1.0,
        );

        for (i, (_ord, label, lvl, fresh, _age)) in items.iter().enumerate() {
            let px = start + i as f64 * (pw + gap);
            let rect = CGRect {
                origin: CGPoint {
                    x: px,
                    y: cy - ph * 0.5,
                },
                size: CGSize {
                    width: pw,
                    height: ph,
                },
            };
            // Heat: tap level in dB → glow, warming past −6 dBFS.
            let db = 20.0 * lvl.max(1e-5).log10();
            let heat = ((db + 40.0) / 40.0).clamp(0.0, 1.25);
            let hot = ((heat - 0.85) / 0.4).clamp(0.0, 1.0);
            let a = theme::ACCENT;
            let ws = theme::WARN_SOFT;
            let (gr, gg, gb) = (
                a.0 + (ws.0 - a.0) * hot,
                a.1 + (ws.1 - a.1) * hot,
                a.2 + (ws.2 - a.2) * hot,
            );
            // Body.
            engine_rounded_path(ctx, rect, 6.0);
            CGContextSetRGBFillColor(ctx, 0.135, 0.155, 0.195, 0.96);
            CGContextFillPath(ctx);
            // Glow border (level-driven) over a faint structural border.
            engine_rounded_path(ctx, rect, 6.0);
            CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.07);
            CGContextSetLineWidth(ctx, 1.0);
            CGContextStrokePath(ctx);
            let glow_a = if *fresh {
                0.14 + 0.75 * heat.min(1.0)
            } else {
                0.10
            };
            engine_rounded_path(ctx, rect, 6.0);
            CGContextSetRGBStrokeColor(ctx, gr, gg, gb, glow_a);
            CGContextSetLineWidth(ctx, 1.4);
            CGContextStrokePath(ctx);
            // Under-glow wash inside the pill when hot.
            if heat > 0.6 && *fresh {
                engine_rounded_path(ctx, rect, 6.0);
                CGContextSetRGBFillColor(ctx, gr, gg, gb, 0.10 * (heat - 0.6) / 0.65);
                CGContextFillPath(ctx);
            }
            // Label.
            let ink = if *fresh && heat > 0.25 {
                theme::INK_HI
            } else {
                theme::INK_LO
            };
            engine_text_centered(
                label,
                px + pw * 0.5,
                cy,
                tick_font,
                ns_color_token(ink),
                0.9,
            );
            // Flow chevron toward the next pill.
            if (i as f64) < m - 1.0 {
                let mx = px + pw + gap * 0.5;
                CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 0.28);
                CGContextSetLineWidth(ctx, 1.2);
                CGContextBeginPath(ctx);
                CGContextMoveToPoint(ctx, mx - 2.0, cy + 3.0);
                CGContextAddLineToPoint(ctx, mx + 1.5, cy);
                CGContextAddLineToPoint(ctx, mx - 2.0, cy - 3.0);
                CGContextStrokePath(ctx);
            }
        }
    }
}

pub(crate) unsafe fn topo_view_class() -> Class {
    unsafe {
        let existing = objc_lookUpClass(c"StaveTopo".as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let nsview = class(c"NSView");
        let cls = objc_allocateClassPair(nsview, c"StaveTopo".as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let rect_ty = c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr();
        let _ = class_addMethod(
            cls,
            sel(c"drawRect:"),
            topo_draw_rect as *const c_void,
            rect_ty,
        );
        objc_registerClassPair(cls);
        cls
    }
}

#[inline]
pub(crate) unsafe fn ns_color(r: f64, g: f64, b: f64, a: f64) -> Id {
    let color_class = class(c"NSColor");
    let sel_init = sel(c"colorWithSRGBRed:green:blue:alpha:");
    let f: unsafe extern "C" fn(Id, Sel, f64, f64, f64, f64) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(color_class, sel_init, r, g, b, a) }
}

#[inline]
pub(crate) unsafe fn make_cg_color(r: f64, g: f64, b: f64, a: f64) -> *mut c_void {
    let ns = unsafe { ns_color(r, g, b, a) };
    if ns.is_null() {
        return core::ptr::null_mut();
    }
    let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns, sel(c"CGColor")) }
}

// ----------------------------------------------------------------------------
// Theme-aware adapters
//
// These bridge the pure-data tokens in `theme.rs` to the AppKit
// constructors. New call sites should prefer these — old hardcoded
// `ns_color(...)` calls can be migrated incrementally.
// ----------------------------------------------------------------------------

/// Convert a `theme::Rgba` token to an NSColor.
#[inline]
pub(crate) unsafe fn ns_color_token(c: theme::Rgba) -> Id {
    unsafe { ns_color(c.0, c.1, c.2, c.3) }
}

/// Build an NSFont from a `theme::role_*` (size, weight) pair.
#[inline]
pub(crate) unsafe fn ns_font_role(role: (f64, f64)) -> Id {
    unsafe { ns_font_w(role.0, role.1) }
}

/// SF Mono tabular digits sized by a theme role — for value readouts.
#[inline]
pub(crate) unsafe fn ns_font_role_mono(role: (f64, f64)) -> Id {
    unsafe { ns_font_mono_digit(role.0, role.1) }
}

#[inline]
pub(crate) unsafe fn ns_string(s: &str) -> Id {
    // Use NSString stringWithUTF8String:
    let mut bytes: Vec<u8> = Vec::with_capacity(s.len() + 1);
    bytes.extend_from_slice(s.as_bytes());
    bytes.push(0);
    let ns_string_class = class(c"NSString");
    let sel_with = sel(c"stringWithUTF8String:");
    let f: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(ns_string_class, sel_with, bytes.as_ptr() as *const c_char) }
}

// Step 7.2 pure move from view.rs.
use crate::view::PluginEditorView;

/// Harmonic-ladder view (engine trio, right of the scope). Repainted
/// by the 30 Hz timer via the meters list.
pub(crate) unsafe fn add_harmonics(
    v: &mut PluginEditorView,
    parent: Id,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    let cls = unsafe { harm_view_class() };
    let raw = unsafe { msg_id(cls, sel(c"alloc")) };
    let rect = CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: w,
            height: h,
        },
    };
    let view = unsafe { msg_id_rect(raw, sel(c"initWithFrame:"), rect) };
    unsafe { msg_void_int(view, sel(c"setWantsLayer:"), 1) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    if !ed_ivar.is_null() {
        unsafe { object_setIvar(view, ed_ivar, v.self_ptr as Id) };
    }
    unsafe { msg_void_id(parent, sel(c"addSubview:"), view) };
    v.meters.push((0, view));
}

/// Live topology strip (engine trio, under the scope/ladder row).
pub(crate) unsafe fn add_topology(
    v: &mut PluginEditorView,
    parent: Id,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    let cls = unsafe { topo_view_class() };
    let raw = unsafe { msg_id(cls, sel(c"alloc")) };
    let rect = CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: w,
            height: h,
        },
    };
    let view = unsafe { msg_id_rect(raw, sel(c"initWithFrame:"), rect) };
    unsafe { msg_void_int(view, sel(c"setWantsLayer:"), 1) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    if !ed_ivar.is_null() {
        unsafe { object_setIvar(view, ed_ivar, v.self_ptr as Id) };
    }
    unsafe { msg_void_id(parent, sel(c"addSubview:"), view) };
    v.meters.push((0, view));
}
