//! Rendering a Scene into RGBA pixels with the portable `stave-raster`
//! software renderer, CoreText painting the text straight into the same
//! buffer ([`CoreText`], the macOS `TextPainter`), and showing the
//! result in a view ([`draw_canvas`]). Pixels are premultiplied RGBA8,
//! row 0 at the top.

use core::ffi::c_void;

extern crate alloc;
use alloc::vec::Vec;

use stave_raster::{Canvas, TextPainter};
use stave_ui::scene::{Pt, Scene};
use stave_ui::style::Color;

extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceRelease(cs: *mut c_void);
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    static kCGColorSpaceSRGB: *const c_void;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
    ) -> *mut c_void;
    fn CGContextRelease(c: *mut c_void);
    fn CGContextScaleCTM(c: *mut c_void, sx: f64, sy: f64);
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
}

/// kCGImageAlphaPremultipliedLast: RGBA, premultiplied.
const RGBA_PREMULTIPLIED: u32 = 1;

/// A CoreGraphics bitmap context over `px` (w × h, RGBA8 premultiplied,
/// row 0 at the top). The caller releases it.
///
/// # Safety
/// `px` must outlive the context and hold `w * h * 4` bytes.
unsafe fn bitmap_context(px: &mut [u8], w: usize, h: usize) -> *mut c_void {
    unsafe {
        let cs = CGColorSpaceCreateDeviceRGB();
        let ctx = CGBitmapContextCreate(px.as_mut_ptr() as *mut c_void, w, h, 8, w * 4, cs, RGBA_PREMULTIPLIED);
        CGColorSpaceRelease(cs);
        ctx
    }
}

/// macOS text for the software renderer: CoreText, drawn straight into
/// the canvas (a bitmap context over its pixels), so text keeps its
/// place in the paint order. It draws through a context scaled by the
/// backing factor at the font's POINT size — exactly as the views always
/// have — so glyph hinting and line metrics are the Mac's own.
///
/// One bitmap context serves a whole frame (created on the first text
/// of the frame, over the canvas's pixels — which do not move while the
/// frame renders — and released when the painter drops).
pub struct CoreText {
    pub scale: f64,
    ctx: *mut c_void,
    over: *const u8,
}

impl CoreText {
    pub fn new(scale: f64) -> Self {
        Self { scale, ctx: core::ptr::null_mut(), over: core::ptr::null() }
    }
}

impl Drop for CoreText {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { CGContextRelease(self.ctx) };
        }
    }
}

impl TextPainter for CoreText {
    fn text(&mut self, canvas: &mut Canvas, at: Pt, text: &str, size: f64, color: Color, font: &stave_ui::scene::Font) {
        let (w, h) = (canvas.w, canvas.h);
        let k = self.scale;
        unsafe {
            if self.ctx.is_null() || self.over != canvas.px.as_ptr() {
                if !self.ctx.is_null() {
                    CGContextRelease(self.ctx);
                }
                self.ctx = bitmap_context(&mut canvas.px, w, h);
                self.over = canvas.px.as_ptr();
                if self.ctx.is_null() {
                    return;
                }
                CGContextScaleCTM(self.ctx, k, k);
            }
            // The context is y-up in points; the canvas is y-down px.
            crate::text::draw(self.ctx, at.x / k, (h as f64 - at.y) / k, text, size / k, &color, font);
        }
    }
}

/// The portable renderer, with CoreText for text: into `canvas`.
pub fn render_raster_into(scene: &Scene, w: f64, h: f64, scale: f64, canvas: &mut Canvas) {
    let (pw, ph) = ((w * scale).round() as usize, (h * scale).round() as usize);
    canvas.reset(pw, ph);
    stave_raster::render(scene, scale, canvas, &mut CoreText::new(scale));
}

/// The portable renderer, with CoreText for text.
pub fn render_raster(scene: &Scene, w: f64, h: f64, scale: f64) -> Vec<u8> {
    let mut c = Canvas::new(0, 0);
    render_raster_into(scene, w, h, scale, &mut c);
    c.px
}

/// Show a rendered canvas in a view's (y-up) context, filling `w × h`
/// points. The image borrows the canvas's pixels for the one draw (CG
/// draws synchronously; the image is released before returning).
///
/// # Safety
/// `ctx` must be a live CGContext for the current drawing pass.
pub(crate) unsafe fn draw_canvas(ctx: *mut c_void, canvas: &Canvas, w: f64, h: f64) {
    if canvas.w == 0 || canvas.h == 0 {
        return;
    }
    unsafe {
        let provider = CGDataProviderCreateWithData(core::ptr::null_mut(), canvas.px.as_ptr(), canvas.px.len(), None);
        // sRGB: what a window context takes the scenes' (DeviceRGB) fill
        // colours to mean; tagging the image DeviceRGB shifts every pixel.
        let cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        let img = CGImageCreate(canvas.w, canvas.h, 8, 32, canvas.w * 4, cs, RGBA_PREMULTIPLIED, provider, core::ptr::null(), false, 0);
        CGContextDrawImage(
            ctx,
            crate::CGRect { origin: crate::CGPoint { x: 0.0, y: 0.0 }, size: crate::CGSize { width: w, height: h } },
            img,
        );
        CGImageRelease(img);
        CGDataProviderRelease(provider);
        CGColorSpaceRelease(cs);
    }
}
