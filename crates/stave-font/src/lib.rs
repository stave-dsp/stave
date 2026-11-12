//! Portable text for the software renderer: [`FontText`] is the
//! `TextPainter` of every platform without a text system of its own to
//! use (Windows and Linux; macOS keeps CoreText). The fonts are embedded
//! — IBM Plex Sans SemiBold and IBM Plex Mono Medium, subset to Latin-1
//! and a few symbols, under the SIL Open Font License 1.1
//! (`fonts/OFL.txt`) — so a face looks the same on every machine,
//! whatever fonts it has.
//!
//! Glyph outlines come from `skrifa` (parsing only); `stave-raster`
//! fills them with its own exact-area coverage, nonzero winding, so text
//! is anti-aliased exactly like every other shape. Unhinted; one weight
//! per family (`Font::weight` is not synthesised). Pure: no clock, file,
//! or global state.

#![forbid(unsafe_code)]

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use stave_raster::{Canvas, TextPainter};
use stave_ui::scene::{Brush, Family, FillRule, Font, Op, Path, Pt, Scene, Seg, TextAlign};
use stave_ui::style::Color;

const SANS: &[u8] = include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf");
const MONO: &[u8] = include_bytes!("../fonts/IBMPlexMono-Medium.ttf");

/// Text in the embedded fonts, filled by the raster's own coverage.
#[derive(Copy, Clone, Debug, Default)]
pub struct FontText;

fn face(family: Family) -> FontRef<'static> {
    let bytes = match family {
        Family::Mono => MONO,
        Family::Sans | Family::Rounded | Family::Serif => SANS,
    };
    FontRef::new(bytes).expect("the embedded fonts parse")
}

impl FontText {
    /// The advance of `text` set at `size` px in `font` (tracking is a
    /// fraction of the size per glyph, as on macOS), in px.
    pub fn width(text: &str, size: f64, font: &Font) -> f64 {
        let f = face(font.family);
        let metrics = f.glyph_metrics(Size::new(size as f32), LocationRef::default());
        let map = f.charmap();
        let mut w = 0.0;
        let mut n = 0usize;
        for ch in text.chars() {
            let gid = map.map(ch).unwrap_or_default();
            w += f64::from(metrics.advance_width(gid).unwrap_or(0.0));
            n += 1;
        }
        w + font.tracking * size * n.saturating_sub(1) as f64
    }

    /// One line as a path (px, y down): baseline at `at.y`, aligned on
    /// `at.x` by `font.align`.
    pub fn outline(at: Pt, text: &str, size: f64, font: &Font) -> Path {
        let f = face(font.family);
        let px = Size::new(size as f32);
        let metrics = f.glyph_metrics(px, LocationRef::default());
        let outlines = f.outline_glyphs();
        let map = f.charmap();
        let width = Self::width(text, size, font);
        let mut x = match font.align {
            TextAlign::Left => at.x,
            TextAlign::Center => at.x - width / 2.0,
            TextAlign::Right => at.x - width,
        };
        let mut pen = Pen { path: Path::new(), x, y: at.y };
        for ch in text.chars() {
            let gid = map.map(ch).unwrap_or_default();
            if let Some(glyph) = outlines.get(gid) {
                pen.x = x;
                // A glyph that fails to draw is left out; the rest of
                // the line still sets.
                let _ = glyph.draw(DrawSettings::unhinted(px, LocationRef::default()), &mut pen);
            }
            x += f64::from(metrics.advance_width(gid).unwrap_or(0.0)) + font.tracking * size;
        }
        pen.path
    }
}

/// Collects outlines into a y-down [`Path`] at a pen position.
struct Pen {
    path: Path,
    x: f64,
    y: f64,
}

impl Pen {
    fn at(&self, x: f32, y: f32) -> Pt {
        Pt { x: self.x + f64::from(x), y: self.y - f64::from(y) }
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.segs.push(Seg::MoveTo(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.segs.push(Seg::LineTo(p));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (c, p) = (self.at(cx, cy), self.at(x, y));
        self.path.segs.push(Seg::QuadTo(c, p));
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let (c0, c1, p) = (self.at(c0x, c0y), self.at(c1x, c1y), self.at(x, y));
        self.path.segs.push(Seg::CubicTo(c0, c1, p));
    }
    fn close(&mut self) {
        self.path.segs.push(Seg::Close);
    }
}

/// Layout measures with the same metrics the painter sets (points in,
/// points out: widths are linear in the size).
impl stave_ui::Fonts for FontText {
    fn width(&self, text: &str, size: f64, font: &Font) -> f64 {
        Self::width(text, size, font)
    }
}

impl TextPainter for FontText {
    fn text(&mut self, canvas: &mut Canvas, at: Pt, text: &str, size: f64, color: Color, font: &Font) {
        if text.is_empty() || size.is_nan() || size <= 0.0 {
            return;
        }
        let scene = Scene { ops: vec![Op::Fill { path: Self::outline(at, text, size, font), brush: Brush::Solid(color), rule: FillRule::NonZero }] };
        stave_raster::render(&scene, 1.0, canvas, &mut stave_raster::NoText);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(c: &Canvas) -> u64 {
        c.pixels().chunks(4).map(|p| u64::from(p[3])).sum()
    }

    #[test]
    fn every_family_parses_and_sets_ascii() {
        for family in [Family::Sans, Family::Mono, Family::Rounded, Family::Serif] {
            let font = Font::new(family, 600);
            assert!(FontText::width("Gain 12 dB", 14.0, &font) > 40.0, "{family:?}");
        }
    }

    #[test]
    fn text_lands_where_it_is_aligned() {
        let white = Color::rgba(1.0, 1.0, 1.0, 1.0);
        for (align, inside) in [(TextAlign::Left, 50..100), (TextAlign::Right, 0..50), (TextAlign::Center, 25..75)] {
            let mut c = Canvas::new(100, 30);
            let font = Font { align, ..Font::DEFAULT };
            FontText.text(&mut c, Pt { x: 50.0, y: 20.0 }, "MMM", 12.0, white, &font);
            assert!(ink(&c) > 0);
            for y in 0..30 {
                for x in 0..100 {
                    if c.pixels()[(y * 100 + x) * 4 + 3] > 0 {
                        assert!(inside.contains(&x), "{align:?}: ink at x={x}");
                        assert!(y <= 21, "{align:?}: ink below the baseline at y={y}");
                    }
                }
            }
        }
    }

    #[test]
    fn ink_scales_with_size_squared() {
        let white = Color::rgba(1.0, 1.0, 1.0, 1.0);
        let at = |size: f64| {
            let mut c = Canvas::new(400, 100);
            FontText.text(&mut c, Pt { x: 200.0, y: 80.0 }, "Tilt", size, white, &Font::DEFAULT);
            ink(&c) as f64
        };
        let r = at(40.0) / at(20.0);
        assert!((r - 4.0).abs() < 0.2, "ink ratio {r}");
    }

    #[test]
    fn the_same_text_is_the_same_pixels() {
        let white = Color::rgba(1.0, 1.0, 1.0, 1.0);
        let draw = || {
            let mut c = Canvas::new(120, 30);
            FontText.text(&mut c, Pt { x: 60.0, y: 20.0 }, "Output −6.0 dB", 11.0, white, &Font::DEFAULT);
            c.pixels().to_vec()
        };
        assert_eq!(draw(), draw());
    }
}
