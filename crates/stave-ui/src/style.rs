//! The box-model vocabulary — CSS's layout subset, value-typed — and
//! the colour the Scene IR paints with. Points everywhere (backing
//! scale is the renderer's business).

/// RGBA in 0..=1 — plain data; a face's theme decides which.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }
}

/// Per-side lengths (top, right, bottom, left — CSS order).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Edges {
    pub t: f64,
    pub r: f64,
    pub b: f64,
    pub l: f64,
}

impl Edges {
    pub const ZERO: Edges = Edges {
        t: 0.0,
        r: 0.0,
        b: 0.0,
        l: 0.0,
    };
    pub const fn all(v: f64) -> Self {
        Self {
            t: v,
            r: v,
            b: v,
            l: v,
        }
    }
    pub const fn xy(x: f64, y: f64) -> Self {
        Self {
            t: y,
            r: x,
            b: y,
            l: x,
        }
    }
    pub fn h(&self) -> f64 {
        self.l + self.r
    }
    pub fn v(&self) -> f64 {
        self.t + self.b
    }
}

/// One dimension of a node's declared size.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Dim {
    /// Intrinsic: leaves from their measure, containers from content.
    #[default]
    Auto,
    /// Fixed points.
    Px(f64),
    /// Share of the REMAINING main-axis space (weight); on the cross
    /// axis any Fill means stretch.
    Fill(f64),
    /// Fraction of the parent's content box (0..=1).
    Pct(f64),
}

/// Main-axis flow of a container's children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dir {
    #[default]
    Row,
    Column,
    /// Children stack on top of each other in the content box
    /// (overlays: meters over rails, captions over plots).
    Stack,
    /// Row-major grid with N equal columns; row height = tallest
    /// cell in the row; `gap` applies on both axes.
    Grid(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    /// Equal gaps BETWEEN children (first/last flush) — the crossover
    /// footer / toolbar pattern.
    SpaceBetween,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// The per-node layout. Everything a node can say about its box; what
/// it does not say defaults.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Style {
    pub padding: Edges,
    pub margin: Edges,
    /// Gap between children along the main axis (and between wrapped
    /// rows on the cross axis).
    pub gap: f64,
    /// Grid/wrap cross-axis gap override (None = `gap`).
    pub gap_y: Option<f64>,
    pub width: Dim,
    pub height: Dim,
    pub dir: Dir,
    pub wrap: bool,
    pub justify: Justify,
    pub align: Align,
}
