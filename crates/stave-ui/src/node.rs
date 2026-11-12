//! `Node<W>` — the value-type view description (SwiftUI's good part).
//! Composition is FUNCTIONS returning trees; the modifier methods give
//! the `.pad(12.0).radius(6.0).bg(c)` ergonomics without a builder
//! ceremony. `W` is the leaf-widget payload — this crate never learns
//! what a param is.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::style::{Border, Color, Dim, Dir, Edges, Justify, Shadow, Style, TextStyle};

/// Vector geometry — decorative strokes and fills ARE shapes, not
/// styled empty boxes. Sized by the node's dims; `style.background`
/// fills, `style.border` strokes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    /// Rounded rect; radius from `style.radius`.
    RRect,
    Circle,
    /// Full-width/height hairline (1-unit thick unless sized).
    Line,
}

/// THE primitive layer (user-specced): Box, Text, Glyph,
/// Shape. Row/Column/Grid/Stack are Box CONSTRUCTORS (layout modes),
/// and every complex component composes down to these. `Widget` is
/// the acknowledged escape hatch for opaque platform components
/// (knob, plot, spectrum) — the north star is their decomposition
/// into Shape/Glyph/Text as the system matures.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind<W> {
    /// Layout container — children flow per `style.dir`.
    Box,
    /// Glyph runs with typography; measured via `MeasureText`.
    Text(String),
    /// A single named icon; the platform host resolves the name.
    Glyph(&'static str),
    /// Vector geometry (see [`Shape`]).
    Shape(Shape),
    /// Opaque platform-mounted component.
    Widget(W),
    /// Empty flexible space (`Fill` by default) — toolbar springs.
    Spacer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node<W> {
    pub kind: Kind<W>,
    pub style: Style,
    /// Semantic mark (HTML-id-like) for platform behaviors that key
    /// on identity, e.g. `"bypass-dim"` → tick dims this view when
    /// the power param engages. Layout never reads it.
    pub tag: Option<&'static str>,
    /// Conditional visibility: show this subtree only while the keyed
    /// discrete control equals `index` (the tab-page mechanism). The
    /// key is opaque here — the embedder's param id, same layering as
    /// `tag`. Layout still solves the node: hidden is not gone, so
    /// pages overlay in a `Stack` and the window never resizes.
    pub when: Option<(u32, u16)>,
    pub children: Vec<Node<W>>,
}

/// Container constructors — layout modes over the Box primitive.
pub fn boxed<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style::default(),
        tag: None,
        when: None,
        children,
    }
}
pub fn grid<W>(cols: u16, children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style {
            dir: Dir::Grid(cols),
            ..Style::default()
        },
        tag: None,
        when: None,
        children,
    }
}
pub fn glyph<W>(name: &'static str) -> Node<W> {
    Node {
        kind: Kind::Glyph(name),
        style: Style::default(),
        tag: None,
        when: None,
        children: Vec::new(),
    }
}
pub fn shape<W>(s: Shape) -> Node<W> {
    Node {
        kind: Kind::Shape(s),
        style: Style::default(),
        tag: None,
        when: None,
        children: Vec::new(),
    }
}
pub fn row<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style {
            dir: Dir::Row,
            ..Style::default()
        },
        tag: None,
        when: None,
        children,
    }
}
pub fn column<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style {
            dir: Dir::Column,
            ..Style::default()
        },
        tag: None,
        when: None,
        children,
    }
}
pub fn stack<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style {
            dir: Dir::Stack,
            ..Style::default()
        },
        tag: None,
        when: None,
        children,
    }
}
pub fn text<W>(content: impl Into<String>) -> Node<W> {
    Node {
        kind: Kind::Text(content.into()),
        style: Style::default(),
        tag: None,
        when: None,
        children: Vec::new(),
    }
}
pub fn widget<W>(w: W) -> Node<W> {
    Node {
        kind: Kind::Widget(w),
        style: Style::default(),
        tag: None,
        when: None,
        children: Vec::new(),
    }
}
pub fn spacer<W>() -> Node<W> {
    Node {
        kind: Kind::Spacer,
        style: Style {
            width: Dim::Fill(1.0),
            height: Dim::Fill(1.0),
            ..Style::default()
        },
        tag: None,
        when: None,
        children: Vec::new(),
    }
}

impl<W> Node<W> {
    pub fn pad(mut self, v: f64) -> Self {
        self.style.padding = Edges::all(v);
        self
    }
    pub fn pad_edges(mut self, e: Edges) -> Self {
        self.style.padding = e;
        self
    }
    pub fn margin(mut self, v: f64) -> Self {
        self.style.margin = Edges::all(v);
        self
    }
    pub fn margin_edges(mut self, e: Edges) -> Self {
        self.style.margin = e;
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.style.background = Some(c);
        self
    }
    pub fn border(mut self, width: f64, color: Color) -> Self {
        self.style.border = Some(Border { width, color });
        self
    }
    pub fn radius(mut self, r: f64) -> Self {
        self.style.radius = r;
        self
    }
    /// Elevation: `(dy, radius, opacity)` — the theme ELEV tuples.
    pub fn shadow(mut self, dy: f64, radius: f64, opacity: f64) -> Self {
        self.style.shadow = Some(Shadow {
            dy,
            radius,
            opacity,
        });
        self
    }
    pub fn gap(mut self, g: f64) -> Self {
        self.style.gap = g;
        self
    }
    pub fn gap_y(mut self, g: f64) -> Self {
        self.style.gap_y = Some(g);
        self
    }
    /// Tuple forms for markup attrs.
    pub fn border_t(self, b: (f64, Color)) -> Self {
        self.border(b.0, b.1)
    }
    pub fn shadow_t(self, s: (f64, f64, f64)) -> Self {
        self.shadow(s.0, s.1, s.2)
    }
    pub fn width(mut self, d: Dim) -> Self {
        self.style.width = d;
        self
    }
    pub fn height(mut self, d: Dim) -> Self {
        self.style.height = d;
        self
    }
    pub fn size(mut self, w: f64, h: f64) -> Self {
        self.style.width = Dim::Px(w);
        self.style.height = Dim::Px(h);
        self
    }
    pub fn fill(mut self, weight: f64) -> Self {
        self.style.width = Dim::Fill(weight);
        self
    }
    pub fn wrap(mut self) -> Self {
        self.style.wrap = true;
        self
    }
    pub fn justify(mut self, j: Justify) -> Self {
        self.style.justify = j;
        self
    }
    pub fn align(mut self, a: crate::style::Align) -> Self {
        self.style.align = a;
        self
    }
    pub fn tag(mut self, t: &'static str) -> Self {
        self.tag = Some(t);
        self
    }
    /// Raw form of conditional visibility (see the `when` field). The
    /// typed spelling is `show_when(&param, index)` in the embedder's
    /// kit — this crate never learns what a param is.
    pub fn when_key(mut self, key: u32, index: u16) -> Self {
        self.when = Some((key, index));
        self
    }
    /// Set inheritable typography on this subtree.
    pub fn text_style(mut self, t: TextStyle) -> Self {
        self.style.text = Some(t);
        self
    }
}
