//! The OS-agnostic view layer: box model,
//! flexbox solver, style + typography types. Everything here is PURE —
//! `solve()` maps a Node tree + available size to rectangles, nothing
//! else — so layout is snapshot-tested headless in CI on any OS.
//!
//! Platform seams (the ONLY places an OS appears, both traits):
//!  - [`MeasureText`] — font metrics. The Cocoa backend measures with
//!    NSFont/CoreText; tests and headless tools use [`FixedMetrics`].
//!  - the widget host (7.5b, backend crates) — mounts leaf widgets and
//!    paints Box/Text primitives from solved rects. macOS implements
//!    it with NSView/CoreGraphics; a Windows/Linux (or wgpu paint-
//!    everything) backend implements the same trait.
//!
//! `Node<W>` is GENERIC over the leaf widget payload so this crate
//! knows nothing about params, probes, or plugins — stave-core
//! instantiates `W` with its ParamId-carrying widget enum.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod layout;
pub mod node;
pub mod photo;
pub mod scene;
pub mod scenes;
pub mod signal;
pub mod style;
pub mod tokens;

pub use layout::{solve, Layout, MeasureText};
pub use node::{
    boxed, column, glyph, grid, row, shape, spacer, stack, text, widget, Kind, Node, Shape,
};
pub use stave_macros::ui;
pub use style::{
    Align, Border, Color, Dim, Dir, Edges, Justify, Shadow, Style, TextStyle, TypeRole,
};

/// Fixed-metrics text measurer for tests and headless layout: every
/// glyph advances `advance × size`, line height is `line × size`.
/// Deterministic on every OS — the snapshot tests' ruler.
#[derive(Clone, Copy, Debug)]
pub struct FixedMetrics {
    pub advance: f64,
    pub line: f64,
}

impl Default for FixedMetrics {
    fn default() -> Self {
        Self {
            advance: 0.6,
            line: 1.25,
        }
    }
}

impl MeasureText for FixedMetrics {
    fn measure(&self, text: &str, style: &TextStyle) -> (f64, f64) {
        let size = style.size;
        (
            text.chars().count() as f64 * self.advance * size
                + style.kern * text.chars().count().saturating_sub(1) as f64,
            self.line * size,
        )
    }
}
