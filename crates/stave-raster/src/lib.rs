//! `stave-raster` — Stave's portable software renderer. It draws a Scene
//! (the one vector vocabulary every Stave picture is made of) into a
//! premultiplied RGBA8 pixel buffer, identically on every platform; the
//! platform's only jobs are to show the buffer and to paint TEXT (fonts
//! are the one thing left to the OS — see [`TextPainter`]).
//!
//! * `path.rs` — flattening (lines, curves, CG-convention arcs) to px.
//! * `coverage.rs` — exact-area anti-aliasing (signed-area accumulation,
//!   nonzero winding).
//! * `stroke.rs` — strokes as polygons (quads, round joins, caps).
//! * here — the op loop and compositing: solid and linear-gradient
//!   fills (gradients interpolate in the encoded colour space and extend
//!   both ends, as CoreGraphics does), source-over in encoded space, and
//!   images (bilinear, premultiplied; an image drawn 1:1 is a straight
//!   copy).
//!
//! Coordinates: scenes are y-DOWN in points; `render` scales by the
//! backing factor. Pure Rust, no unsafe, no platform code.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use stave_ui::scene::{Brush, Image, Op, Pt, Scene};
use stave_ui::style::Color;

mod coverage;
mod path;
mod stroke;

pub use coverage::Mask;

/// A premultiplied RGBA8 pixel buffer, row 0 at the top.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![0; w * h * 4] }
    }

    /// Clear to transparent (reuses the allocation when sizes match).
    pub fn reset(&mut self, w: usize, h: usize) {
        if self.w != w || self.h != h {
            *self = Self::new(w, h);
        } else {
            self.px.fill(0);
        }
    }
}

/// The platform's text: draw `text` into `canvas`, its line centred on
/// `at.x` with its baseline at `at.y` (device px), at `size` px, in
/// `color`. A painter draws straight into the buffer, so text keeps its
/// place in the paint order.
pub trait TextPainter {
    fn text(&mut self, canvas: &mut Canvas, at: Pt, text: &str, size: f64, color: Color);
}

/// No text (headless tests of everything else).
pub struct NoText;
impl TextPainter for NoText {
    fn text(&mut self, _: &mut Canvas, _: Pt, _: &str, _: f64, _: Color) {}
}

/// Draw `scene` (points, y-down) into `canvas` at `scale` px per point.
pub fn render(scene: &Scene, scale: f64, canvas: &mut Canvas, text: &mut dyn TextPainter) {
    for op in &scene.ops {
        match op {
            Op::Fill { path, brush } => {
                let polys: Vec<Vec<Pt>> = path::flatten(path, scale).into_iter().map(|p| p.pts).collect();
                fill(canvas, &polys, brush, scale);
            }
            Op::Stroke { path, stroke } => {
                let polys = stroke::stroke(&path::flatten(path, scale), stroke.width * scale, stroke.cap);
                fill(canvas, &polys, &Brush::Solid(stroke.color), scale);
            }
            Op::Text { at, text: s, size, color } => {
                text.text(canvas, Pt { x: at.x * scale, y: at.y * scale }, s, size * scale, *color);
            }
            Op::Image { at, w, h, image } => draw_image(canvas, image, at.x * scale, at.y * scale, w * scale, h * scale),
        }
    }
}

#[inline]
fn to8(v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * 255.0).round()
}

/// Source-over one premultiplied pixel (components 0..255 as f32).
#[inline]
fn over(dst: &mut [u8], r: f32, g: f32, b: f32, a: f32) {
    let k = 1.0 - a / 255.0;
    dst[0] = (r + dst[0] as f32 * k).round().min(255.0) as u8;
    dst[1] = (g + dst[1] as f32 * k).round().min(255.0) as u8;
    dst[2] = (b + dst[2] as f32 * k).round().min(255.0) as u8;
    dst[3] = (a + dst[3] as f32 * k).round().min(255.0) as u8;
}

/// The colour of a linear gradient at `t` (stops interpolated in the
/// encoded space; extended at both ends).
fn gradient_at(stops: &[(f64, Color)], t: f64) -> Color {
    if stops.is_empty() {
        return Color::rgba(0.0, 0.0, 0.0, 0.0);
    }
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return Color::rgba(c0.r + (c1.r - c0.r) * u, c0.g + (c1.g - c0.g) * u, c0.b + (c1.b - c0.b) * u, c0.a + (c1.a - c0.a) * u);
        }
    }
    stops[stops.len() - 1].1
}

/// Fill polygons (device px) with a brush (gradient ends in points).
fn fill(canvas: &mut Canvas, polys: &[Vec<Pt>], brush: &Brush, scale: f64) {
    let Some(b) = coverage::bounds(polys) else { return };
    let Some(mut mask) = Mask::new(b, canvas.w, canvas.h) else { return };
    for p in polys {
        mask.polygon(p);
    }
    mask.finish();
    let (cw, x0, y0) = (canvas.w, mask.x0 as usize, mask.y0 as usize);
    match brush {
        Brush::Solid(c) => {
            let (r, g, bl, a) = (to8(c.r), to8(c.g), to8(c.b), c.a.clamp(0.0, 1.0));
            for my in 0..mask.h {
                for mx in 0..mask.w {
                    let cov = mask.cov[my * mask.w + mx];
                    if cov <= 0.0 {
                        continue;
                    }
                    let k = a as f32 * cov;
                    let i = ((y0 + my) * cw + x0 + mx) * 4;
                    over(&mut canvas.px[i..i + 4], r as f32 * k, g as f32 * k, bl as f32 * k, 255.0 * k);
                }
            }
        }
        Brush::Linear { from, to, stops } => {
            let (fx, fy) = (from.x * scale, from.y * scale);
            let (dx, dy) = (to.x * scale - fx, to.y * scale - fy);
            let len2 = (dx * dx + dy * dy).max(1e-12);
            for my in 0..mask.h {
                for mx in 0..mask.w {
                    let cov = mask.cov[my * mask.w + mx];
                    if cov <= 0.0 {
                        continue;
                    }
                    let (px, py) = ((x0 + mx) as f64 + 0.5, (y0 + my) as f64 + 0.5);
                    let t = ((px - fx) * dx + (py - fy) * dy) / len2;
                    let c = gradient_at(stops, t);
                    let k = c.a.clamp(0.0, 1.0) as f32 * cov;
                    let i = ((y0 + my) * cw + x0 + mx) * 4;
                    over(&mut canvas.px[i..i + 4], to8(c.r) as f32 * k, to8(c.g) as f32 * k, to8(c.b) as f32 * k, 255.0 * k);
                }
            }
        }
    }
}

/// Draw a premultiplied image into the device rect (x, y, w, h):
/// bilinear sampling; a pixel-aligned 1:1 draw is a straight blend.
fn draw_image(canvas: &mut Canvas, img: &Image, x: f64, y: f64, w: f64, h: f64) {
    let (iw, ih) = (img.px_w as usize, img.px_h as usize);
    if iw == 0 || ih == 0 || img.rgba.len() < iw * ih * 4 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let (cw, ch) = (canvas.w as i64, canvas.h as i64);
    let (x0, y0) = ((x.floor() as i64).max(0), (y.floor() as i64).max(0));
    let (x1, y1) = (((x + w).ceil() as i64).min(cw), ((y + h).ceil() as i64).min(ch));
    let one_to_one = (w - iw as f64).abs() < 1e-6 && (h - ih as f64).abs() < 1e-6 && x.fract() == 0.0 && y.fract() == 0.0;
    if one_to_one {
        // The common case (a face's bake, a sprite at its own size),
        // row by row: runs of opaque pixels are one bulk copy, the rest
        // blend in integer source-over.
        let (ox, oy) = (x as i64, y as i64);
        let (cx0, cx1) = (x0.max(ox), x1.min(ox + iw as i64));
        if cx1 <= cx0 {
            return;
        }
        let n = (cx1 - cx0) as usize;
        for py in y0.max(oy)..y1.min(oy + ih as i64) {
            let srow = ((py - oy) as usize * iw + (cx0 - ox) as usize) * 4;
            let drow = ((py * cw + cx0) * 4) as usize;
            let src = &img.rgba[srow..srow + n * 4];
            let dst = &mut canvas.px[drow..drow + n * 4];
            let mut i = 0;
            while i < n {
                // An opaque run: copy it whole.
                let start = i;
                while i < n && src[i * 4 + 3] == 255 {
                    i += 1;
                }
                if i > start {
                    dst[start * 4..i * 4].copy_from_slice(&src[start * 4..i * 4]);
                }
                // A translucent run: blend.
                while i < n && src[i * 4 + 3] != 255 {
                    let a = src[i * 4 + 3];
                    if a != 0 {
                        let k = 255 - u32::from(a);
                        for c in 0..4 {
                            let v = u32::from(src[i * 4 + c]) + (u32::from(dst[i * 4 + c]) * k + 127) / 255;
                            dst[i * 4 + c] = v.min(255) as u8;
                        }
                    }
                    i += 1;
                }
            }
        }
        return;
    }
    let (sx, sy) = (iw as f64 / w, ih as f64 / h);
    for py in y0..y1 {
        for px in x0..x1 {
            let (r, g, b, a) = if one_to_one {
                let (ix, iy) = ((px - x as i64) as usize, (py - y as i64) as usize);
                if ix >= iw || iy >= ih {
                    continue;
                }
                let s = &img.rgba[(iy * iw + ix) * 4..(iy * iw + ix) * 4 + 4];
                (s[0] as f32, s[1] as f32, s[2] as f32, s[3] as f32)
            } else {
                // Sample at the pixel centre, in image space.
                let u = (px as f64 + 0.5 - x) * sx - 0.5;
                let v = (py as f64 + 0.5 - y) * sy - 0.5;
                if u < -0.5 || v < -0.5 || u > iw as f64 - 0.5 || v > ih as f64 - 0.5 {
                    continue;
                }
                let (u0, v0) = (u.floor().max(0.0) as usize, v.floor().max(0.0) as usize);
                let (u1, v1) = ((u0 + 1).min(iw - 1), (v0 + 1).min(ih - 1));
                let (fu, fv) = ((u - u.floor()).clamp(0.0, 1.0) as f32, (v - v.floor()).clamp(0.0, 1.0) as f32);
                let at = |xx: usize, yy: usize, c: usize| img.rgba[(yy * iw + xx) * 4 + c] as f32;
                let mut out = [0.0f32; 4];
                for (c, o) in out.iter_mut().enumerate() {
                    let top = at(u0, v0, c) + (at(u1, v0, c) - at(u0, v0, c)) * fu;
                    let bot = at(u0, v1, c) + (at(u1, v1, c) - at(u0, v1, c)) * fu;
                    *o = top + (bot - top) * fv;
                }
                (out[0], out[1], out[2], out[3])
            };
            if a <= 0.0 {
                continue;
            }
            let i = ((py * cw + px) * 4) as usize;
            over(&mut canvas.px[i..i + 4], r, g, b, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::{pt, Path, Stroke};

    #[test]
    fn a_solid_square_paints_its_pixels_and_only_them() {
        let mut sc = Scene::new();
        sc.solid(Path::rect(pt(1.0, 1.0), 2.0, 2.0), Color::rgba(1.0, 0.0, 0.0, 1.0));
        let mut c = Canvas::new(8, 8);
        render(&sc, 2.0, &mut c, &mut NoText);
        let px = |x: usize, y: usize| &c.px[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
        assert_eq!(px(3, 3), &[255, 0, 0, 255]);
        assert_eq!(px(1, 1), &[0, 0, 0, 0]);
        assert_eq!(px(6, 6), &[0, 0, 0, 0]);
    }

    #[test]
    fn half_alpha_composites_source_over() {
        let mut sc = Scene::new();
        sc.solid(Path::rect(pt(0.0, 0.0), 4.0, 4.0), Color::rgba(0.0, 0.0, 1.0, 1.0));
        sc.solid(Path::rect(pt(0.0, 0.0), 4.0, 4.0), Color::rgba(1.0, 0.0, 0.0, 0.5));
        let mut c = Canvas::new(4, 4);
        render(&sc, 1.0, &mut c, &mut NoText);
        assert_eq!(&c.px[0..4], &[128, 0, 128, 255]);
    }

    #[test]
    fn a_round_stroke_has_round_ends() {
        let mut sc = Scene::new();
        sc.stroke(Path::new().move_to(pt(4.0, 8.0)).line_to(pt(12.0, 8.0)), Stroke::round(Color::rgba(1.0, 1.0, 1.0, 1.0), 4.0));
        let mut c = Canvas::new(16, 16);
        render(&sc, 1.0, &mut c, &mut NoText);
        let a = |x: usize, y: usize| c.px[(y * 16 + x) * 4 + 3];
        assert_eq!(a(8, 8), 255, "the body");
        assert!(a(2, 8) > 100, "the round cap reaches past the end");
        assert_eq!(a(8, 3), 0, "nothing beyond the width");
    }

    #[test]
    fn an_image_drawn_one_to_one_is_copied() {
        let rgba: Vec<u8> = (0..16).flat_map(|i| [i as u8 * 10, 0, 0, 255]).collect();
        let img = Image { px_w: 4, px_h: 4, rgba: alloc::sync::Arc::from(rgba.clone()) };
        let mut sc = Scene::new();
        sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: 2.0, h: 2.0, image: img });
        let mut c = Canvas::new(4, 4);
        render(&sc, 2.0, &mut c, &mut NoText);
        assert_eq!(c.px, rgba);
    }

    #[test]
    fn a_gradient_runs_from_its_first_stop_to_its_last() {
        let mut sc = Scene::new();
        sc.fill(
            Path::rect(pt(0.0, 0.0), 10.0, 1.0),
            Brush::Linear { from: pt(0.0, 0.0), to: pt(10.0, 0.0), stops: vec![(0.0, Color::rgba(0.0, 0.0, 0.0, 1.0)), (1.0, Color::rgba(1.0, 1.0, 1.0, 1.0))] },
        );
        let mut c = Canvas::new(10, 1);
        render(&sc, 1.0, &mut c, &mut NoText);
        assert!(c.px[0] < 20 && c.px[9 * 4] > 235 && (c.px[5 * 4] as i32 - 140).abs() < 15);
    }
}
