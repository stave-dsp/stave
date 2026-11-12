//! Text on macOS: the Scene IR's [`Font`] resolved onto the system faces
//! (SF Pro, SF Mono, SF Pro Rounded, New York) through AppKit, set with
//! CoreText. Fonts are created once per (family, weight, size) and kept
//! for the life of the process; a face uses a handful.

use core::ffi::c_void;
use std::cell::RefCell;
use std::collections::HashMap;

use stave_ui::scene::{Family, Font, TextAlign};
use stave_ui::style::Color;

use crate::*;

#[repr(C)]
#[derive(Copy, Clone)]
struct CGAffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    fn CTLineCreateWithAttributedString(s: *const c_void) -> *const c_void;
    fn CTLineDraw(line: *const c_void, ctx: *mut c_void);
    fn CTLineGetTypographicBounds(line: *const c_void, ascent: *mut f64, descent: *mut f64, leading: *mut f64) -> f64;
    static kCTFontAttributeName: *const c_void;
    static kCTForegroundColorAttributeName: *const c_void;
    static kCTKernAttributeName: *const c_void;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSFontDescriptorSystemDesignRounded: Id;
    static NSFontDescriptorSystemDesignSerif: Id;
}

extern "C" {
    fn CFStringCreateWithBytes(alloc: *const c_void, bytes: *const u8, len: isize, encoding: u32, external: bool) -> *const c_void;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> *const c_void;
    fn CFAttributedStringCreate(alloc: *const c_void, s: *const c_void, attributes: *const c_void) -> *const c_void;
    fn CFNumberCreate(alloc: *const c_void, the_type: isize, value: *const c_void) -> *const c_void;
    fn CFRetain(v: *const c_void) -> *const c_void;
    fn CFRelease(v: *const c_void);
    fn CGColorCreateGenericRGB(r: f64, g: f64, b: f64, a: f64) -> *const c_void;
    fn CGContextSetTextMatrix(ctx: *mut c_void, t: CGAffineTransform);
    fn CGContextSetTextPosition(ctx: *mut c_void, x: f64, y: f64);
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_FLOAT64: isize = 6;

/// CSS weight (100–900) → AppKit's NSFontWeight.
fn ns_weight(w: u16) -> f64 {
    const STOPS: [(f64, f64); 9] = [
        (100.0, -0.80),
        (200.0, -0.60),
        (300.0, -0.40),
        (400.0, 0.00),
        (500.0, 0.23),
        (600.0, 0.30),
        (700.0, 0.40),
        (800.0, 0.56),
        (900.0, 0.62),
    ];
    let w = f64::from(w).clamp(100.0, 900.0);
    for pair in STOPS.windows(2) {
        let ((w0, n0), (w1, n1)) = (pair[0], pair[1]);
        if w <= w1 {
            return n0 + (n1 - n0) * (w - w0) / (w1 - w0);
        }
    }
    0.62
}

thread_local! {
    static FONTS: RefCell<HashMap<(Family, u16, u64), usize>> = RefCell::new(HashMap::new());
}

/// The CTFont (toll-free bridged NSFont) for `font` at `size` points,
/// retained for the process lifetime.
unsafe fn ct_font(font: &Font, size: f64) -> *const c_void {
    let key = (font.family, font.weight, size.to_bits());
    if let Some(f) = FONTS.with(|m| m.borrow().get(&key).copied()) {
        return f as *const c_void;
    }
    let weight = ns_weight(font.weight);
    unsafe {
        let ns_font = class(c"NSFont");
        let sys: unsafe extern "C" fn(Class, Sel, f64, f64) -> Id = core::mem::transmute(objc_msgSend as *const ());
        let made: Id = match font.family {
            Family::Sans => sys(ns_font, sel(c"systemFontOfSize:weight:"), size, weight),
            Family::Mono => sys(ns_font, sel(c"monospacedSystemFontOfSize:weight:"), size, weight),
            Family::Rounded | Family::Serif => {
                let base = sys(ns_font, sel(c"systemFontOfSize:weight:"), size, weight);
                let desc = msg_id(base, sel(c"fontDescriptor"));
                let design = if font.family == Family::Rounded {
                    NSFontDescriptorSystemDesignRounded
                } else {
                    NSFontDescriptorSystemDesignSerif
                };
                let with: unsafe extern "C" fn(Id, Sel, Id) -> Id = core::mem::transmute(objc_msgSend as *const ());
                let designed = with(desc, sel(c"fontDescriptorWithDesign:"), design);
                if designed.is_null() {
                    base
                } else {
                    let make: unsafe extern "C" fn(Class, Sel, Id, f64) -> Id = core::mem::transmute(objc_msgSend as *const ());
                    let f = make(ns_font, sel(c"fontWithDescriptor:size:"), designed, size);
                    if f.is_null() { base } else { f }
                }
            }
        };
        let kept = CFRetain(made.cast_const());
        FONTS.with(|m| m.borrow_mut().insert(key, kept as usize));
        kept
    }
}

/// A CTLine for `text`, its typographic width, and the CF objects to
/// release once drawn.
unsafe fn line(text: &str, size: f64, color: &Color, font: &Font) -> Option<(*const c_void, f64, [*const c_void; 5])> {
    unsafe {
        let cf = CFStringCreateWithBytes(core::ptr::null(), text.as_ptr(), text.len() as isize, UTF8, false);
        if cf.is_null() {
            return None;
        }
        let kern = font.tracking * size;
        let kern_num = CFNumberCreate(core::ptr::null(), CF_NUMBER_FLOAT64, (&kern as *const f64).cast());
        let cg_color = CGColorCreateGenericRGB(color.r, color.g, color.b, color.a);
        let keys = [kCTFontAttributeName, kCTForegroundColorAttributeName, kCTKernAttributeName];
        let values = [ct_font(font, size), cg_color, kern_num];
        let attrs = CFDictionaryCreate(
            core::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            3,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let styled = CFAttributedStringCreate(core::ptr::null(), cf, attrs);
        let line = CTLineCreateWithAttributedString(styled);
        let mut width = CTLineGetTypographicBounds(line, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        // Tracking adds space after the last letter too; it is not ink.
        if !text.is_empty() {
            width -= kern;
        }
        Some((line, width, [styled, attrs, kern_num, cg_color, cf]))
    }
}

/// The typographic width of one line, in points.
pub fn measure(text: &str, size: f64, font: &Font) -> f64 {
    unsafe {
        let Some((l, w, rest)) = line(text, size, &Color::rgba(0.0, 0.0, 0.0, 1.0), font) else {
            return 0.0;
        };
        CFRelease(l);
        for v in rest {
            if !v.is_null() {
                CFRelease(v);
            }
        }
        w
    }
}

/// One line into a y-UP CG context: baseline at `y`, aligned on `x`.
pub(crate) unsafe fn draw(ctx: *mut c_void, x: f64, y: f64, text: &str, size: f64, color: &Color, font: &Font) {
    unsafe {
        let Some((l, width, rest)) = line(text, size, color, font) else {
            return;
        };
        let x0 = match font.align {
            TextAlign::Left => x,
            TextAlign::Center => x - width * 0.5,
            TextAlign::Right => x - width,
        };
        CGContextSetTextMatrix(ctx, CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 });
        CGContextSetTextPosition(ctx, x0, y);
        CTLineDraw(l, ctx);
        CFRelease(l);
        for v in rest {
            if !v.is_null() {
                CFRelease(v);
            }
        }
    }
}
