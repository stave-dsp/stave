//! `Node<W>` — a layout tree. Composition is functions returning trees;
//! the modifier methods give `.pad(12.0).gap(8.0)` ergonomics without a
//! builder ceremony. `W` is the leaf payload (a face's part) — this
//! crate never learns what a param is. Nodes carry layout only: what a
//! leaf looks like is the leaf's business.

extern crate alloc;
use alloc::vec::Vec;

use crate::style::{Dim, Dir, Edges, Justify, Style};

/// What a node is: a layout container, a leaf, or flexible space.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind<W> {
    /// Layout container — children flow per `style.dir`.
    Box,
    /// A leaf: the embedder's payload (a face's part).
    Widget(W),
    /// Empty flexible space (`Fill` by default) — toolbar springs.
    Spacer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node<W> {
    pub kind: Kind<W>,
    pub style: Style,
    /// Conditional visibility: show this subtree only while page set
    /// `key` shows `index` (a face's pages). The key is opaque here.
    /// Layout still solves the node: hidden is not gone, so pages
    /// overlay in a `Stack` and the window never resizes.
    pub when: Option<(u32, u16)>,
    pub children: Vec<Node<W>>,
}

/// Container constructors — layout modes over the Box primitive.
pub fn boxed<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style::default(),
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
        when: None,
        children,
    }
}
pub fn row<W>(children: Vec<Node<W>>) -> Node<W> {
    Node {
        kind: Kind::Box,
        style: Style {
            dir: Dir::Row,
            ..Style::default()
        },
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
        when: None,
        children,
    }
}
pub fn widget<W>(w: W) -> Node<W> {
    Node {
        kind: Kind::Widget(w),
        style: Style::default(),
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
    pub fn gap(mut self, g: f64) -> Self {
        self.style.gap = g;
        self
    }
    pub fn gap_y(mut self, g: f64) -> Self {
        self.style.gap_y = Some(g);
        self
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
    /// Conditional visibility (see the `when` field).
    pub fn when_key(mut self, key: u32, index: u16) -> Self {
        self.when = Some((key, index));
        self
    }
}
