//! Text that is the same on every machine: each glyph a filled box on
//! the baseline, a fixed advance per character. Tests and golden images
//! paint with it so a picture depends on the scene alone, never on the
//! fonts a machine has; layout checks measure with the same metrics.

use stave_ui::scene::{Font, Pt, TextAlign};
use stave_ui::style::Color;

use crate::{Canvas, TextPainter};

/// Deterministic text: every visible glyph a box `BOX_W` × `CAP` of the
/// size, `ADVANCE` of the size apart (plus the font's tracking).
#[derive(Copy, Clone, Debug, Default)]
pub struct BoxText;

impl BoxText {
    /// Advance per glyph, as a fraction of the size.
    pub const ADVANCE: f64 = 0.6;
    /// Glyph box width, as a fraction of the size.
    pub const BOX_W: f64 = 0.45;
    /// Glyph box height above the baseline (cap height), as a fraction
    /// of the size.
    pub const CAP: f64 = 0.7;

    /// The width a line takes, in the units of `size`.
    pub fn width(text: &str, size: f64, font: &Font) -> f64 {
        let n = text.chars().count() as f64;
        if n == 0.0 {
            return 0.0;
        }
        n * (size * Self::ADVANCE + font.tracking) - (size * (Self::ADVANCE - Self::BOX_W) + font.tracking)
    }

    /// The box a line covers: (x, y, w, h), y down, with its baseline
    /// anchor at `at` aligned by `font.align`.
    pub fn bounds(at: Pt, text: &str, size: f64, font: &Font) -> (f64, f64, f64, f64) {
        let w = Self::width(text, size, font);
        let x = match font.align {
            TextAlign::Left => at.x,
            TextAlign::Center => at.x - w / 2.0,
            TextAlign::Right => at.x - w,
        };
        (x, at.y - size * Self::CAP, w, size * Self::CAP)
    }
}

impl TextPainter for BoxText {
    fn text(&mut self, canvas: &mut Canvas, at: Pt, text: &str, size: f64, color: Color, font: &Font) {
        let (x0, top, _, h) = Self::bounds(at, text, size, font);
        let step = size * Self::ADVANCE + font.tracking;
        let a = color.a.clamp(0.0, 1.0);
        let c = [color.r, color.g, color.b].map(|v| (v.clamp(0.0, 1.0) * a * 255.0) as f32);
        for (i, ch) in text.chars().enumerate() {
            if ch.is_whitespace() {
                continue;
            }
            let x = x0 + i as f64 * step;
            fill_box(canvas, x, top, size * Self::BOX_W, h, c, (a * 255.0) as f32);
        }
    }
}

/// Source-over a box with exact edge coverage (premultiplied colour).
fn fill_box(canvas: &mut Canvas, x: f64, y: f64, w: f64, h: f64, c: [f32; 3], a: f32) {
    let (x1, y1) = (x + w, y + h);
    let cols = (x.floor().max(0.0) as usize)..(x1.ceil().min(canvas.w as f64).max(0.0) as usize);
    let rows = (y.floor().max(0.0) as usize)..(y1.ceil().min(canvas.h as f64).max(0.0) as usize);
    for py in rows {
        let cy = ((py + 1) as f64).min(y1) - (py as f64).max(y);
        for px in cols.clone() {
            let cx = ((px + 1) as f64).min(x1) - (px as f64).max(x);
            let k = (cx * cy).clamp(0.0, 1.0) as f32;
            if k > 0.0 {
                let i = (py * canvas.w + px) * 4;
                crate::over(&mut canvas.px[i..i + 4], c[0] * k, c[1] * k, c[2] * k, a * k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_centred_line_sits_on_its_anchor() {
        let font = Font::DEFAULT;
        let (x, y, w, h) = BoxText::bounds(Pt { x: 50.0, y: 20.0 }, "ab", 10.0, &font);
        assert_eq!((x + w / 2.0, y + h), (50.0, 20.0));
        assert!((w - (6.0 + 4.5)).abs() < 1e-12, "two glyphs: one advance and one box");
    }

    #[test]
    fn glyphs_paint_where_they_are_measured() {
        let mut c = Canvas::new(40, 20);
        let font = Font { align: TextAlign::Left, ..Font::DEFAULT };
        BoxText.text(&mut c, Pt { x: 2.0, y: 15.0 }, "I I", 10.0, Color::rgba(1.0, 1.0, 1.0, 1.0), &font);
        let a = |x: usize, y: usize| c.px[(y * 40 + x) * 4 + 3];
        assert_eq!(a(4, 12), 255, "inside the first glyph");
        assert_eq!(a(10, 12), 0, "the space paints nothing");
        assert_eq!(a(16, 12), 255, "the third glyph");
        assert_eq!(a(4, 16), 0, "nothing below the baseline");
    }
}
