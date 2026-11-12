//! The OS-agnostic look layer: the layout tree and its flexbox solver,
//! and the Scene IR every picture is made of. Everything here is PURE —
//! `solve()` maps a Node tree + available size to rectangles, a
//! `Scene` is plain data — so both are tested headless on any OS.
//!
//! `Node<W>` is GENERIC over the leaf payload so this crate knows
//! nothing about params, parts or plugins — stave-face instantiates
//! `W` with its parts.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod layout;
pub mod node;
pub mod scene;
pub mod style;

pub use layout::{solve, Fonts, Layout, Measure, Unmeasured};
pub use node::{boxed, column, grid, row, spacer, stack, widget, Kind, Node};
pub use style::{Align, Color, Dim, Dir, Edges, Justify, Style};
