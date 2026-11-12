//! Text on macOS: the Scene IR's [`Font`] resolved onto the system faces
//! (SF Pro, SF Mono, SF Pro Rounded, New York) through AppKit, set with
//! CoreText. Fonts are made once per (family, weight, size) and cached;
//! a face uses a handful, and the cache starts over past [`FONT_CACHE`].

use core::ffi::c_void;
use std::cell::RefCell;
use std::collections::HashMap;

use stave_ui::scene::{Family, Font, TextAlign};
use stave_ui::style::Color;

use crate::cf::CfRef;
use crate::objc::{class, send, sel, Obj};

/// The most fonts kept at once; more (a face animating its type size)
/// empties the cache rather than growing it.
pub const FONT_CACHE: usize = 64;

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
    static NSFontDescriptorSystemDesignRounded: Option<Obj>;
    static NSFontDescriptorSystemDesignSerif: Option<Obj>;
}

#[expect(
    clippy::duplicated_attributes,
    reason = "one #[link] per framework repeats kind"
)]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
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
    static FONTS: RefCell<HashMap<(Family, u16, u64), CfRef>> = RefCell::new(HashMap::new());
}

/// The CTFont (toll-free bridged NSFont) for `font` at `size` points, as
/// a reference of the caller's own; `None` if AppKit makes none.
fn ct_font(font: &Font, size: f64) -> Option<CfRef> {
    let key = (font.family, font.weight, size.to_bits());
    let retain = |f: &CfRef| {
        // SAFETY: a live font the cache holds; the new reference is the
        // caller's.
        unsafe { CfRef::from_create(CFRetain(f.as_ptr())) }
    };
    if let Some(f) = FONTS.with(|m| m.borrow().get(&key).and_then(retain)) {
        return Some(f);
    }
    let made = ns_font(font, size)?;
    // SAFETY: `made` is a live NSFont (autoreleased); the cache keeps its
    // own reference.
    let kept = unsafe { CfRef::from_create(CFRetain(made.as_ptr())) }?;
    let mine = retain(&kept);
    FONTS.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= FONT_CACHE {
            m.clear();
        }
        m.insert(key, kept);
    });
    mine
}

/// The system NSFont for `font` at `size` (autoreleased).
fn ns_font(font: &Font, size: f64) -> Option<Obj> {
    let weight = ns_weight(font.weight);
    let ns_font = class(c"NSFont");
    // SAFETY: NSFont's class constructors and NSFontDescriptor's
    // `fontDescriptorWithDesign:` with their real signatures. Text is
    // set on the main thread (the face view draws there; the storybook's
    // screenshots run there too).
    unsafe {
        let system: Option<Obj> = send(ns_font, sel!(c"systemFontOfSize:weight:"), (size, weight));
        match font.family {
            Family::Sans => system,
            Family::Mono => send(ns_font, sel!(c"monospacedSystemFontOfSize:weight:"), (size, weight)),
            Family::Rounded | Family::Serif => {
                let base = system?;
                let design = if font.family == Family::Rounded {
                    NSFontDescriptorSystemDesignRounded
                } else {
                    NSFontDescriptorSystemDesignSerif
                };
                let desc: Option<Obj> = send(base, sel!(c"fontDescriptor"), ());
                let designed: Option<Obj> = match (desc, design) {
                    (Some(d), Some(design)) => send(d, sel!(c"fontDescriptorWithDesign:"), (design,)),
                    _ => None,
                };
                let Some(designed) = designed else {
                    return Some(base);
                };
                let made: Option<Obj> = send(ns_font, sel!(c"fontWithDescriptor:size:"), (designed, size));
                made.or(Some(base))
            }
        }
    }
}

/// A set line: the CTLine and its typographic width.
struct Line {
    line: CfRef,
    width: f64,
}

/// `text` set in `font` at `size` and `color`.
fn line(text: &str, size: f64, color: &Color, font: &Font) -> Option<Line> {
    let font_ref = ct_font(font, size)?;
    let kern = font.tracking * size;
    // SAFETY: each call is a CoreFoundation/CoreGraphics create function
    // given valid inputs (bytes and length from one `&str`; the keys,
    // values and callbacks of a three-entry CF-type dictionary); each
    // result is owned by the `CfRef` it goes into (null fails the line).
    unsafe {
        let string = CfRef::from_create(CFStringCreateWithBytes(core::ptr::null(), text.as_ptr(), text.len() as isize, UTF8, false))?;
        let kern_num = CfRef::from_create(CFNumberCreate(core::ptr::null(), CF_NUMBER_FLOAT64, (&kern as *const f64).cast()))?;
        let cg_color = CfRef::from_create(CGColorCreateGenericRGB(color.r, color.g, color.b, color.a))?;
        let keys = [kCTFontAttributeName, kCTForegroundColorAttributeName, kCTKernAttributeName];
        let values = [font_ref.as_ptr().cast_const(), cg_color.as_ptr().cast_const(), kern_num.as_ptr().cast_const()];
        let attrs = CfRef::from_create(CFDictionaryCreate(
            core::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            3,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        ))?;
        let styled = CfRef::from_create(CFAttributedStringCreate(core::ptr::null(), string.as_ptr(), attrs.as_ptr()))?;
        let line = CfRef::from_create(CTLineCreateWithAttributedString(styled.as_ptr()))?;
        let mut width = CTLineGetTypographicBounds(line.as_ptr(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        // Tracking adds space after the last letter too; it is not ink.
        if !text.is_empty() {
            width -= kern;
        }
        Some(Line { line, width })
    }
}

/// The typographic width of one line, in points.
pub fn measure(text: &str, size: f64, font: &Font) -> f64 {
    line(text, size, &Color::rgba(0.0, 0.0, 0.0, 1.0), font).map_or(0.0, |l| l.width)
}

/// One line into a y-UP CG context: baseline at `y`, aligned on `x`.
///
/// # Safety
/// `ctx` is a live CGContext.
pub(crate) unsafe fn draw(ctx: *mut c_void, x: f64, y: f64, text: &str, size: f64, color: &Color, font: &Font) {
    let Some(Line { line, width }) = line(text, size, color, font) else {
        return;
    };
    let x0 = match font.align {
        TextAlign::Left => x,
        TextAlign::Center => x - width * 0.5,
        TextAlign::Right => x - width,
    };
    // SAFETY: `ctx` is live (this function's contract); `line` is a live
    // CTLine.
    unsafe {
        CGContextSetTextMatrix(ctx, CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 });
        CGContextSetTextPosition(ctx, x0, y);
        CTLineDraw(line.as_ptr(), ctx);
    }
}
