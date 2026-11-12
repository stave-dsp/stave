//! The box-model vocabulary — CSS's good parts, value-typed. Points
//! everywhere (backing scale is the backend's business, per the
//! HiDPI plan).

/// RGBA in 0..=1 — plain data; tokens live in the theme layer, which
/// resolves to these before the tree is handed to a backend.
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f64,
    pub color: Color,
}

/// Drop shadow — PAINT only, never layout (elevation tokens map here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub dy: f64,
    pub radius: f64,
    pub opacity: f64,
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

/// Type roles — the theme's named text intents (display, label, tick,
/// value…). The backend maps a role to a concrete font; `TextStyle`
/// carries the resolved knobs the solver + painter need.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TypeRole {
    Display,
    #[default]
    Label,
    Tick,
    Value,
    Caption,
    /// Card/section title chrome — heavy weight, wide kern.
    Overlay,
}

/// Resolved typography. INHERITS down the tree CSS-style: a container
/// may set `text`, and descendants without their own resolve through
/// the nearest ancestor (see `layout::solve`'s inherited context).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub role: TypeRole,
    pub size: f64,
    pub kern: f64,
    pub color: Color,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            role: TypeRole::Label,
            size: 11.0,
            kern: 0.0,
            color: Color::rgba(1.0, 1.0, 1.0, 0.85),
        }
    }
}

/// The per-node style. Everything a node can say about itself; what
/// it does NOT say inherits (text) or defaults (everything else).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Style {
    pub padding: Edges,
    pub margin: Edges,
    pub border: Option<Border>,
    pub shadow: Option<Shadow>,
    pub radius: f64,
    pub background: Option<Color>,
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
    /// Container-level text defaults inherited by descendants.
    pub text: Option<TextStyle>,
}
