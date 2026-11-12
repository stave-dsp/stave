//! The Scene renderer — the ONE place raw CoreGraphics drawing
//! lives. Widgets build `stave_ui::Scene`
//! values (pure, property-tested); this maps ops 1:1 onto CG. The arc
//! convention is CG's own (`Seg::Arc` fields pass straight through
//! to `CGContextAddArc`), so migrated widgets are pixel-faithful by
//! construction.
//!
//! `Op::Text` draws through CoreText (it landed with its first
//! consumer — the algorithm diagram's op numbers): the system UI
//! font at the op's size, `at` = horizontal CENTER of the line,
//! vertical BASELINE. Scene authors center by construction; they
//! cannot measure text, so the renderer owns the centering.

use core::ffi::c_void;

use stave_ui::scene::{Brush, Cap, Image, Op, Path, Scene, Seg};
use stave_ui::style::Color;

use super::knob::{
    CGColorSpaceCreateDeviceRGB, CGColorSpaceRelease, CGContextAddArc, CGContextAddLineToPoint,
    CGContextBeginPath, CGContextClip, CGContextDrawLinearGradient, CGContextFillPath,
    CGContextMoveToPoint, CGContextRestoreGState, CGContextSaveGState, CGContextSetLineCap,
    CGContextSetLineWidth, CGContextSetRGBFillColor, CGContextSetRGBStrokeColor,
    CGContextStrokePath, CGGradientCreateWithColorComponents, CGGradientRelease,
};
use crate::CGPoint;

extern crate alloc;
use alloc::vec::Vec;

extern "C" {
    fn CGContextClosePath(c: *mut c_void);
    fn CGContextAddQuadCurveToPoint(c: *mut c_void, cpx: f64, cpy: f64, x: f64, y: f64);
    fn CGContextAddCurveToPoint(
        c: *mut c_void,
        c1x: f64,
        c1y: f64,
        c2x: f64,
        c2y: f64,
        x: f64,
        y: f64,
    );
}

unsafe fn emit_path(ctx: *mut c_void, path: &Path) {
    unsafe {
        CGContextBeginPath(ctx);
        for seg in &path.segs {
            match *seg {
                Seg::MoveTo(p) => CGContextMoveToPoint(ctx, p.x, p.y),
                Seg::LineTo(p) => CGContextAddLineToPoint(ctx, p.x, p.y),
                Seg::QuadTo(c1, p) => CGContextAddQuadCurveToPoint(ctx, c1.x, c1.y, p.x, p.y),
                Seg::CubicTo(c1, c2, p) => {
                    CGContextAddCurveToPoint(ctx, c1.x, c1.y, c2.x, c2.y, p.x, p.y)
                }
                Seg::Arc { c, r, a0, a1, cw } => {
                    CGContextAddArc(ctx, c.x, c.y, r, a0, a1, i32::from(cw))
                }
                Seg::Close => CGContextClosePath(ctx),
            }
        }
    }
}

/// Draw a scene into a CGContext, ops in order.
///
/// # Safety
/// `ctx` must be a live CGContext for the current drawing pass.
pub(crate) unsafe fn render(ctx: *mut c_void, scene: &Scene) {
    for op in &scene.ops {
        match op {
            Op::Fill { path, brush } => match brush {
                Brush::Solid(c) => unsafe {
                    CGContextSetRGBFillColor(ctx, c.r, c.g, c.b, c.a);
                    emit_path(ctx, path);
                    CGContextFillPath(ctx);
                },
                Brush::Linear { from, to, stops } => unsafe {
                    CGContextSaveGState(ctx);
                    emit_path(ctx, path);
                    CGContextClip(ctx);
                    let mut comps: Vec<f64> = Vec::with_capacity(stops.len() * 4);
                    let mut locs: Vec<f64> = Vec::with_capacity(stops.len());
                    for (t, c) in stops {
                        comps.extend_from_slice(&[c.r, c.g, c.b, c.a]);
                        locs.push(*t);
                    }
                    let cs = CGColorSpaceCreateDeviceRGB();
                    let grad = CGGradientCreateWithColorComponents(
                        cs,
                        comps.as_ptr(),
                        locs.as_ptr(),
                        stops.len(),
                    );
                    CGContextDrawLinearGradient(
                        ctx,
                        grad,
                        CGPoint {
                            x: from.x,
                            y: from.y,
                        },
                        CGPoint { x: to.x, y: to.y },
                        3, // extend both ends
                    );
                    CGGradientRelease(grad);
                    CGColorSpaceRelease(cs);
                    CGContextRestoreGState(ctx);
                },
            },
            Op::Stroke { path, stroke } => unsafe {
                let c = stroke.color;
                CGContextSetRGBStrokeColor(ctx, c.r, c.g, c.b, c.a);
                CGContextSetLineWidth(ctx, stroke.width);
                CGContextSetLineCap(
                    ctx,
                    match stroke.cap {
                        Cap::Butt => 0,
                        Cap::Round => 1,
                    },
                );
                emit_path(ctx, path);
                CGContextStrokePath(ctx);
            },
            Op::Text {
                at,
                text,
                size,
                color,
            } => unsafe {
                draw_text(ctx, at.x, at.y, text, *size, color);
            },
            Op::Image { at, w, h, image } => unsafe {
                draw_image(ctx, at.x, at.y, *w, *h, image);
            },
        }
    }
}

// ------------------------------------------------------------- image
extern "C" {
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const u8,
        size: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize)>,
    ) -> *mut c_void;
    fn CGDataProviderRelease(p: *mut c_void);
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        colorspace: *mut c_void,
        bitmap_info: u32,
        provider: *mut c_void,
        decode: *const f32,
        should_interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGContextDrawImage(c: *mut c_void, rect: crate::CGRect, image: *mut c_void);
    fn CGContextSetInterpolationQuality(c: *mut c_void, q: i32);
}

/// CG is done with the pixels: drop the Arc reference the provider held.
unsafe extern "C" fn release_shared(_info: *mut c_void, data: *const u8, size: usize) {
    let slice = core::ptr::slice_from_raw_parts(data, size);
    drop(unsafe { alloc::sync::Arc::from_raw(slice) });
}

/// One shared bitmap into the y-up rect with bottom-left `(x, y)`.
/// The provider borrows the Arc'd pixels (one refcount, no copy), so
/// a memoized picture costs nothing to hand back each paint.
unsafe fn draw_image(ctx: *mut c_void, x: f64, y: f64, w: f64, h: f64, image: &Image) {
    let (pw, ph) = (image.px_w as usize, image.px_h as usize);
    if pw == 0 || ph == 0 || image.rgba.len() < pw * ph * 4 {
        return;
    }
    let shared = alloc::sync::Arc::into_raw(image.rgba.clone());
    unsafe {
        let provider = CGDataProviderCreateWithData(
            core::ptr::null_mut(),
            shared as *const u8,
            image.rgba.len(),
            Some(release_shared),
        );
        let cs = CGColorSpaceCreateDeviceRGB();
        // kCGImageAlphaPremultipliedLast = 1.
        let img = CGImageCreate(pw, ph, 8, 32, pw * 4, cs, 1, provider, core::ptr::null(), true, 0);
        CGContextSetInterpolationQuality(ctx, 3); // kCGInterpolationHigh
        CGContextDrawImage(
            ctx,
            crate::CGRect {
                origin: CGPoint { x, y },
                size: crate::CGSize { width: w, height: h },
            },
            img,
        );
        CGImageRelease(img);
        CGDataProviderRelease(provider);
        CGColorSpaceRelease(cs);
    }
}

// -------------------------------------------------------------- text
#[repr(C)]
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
    fn CTFontCreateUIFontForLanguage(
        ui_type: u32,
        size: f64,
        language: *const c_void,
    ) -> *const c_void;
    fn CTLineCreateWithAttributedString(s: *const c_void) -> *const c_void;
    fn CTLineDraw(line: *const c_void, ctx: *mut c_void);
    fn CTLineGetTypographicBounds(
        line: *const c_void,
        ascent: *mut f64,
        descent: *mut f64,
        leading: *mut f64,
    ) -> f64;
    static kCTFontAttributeName: *const c_void;
    static kCTForegroundColorAttributeName: *const c_void;
}

extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: bool,
    ) -> *const c_void;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> *const c_void;
    fn CFAttributedStringCreate(
        alloc: *const c_void,
        s: *const c_void,
        attributes: *const c_void,
    ) -> *const c_void;
    fn CFRelease(v: *const c_void);
    fn CGColorCreateGenericRGB(r: f64, g: f64, b: f64, a: f64) -> *const c_void;
    fn CGContextSetTextMatrix(ctx: *mut c_void, t: CGAffineTransform);
    fn CGContextSetTextPosition(ctx: *mut c_void, x: f64, y: f64);
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

const UTF8: u32 = 0x0800_0100;
const UI_FONT_EMPHASIZED_SYSTEM: u32 = 3;

/// One line of text, centered horizontally on `x`, baseline at `y`.
pub(crate) unsafe fn draw_text(ctx: *mut c_void, x: f64, y: f64, text: &str, size: f64, color: &Color) {
    unsafe {
        let cf = CFStringCreateWithBytes(
            core::ptr::null(),
            text.as_ptr(),
            text.len() as isize,
            UTF8,
            false,
        );
        if cf.is_null() {
            return;
        }
        let font =
            CTFontCreateUIFontForLanguage(UI_FONT_EMPHASIZED_SYSTEM, size, core::ptr::null());
        let cg_color = CGColorCreateGenericRGB(color.r, color.g, color.b, color.a);
        let keys = [kCTFontAttributeName, kCTForegroundColorAttributeName];
        let values = [font, cg_color];
        let attrs = CFDictionaryCreate(
            core::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            2,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let styled = CFAttributedStringCreate(core::ptr::null(), cf, attrs);
        let line = CTLineCreateWithAttributedString(styled);
        let width = CTLineGetTypographicBounds(
            line,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        CGContextSetTextMatrix(
            ctx,
            CGAffineTransform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        CGContextSetTextPosition(ctx, x - width * 0.5, y);
        CTLineDraw(line, ctx);
        for v in [line, styled, attrs, cg_color, font, cf] {
            if !v.is_null() {
                CFRelease(v);
            }
        }
    }
}
