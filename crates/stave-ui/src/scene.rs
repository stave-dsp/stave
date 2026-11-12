//! The Scene IR: ONE vector vocabulary under every visual. Parts paint
//! scenes; renderers (stave-raster) consume them; and because a scene is plain
//! data, VISUAL INVARIANTS become derived tests — "the attack knob moves
//! the envelope path's first control point" asserts on ops, no
//! screenshot needed.
//!
//! Kept deliberately small: paths (lines, quads, cubics, circular
//! arcs), solid + axial-gradient brushes, stroked or filled, text
//! runs. Everything an audio UI draws — knobs, rings, envelopes,
//! diagrams, scopes — is these ops. No transforms/layers yet: every
//! scene is in the widget's own coordinate space, and the arc
//! convention deliberately matches CoreGraphics' `CGContextAddArc`
//! (angles in radians, `cw` flag as CG means it in a y-up context)
//! so a CoreGraphics renderer is a 1:1 mapping.

use std::string::String;
use std::vec::Vec;

use crate::style::Color;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

pub const fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

/// One path segment. `Arc` is a circular arc about `c` from `a0` to
/// `a1` radians, direction per `cw` (the CG convention — the
/// renderer passes it straight through).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Seg {
    MoveTo(Pt),
    LineTo(Pt),
    QuadTo(Pt, Pt),
    CubicTo(Pt, Pt, Pt),
    Arc {
        c: Pt,
        r: f64,
        a0: f64,
        a1: f64,
        cw: bool,
    },
    Close,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub segs: Vec<Seg>,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(mut self, p: Pt) -> Self {
        self.segs.push(Seg::MoveTo(p));
        self
    }
    pub fn line_to(mut self, p: Pt) -> Self {
        self.segs.push(Seg::LineTo(p));
        self
    }
    pub fn quad_to(mut self, ctrl: Pt, p: Pt) -> Self {
        self.segs.push(Seg::QuadTo(ctrl, p));
        self
    }
    pub fn cubic_to(mut self, c1: Pt, c2: Pt, p: Pt) -> Self {
        self.segs.push(Seg::CubicTo(c1, c2, p));
        self
    }
    pub fn close(mut self) -> Self {
        self.segs.push(Seg::Close);
        self
    }

    /// A full circle (one closed arc).
    pub fn circle(c: Pt, r: f64) -> Self {
        Self {
            segs: std::vec![Seg::Arc {
                c,
                r,
                a0: 0.0,
                a1: core::f64::consts::TAU,
                cw: false
            }],
        }
    }

    /// An open circular arc (the knob/ring primitive).
    pub fn arc(c: Pt, r: f64, a0: f64, a1: f64, cw: bool) -> Self {
        Self {
            segs: std::vec![Seg::Arc { c, r, a0, a1, cw }],
        }
    }

    /// An open polyline (the scope/spectrum primitive).
    pub fn polyline(pts: &[Pt]) -> Self {
        let mut segs = Vec::with_capacity(pts.len());
        for (i, &p) in pts.iter().enumerate() {
            segs.push(if i == 0 {
                Seg::MoveTo(p)
            } else {
                Seg::LineTo(p)
            });
        }
        Self { segs }
    }

    /// An axis-aligned rectangle.
    pub fn rect(origin: Pt, w: f64, h: f64) -> Self {
        Self::new()
            .move_to(origin)
            .line_to(pt(origin.x + w, origin.y))
            .line_to(pt(origin.x + w, origin.y + h))
            .line_to(pt(origin.x, origin.y + h))
            .close()
    }
}

/// How a fill paints. Axial gradients carry (t, color) stops.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    Linear {
        from: Pt,
        to: Pt,
        stops: Vec<(f64, Color)>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Square, flush with the end.
    Butt,
    /// A half disc past the end.
    Round,
    /// Square, half the width past the end.
    Square,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    pub cap: Cap,
}

impl Stroke {
    pub const fn new(color: Color, width: f64) -> Self {
        Self {
            color,
            width,
            cap: Cap::Butt,
        }
    }
    pub const fn round(color: Color, width: f64) -> Self {
        Self {
            color,
            width,
            cap: Cap::Round,
        }
    }
}

/// One draw operation. Text is part of the vocabulary (displays
/// label their axes); the cocoa renderer implements it
/// with its first consumer — the knob draws no text.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Fill {
        path: Path,
        brush: Brush,
    },
    Stroke {
        path: Path,
        stroke: Stroke,
    },
    /// One line of text. `at` is the anchor on the BASELINE; `font.align`
    /// says whether the line starts, centers or ends there.
    Text {
        at: Pt,
        text: String,
        size: f64,
        color: Color,
        font: Font,
    },
    /// A raster picture drawn into the rect at `at` (its top-left,
    /// y-down) sized `w × h` — for pictures vectors cannot carry
    /// (photographic surfaces, baked lighting). The pixels are shared,
    /// so a build fn can hand back a memoized bitmap every paint.
    Image {
        at: Pt,
        w: f64,
        h: f64,
        image: Image,
    },
}

/// Premultiplied RGBA8 pixels, row 0 at the TOP, `px_w × px_h`. The
/// pixel size is independent of the drawn size, so a picture baked
/// at 2× stays sharp on Retina.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub px_w: u32,
    pub px_h: u32,
    pub rgba: std::sync::Arc<[u8]>,
}

/// A typeface family. Renderers map these onto the platform's system
/// faces (SF Pro, SF Mono, SF Pro Rounded, New York on macOS).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    #[default]
    Sans,
    Mono,
    Rounded,
    Serif,
}

/// Where a text line sits relative to its anchor.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// How a text run is set: family, weight (CSS scale, 100–900), letter
/// tracking in em (0.12 = a tenth-and-a-bit of the size between
/// letters), and alignment on the anchor.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Font {
    pub family: Family,
    pub weight: u16,
    pub tracking: f64,
    pub align: TextAlign,
}

impl Font {
    pub const DEFAULT: Font = Font {
        family: Family::Sans,
        weight: 600,
        tracking: 0.0,
        align: TextAlign::Center,
    };
    pub const fn new(family: Family, weight: u16) -> Self {
        Font {
            family,
            weight,
            tracking: 0.0,
            align: TextAlign::Center,
        }
    }
    pub const fn tracking(mut self, em: f64) -> Self {
        self.tracking = em;
        self
    }
    pub const fn align(mut self, a: TextAlign) -> Self {
        self.align = a;
        self
    }
}

impl Default for Font {
    fn default() -> Self {
        Font::DEFAULT
    }
}

/// A widget's whole picture: ops in paint order, in the widget's
/// own coordinate space. Plain data — comparable, property-testable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub ops: Vec<Op>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fill(&mut self, path: Path, brush: Brush) {
        self.ops.push(Op::Fill { path, brush });
    }
    pub fn solid(&mut self, path: Path, color: Color) {
        self.ops.push(Op::Fill {
            path,
            brush: Brush::Solid(color),
        });
    }
    pub fn stroke(&mut self, path: Path, stroke: Stroke) {
        self.ops.push(Op::Stroke { path, stroke });
    }
    /// One line of text in the default font (centered on `at`).
    pub fn text(&mut self, at: Pt, text: impl Into<String>, size: f64, color: Color) {
        self.text_in(at, text, size, color, Font::DEFAULT);
    }
    /// One line of text, set in `font`.
    pub fn text_in(&mut self, at: Pt, text: impl Into<String>, size: f64, color: Color, font: Font) {
        self.ops.push(Op::Text {
            at,
            text: text.into(),
            size,
            color,
            font,
        });
    }
}
