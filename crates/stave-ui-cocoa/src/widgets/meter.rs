use core::ffi::c_void;

extern crate alloc;

use stave_core::editor::MeterStyle;

use crate::theme;

use crate::*;

// ============================================================================
// StaveMeter — read-only vertical dot-ladder level meter
//
// An NSView that reads a probe (dBFS) from the bus and draws a ladder of
// LED-style dots filling bottom-up, coloured by a teal → accent → amber →
// red gradient over their position (the classic digital-meter read: cool
// when safe, hot near 0 dBFS). Unlit dots stay as faint pips so the
// ladder is visible at idle. No mouse. The editor's frame uses two of
// these for the I/O rails; the 30 Hz timer calls setNeedsDisplay so they
// animate.
// ============================================================================

/// Dot colour at ladder position `p` (0 = bottom, 1 = top): piecewise
/// lerp through teal → ACCENT → WARN_SOFT → WARN.
pub(crate) fn meter_ramp(p: f64) -> (f64, f64, f64) {
    const TEAL: (f64, f64, f64) = (0.30, 0.85, 0.75);
    let stops: [(f64, (f64, f64, f64)); 4] = [
        (0.0, TEAL),
        (0.55, (theme::ACCENT.0, theme::ACCENT.1, theme::ACCENT.2)),
        (
            0.82,
            (theme::WARN_SOFT.0, theme::WARN_SOFT.1, theme::WARN_SOFT.2),
        ),
        (1.0, (theme::WARN.0, theme::WARN.1, theme::WARN.2)),
    ];
    let p = p.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (p0, c0) = w[0];
        let (p1, c1) = w[1];
        if p <= p1 {
            let t = if p1 > p0 { (p - p0) / (p1 - p0) } else { 0.0 };
            return (
                c0.0 + (c1.0 - c0.0) * t,
                c0.1 + (c1.1 - c0.1) * t,
                c0.2 + (c1.2 - c0.2) * t,
            );
        }
    }
    (theme::WARN.0, theme::WARN.1, theme::WARN.2)
}

pub(crate) unsafe extern "C" fn meter_draw_rect(this: Id, _cmd: Sel, _dirty: CGRect) {
    let bounds: CGRect = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(this, sel(c"bounds"))
    };
    let cls = unsafe { objc_lookUpClass(c"StaveMeter".as_ptr()) };
    let ed_ivar = unsafe { class_getInstanceVariable(cls, c"editor_ptr".as_ptr()) };
    let pi_ivar = unsafe { class_getInstanceVariable(cls, c"probe_idx".as_ptr()) };
    let style_ivar = unsafe { class_getInstanceVariable(cls, c"meter_style".as_ptr()) };
    if ed_ivar.is_null() || pi_ivar.is_null() || style_ivar.is_null() {
        return;
    }
    let editor_ptr = unsafe { object_getIvar(this, ed_ivar) } as *const PluginEditorView;
    if editor_ptr.is_null() {
        return;
    }
    let editor = unsafe { &*editor_ptr };
    let pi = unsafe { object_getIvar(this, pi_ivar) } as i64;
    let style = match unsafe { object_getIvar(this, style_ivar) } as i64 {
        1 => MeterStyle::Segments,
        _ => MeterStyle::Dots,
    };
    let mut db = -120.0f64;
    if pi >= 0 {
        let used = editor
            .bus
            .probes_used
            .load(core::sync::atomic::Ordering::Acquire) as usize;
        if (pi as usize) < used {
            db = f64::from_bits(
                editor.bus.probes[pi as usize]
                    .latest
                    .load(core::sync::atomic::Ordering::Relaxed),
            );
        }
    }
    // -60..0 dBFS → 0..1.
    let norm = ((db + 60.0) / 60.0).clamp(0.0, 1.0);

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

    let bw = bounds.size.width;
    let bx = bounds.origin.x;
    let by = bounds.origin.y + 1.0;
    let bh = bounds.size.height - 2.0;
    unsafe {
        match style {
            MeterStyle::Dots => {
                // Dot ladder, bottom-up (y-up coordinates). Diameter from the
                // rail width; gap tuned so the pips read as discrete LEDs.
                let diameter = bw.min(9.0);
                let gap = (diameter * 0.55).max(2.0);
                let step = diameter + gap;
                let count = (((bh + gap) / step).floor() as usize).max(4);
                let total = count as f64 * step - gap;
                let y_start = by + (bh - total) * 0.5;
                let x = bx + (bw - diameter) * 0.5;
                let lit_count = (norm * count as f64).round() as usize;
                for index in 0..count {
                    let y = y_start + index as f64 * step;
                    let rect = CGRect {
                        origin: CGPoint { x, y },
                        size: CGSize {
                            width: diameter,
                            height: diameter,
                        },
                    };
                    if index < lit_count {
                        let position = if count > 1 {
                            index as f64 / (count - 1) as f64
                        } else {
                            0.0
                        };
                        let (red, green, blue) = meter_ramp(position);
                        let head = index + 1 == lit_count;
                        CGContextSetRGBFillColor(
                            ctx,
                            red,
                            green,
                            blue,
                            if head { 1.0 } else { 0.88 },
                        );
                        CGContextFillEllipseInRect(ctx, rect);
                    } else {
                        CGContextSetRGBFillColor(ctx, 0.75, 0.80, 0.90, 0.10);
                        CGContextFillEllipseInRect(ctx, rect);
                    }
                }
            }
            MeterStyle::Segments => {
                // A dense ladder of horizontal strokes. The meter remains
                // readable at four logical pixels wide, making two channels
                // cheaper than one dot rail in compact shell chrome.
                let segment_height = 1.5;
                let gap = 2.25;
                let step = segment_height + gap;
                let count = (((bh + gap) / step).floor() as usize).max(6);
                let total = count as f64 * step - gap;
                let y_start = by + (bh - total) * 0.5;
                let segment_width = bw.min(5.0);
                let x = bx + (bw - segment_width) * 0.5;
                let lit_count = (norm * count as f64).round() as usize;
                for index in 0..count {
                    let rect = CGRect {
                        origin: CGPoint {
                            x,
                            y: y_start + index as f64 * step,
                        },
                        size: CGSize {
                            width: segment_width,
                            height: segment_height,
                        },
                    };
                    if index < lit_count {
                        let position = if count > 1 {
                            index as f64 / (count - 1) as f64
                        } else {
                            0.0
                        };
                        let (red, green, blue) = meter_ramp(position);
                        let head = index + 1 == lit_count;
                        CGContextSetRGBFillColor(
                            ctx,
                            red,
                            green,
                            blue,
                            if head { 1.0 } else { 0.88 },
                        );
                    } else {
                        CGContextSetRGBFillColor(ctx, 0.75, 0.80, 0.90, 0.10);
                    }
                    CGContextFillRect(ctx, rect);
                }
            }
        }
    }
}

/// Lazily create the StaveMeter NSView subclass (drawRect: + editor_ptr,
/// probe_idx ivars). Read-only, so no mouse methods.
pub(crate) unsafe fn meter_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveMeter".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsview = class(c"NSView");
    let cls = unsafe { objc_allocateClassPair(nsview, c"StaveMeter".as_ptr(), 0) };
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
            c"probe_idx".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"meter_style".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"drawRect:"),
            meter_draw_rect as *const c_void,
            c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}
