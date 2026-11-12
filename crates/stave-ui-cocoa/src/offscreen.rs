//! Text for the portable `stave-raster` renderer, and the blit: CoreText
//! paints text straight into the canvas's pixels ([`CoreText`], the
//! macOS `TextPainter`), and `draw_canvas` shows a canvas in a view.
//! Pixels are premultiplied RGBA8, row 0 at the top.

use core::ffi::c_void;

use stave_raster::{Canvas, TextPainter};
use stave_ui::scene::Pt;
use stave_ui::style::Color;

use crate::cf::{CGRect, CfRef};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
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
    fn CGContextScaleCTM(c: *mut c_void, sx: f64, sy: f64);
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const u8,
        size: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize)>,
    ) -> *mut c_void;
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
    fn CGContextDrawImage(c: *mut c_void, rect: CGRect, image: *mut c_void);
}

/// kCGImageAlphaPremultipliedLast: RGBA, premultiplied.
const RGBA_PREMULTIPLIED: u32 = 1;

/// macOS text for the software renderer: CoreText, drawn straight into
/// the canvas (a bitmap context over its pixels), so text keeps its
/// place in the paint order. It draws through a context scaled by the
/// backing factor at the font's POINT size, so glyph hinting and line
/// metrics are the Mac's own.
///
/// One bitmap context serves a whole frame: made on the first text of
/// the frame, over the canvas's pixels (which do not move while the
/// frame renders; a different buffer gets a new context), released when
/// the painter drops.
pub struct CoreText {
    pub scale: f64,
    /// The context, and the pixels it draws into.
    ctx: Option<(CfRef, *const u8)>,
}

impl CoreText {
    pub fn new(scale: f64) -> Self {
        Self { scale, ctx: None }
    }

    /// The context over `canvas`'s pixels, made if need be.
    fn context(&mut self, canvas: &mut Canvas) -> Option<*mut c_void> {
        let over = canvas.px.as_ptr();
        if self.ctx.as_ref().is_none_or(|(_, p)| *p != over) {
            self.ctx = None;
            let (w, h) = (canvas.w, canvas.h);
            assert!(canvas.px.len() >= w * h * 4, "a canvas holds its pixels");
            // SAFETY: the context draws into `w × h × 4` bytes the canvas
            // owns; it is used only while those pixels stay put (the next
            // call with moved pixels replaces it).
            let ctx = unsafe {
                let cs = CfRef::from_create(CGColorSpaceCreateDeviceRGB())?;
                let ctx = CGBitmapContextCreate(canvas.px.as_mut_ptr().cast(), w, h, 8, w * 4, cs.as_ptr(), RGBA_PREMULTIPLIED);
                CfRef::from_create(ctx)?
            };
            // SAFETY: a live context.
            unsafe { CGContextScaleCTM(ctx.as_ptr(), self.scale, self.scale) };
            self.ctx = Some((ctx, over));
        }
        self.ctx.as_ref().map(|(c, _)| c.as_ptr())
    }
}

impl TextPainter for CoreText {
    fn text(&mut self, canvas: &mut Canvas, at: Pt, text: &str, size: f64, color: Color, font: &stave_ui::scene::Font) {
        let (h, k) = (canvas.h as f64, self.scale);
        let Some(ctx) = self.context(canvas) else {
            return;
        };
        // The context is y-up in points; the canvas is y-down px.
        // SAFETY: a live context over the canvas.
        unsafe { crate::text::draw(ctx, at.x / k, (h - at.y) / k, text, size / k, &color, font) };
    }
}

/// Show a rendered canvas in a view's (y-up) context, filling `w × h`
/// points. The image borrows the canvas's pixels for the one draw (CG
/// draws synchronously; the image is released before returning).
///
/// # Safety
/// `ctx` is a live CGContext for the current drawing pass.
pub(crate) unsafe fn draw_canvas(ctx: *mut c_void, canvas: &Canvas, w: f64, h: f64) {
    if canvas.w == 0 || canvas.h == 0 || canvas.px.len() < canvas.w * canvas.h * 4 {
        return;
    }
    // SAFETY: the provider borrows the canvas's pixels (length checked
    // above) and the image made from it is drawn and released before this
    // returns; `ctx` is live (this function's contract).
    unsafe {
        let Some(provider) = CfRef::from_create(CGDataProviderCreateWithData(core::ptr::null_mut(), canvas.px.as_ptr(), canvas.px.len(), None)) else {
            return;
        };
        // sRGB: what a window context takes the scenes' (DeviceRGB) fill
        // colours to mean; tagging the image DeviceRGB shifts every pixel.
        let Some(cs) = CfRef::from_create(CGColorSpaceCreateWithName(kCGColorSpaceSRGB)) else {
            return;
        };
        let image = CGImageCreate(canvas.w, canvas.h, 8, 32, canvas.w * 4, cs.as_ptr(), RGBA_PREMULTIPLIED, provider.as_ptr(), core::ptr::null(), false, 0);
        let Some(image) = CfRef::from_create(image) else {
            return;
        };
        CGContextDrawImage(ctx, CGRect::new(0.0, 0.0, w, h), image.as_ptr());
    }
}
