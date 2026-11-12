//! The flexbox solver — PURE: `solve(tree, avail, measure)` → rects.
//! No OS, no side effects; snapshot tests pin the geometry headless.
//!
//! A leaf whose width or height is `Auto` takes its natural size from
//! the [`Measure`] (a title measures its text in the platform's
//! [`Fonts`]); `Px` overrides it, and under `Fill` it is the floor.
//!
//! Model (the CSS subset a face needs, nothing more):
//!  - A node's rect EXCLUDES its margin (margin is spacing the parent
//!    consumes); border + padding inset the content box (border-box).
//!  - Main axis from `dir` (Row = x, Column = y; Stack overlays).
//!  - `Dim`: Px fixed; Pct of parent content; Auto intrinsic;
//!    Fill(weight) shares the remaining main-axis space — on the
//!    cross axis Fill means stretch. Fill inside a WRAPPING container
//!    is treated as Auto (lines have no stable remainder).
//!  - `justify` places the line along main; `align` places children
//!    across (Stretch grows Auto-cross children to the line extent).

use std::collections::HashMap;
use std::vec::Vec;

use crate::node::{Kind, Node};
use crate::scene::Font;
use crate::style::{Align, Dim, Dir, Justify};

/// The platform's fonts, as layout sees them: how wide a line of text
/// sets. The platform that paints the text supplies it (CoreText on
/// macOS; `stave_raster::BoxText` for tests and golden images), so what
/// layout measures is what the painter draws.
pub trait Fonts {
    /// The width of `text` set in `font` at `size` points, in points.
    fn width(&self, text: &str, size: f64, font: &Font) -> f64;
}

/// What layout asks of the leaves: each one's natural size (w, h) in
/// points, for the axes its style leaves `Auto` (and as the floor under
/// `Fill`). Any `Fn(&W) -> (f64, f64)` is one.
pub trait Measure<W> {
    fn size(&self, leaf: &W) -> (f64, f64);
}

impl<W, F: Fn(&W) -> (f64, f64)> Measure<W> for F {
    fn size(&self, leaf: &W) -> (f64, f64) {
        self(leaf)
    }
}

/// Leaves with no natural size: every leaf is sized by its style alone.
pub struct Unmeasured;

impl<W> Measure<W> for Unmeasured {
    fn size(&self, _leaf: &W) -> (f64, f64) {
        (0.0, 0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Solved geometry, parallel to the Node tree (child i ↔ child i).
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub rect: Rect,
    pub children: Vec<Layout>,
}

/// Lay `node` out in `avail` points, asking `measure` for the natural
/// size of each leaf.
///
/// Each leaf is measured once, and each node's size at a given available
/// space is computed once and remembered: the work is linear in the
/// leaves and polynomial in the depth. (Sizing a box asks its children
/// twice, for its width and then for its height at that width, and
/// placing asks again; without the memo that was exponential in the
/// nesting depth.)
pub fn solve<W>(node: &Node<W>, avail: (f64, f64), measure: &dyn Measure<W>) -> Layout {
    Solver::new(measure).solve(node, avail)
}

/// A node asked at an available space (its bits): what the size memo
/// is keyed by.
type Asked<W> = (*const Node<W>, u64, u64);

/// One solve's memo: each leaf's natural size, and each node's size at
/// each available space it is asked at. A size is a pure function of the
/// node and the space, so the memo changes the cost and nothing else.
struct Solver<'m, W> {
    m: &'m dyn Measure<W>,
    leaves: HashMap<*const Node<W>, (f64, f64)>,
    sizes: HashMap<Asked<W>, (f64, f64)>,
    /// Sizes computed (memo misses), for the complexity tests.
    #[cfg(test)]
    computed: usize,
}

impl<'m, W> Solver<'m, W> {
    fn new(m: &'m dyn Measure<W>) -> Self {
        Solver {
            m,
            leaves: HashMap::new(),
            sizes: HashMap::new(),
            #[cfg(test)]
            computed: 0,
        }
    }

    fn solve(&mut self, node: &Node<W>, avail: (f64, f64)) -> Layout {
        let (w, h) = self.outer_size(node, avail);
        self.place(
            node,
            Rect {
                x: node.style.margin.l,
                y: node.style.margin.t,
                w,
                h,
            },
        )
    }

    /// A leaf's natural size, measured once per solve.
    fn leaf_size(&mut self, node: &Node<W>, leaf: &W) -> (f64, f64) {
        let m = self.m;
        *self.leaves.entry(core::ptr::from_ref(node)).or_insert_with(|| m.size(leaf))
    }

    /// The node's size EXCLUDING margin, given the parent's content box
    /// (memoized [`compute_size`](Self::compute_size)).
    fn outer_size(&mut self, node: &Node<W>, avail: (f64, f64)) -> (f64, f64) {
        let key = (core::ptr::from_ref(node), avail.0.to_bits(), avail.1.to_bits());
        if let Some(&s) = self.sizes.get(&key) {
            return s;
        }
        #[cfg(test)]
        {
            self.computed += 1;
        }
        let s = self.compute_size(node, avail);
        self.sizes.insert(key, s);
        s
    }

    /// The node's size EXCLUDING margin, given the parent's content box.
    fn compute_size(&mut self, node: &Node<W>, avail: (f64, f64)) -> (f64, f64) {
        // `intrinsic` is the content's size, computed only for the
        // dims that read it (Fill and Auto).
        let reads = |d: Dim| matches!(d, Dim::Fill(_) | Dim::Auto);
        let dim = |d: Dim, avail_axis: f64, intrinsic: f64| -> f64 {
            match d {
                Dim::Px(v) => v,
                Dim::Pct(f) => f * avail_axis,
                // At least the content (min-content), else the space given;
                // flow parents override with basis + share.
                Dim::Fill(_) => avail_axis.max(intrinsic),
                Dim::Auto => intrinsic,
            }
        };
        let (it, ir, ib, il) = inset(node);
        match &node.kind {
            Kind::Widget(leaf) => {
                let auto = matches!(node.style.width, Dim::Auto | Dim::Fill(_))
                    || matches!(node.style.height, Dim::Auto | Dim::Fill(_));
                let (nw, nh) = if auto { self.leaf_size(node, leaf) } else { (0.0, 0.0) };
                (dim(node.style.width, avail.0, nw), dim(node.style.height, avail.1, nh))
            }
            Kind::Spacer => (dim(node.style.width, avail.0, 0.0), dim(node.style.height, avail.1, 0.0)),
            Kind::Box => {
                let content_avail = ((avail.0 - il - ir).max(0.0), (avail.1 - it - ib).max(0.0));
                let iw = if reads(node.style.width) { self.content_size(node, content_avail).0 + il + ir } else { 0.0 };
                let w = dim(node.style.width, avail.0, iw);
                // Height flows at the RESOLVED width: a fixed-width box
                // wraps inside its own width, not the parent's avail.
                let h_avail = ((w - il - ir).max(0.0), content_avail.1);
                let ih = if reads(node.style.height) { self.content_size(node, h_avail).1 + it + ib } else { 0.0 };
                let h = dim(node.style.height, avail.1, ih);
                (w, h)
            }
        }
    }

    /// Natural content extent of a Box (children flowed at Auto).
    fn content_size(&mut self, node: &Node<W>, avail: (f64, f64)) -> (f64, f64) {
        let gap = node.style.gap_y.unwrap_or(node.style.gap);
        match node.style.dir {
            Dir::Grid(cols) => {
                let cols = cols.max(1) as usize;
                let mut col_w = 0.0f64;
                let mut h = 0.0f64;
                let mut row_h = 0.0f64;
                for (i, c) in node.children.iter().enumerate() {
                    let (cw, ch) = self.outer_size(c, avail);
                    col_w = col_w.max(cw + c.style.margin.h());
                    row_h = row_h.max(ch + c.style.margin.v());
                    if (i + 1) % cols == 0 || i + 1 == node.children.len() {
                        h += row_h + if i >= cols { gap } else { 0.0 };
                        row_h = 0.0;
                    }
                }
                (
                    col_w * cols as f64 + node.style.gap * (cols.saturating_sub(1)) as f64,
                    h,
                )
            }
            Dir::Stack => {
                let mut w = 0.0f64;
                let mut h = 0.0f64;
                // A stack sizes to its content: a Fill child stretches to
                // the stack, it does not size it.
                for c in &node.children {
                    let (cw, ch) = self.outer_size(c, avail);
                    // A Fill child counts with its min-content only.
                    let (mw, mh) = self.outer_size(c, (0.0, 0.0));
                    let cw = if matches!(c.style.width, Dim::Fill(_)) { mw } else { cw };
                    let ch = if matches!(c.style.height, Dim::Fill(_)) { mh } else { ch };
                    w = w.max(cw + c.style.margin.h());
                    h = h.max(ch + c.style.margin.v());
                }
                (w, h)
            }
            Dir::Row => {
                let mut w = 0.0f64;
                let mut h = 0.0f64;
                for (li, line) in self.flow_lines(node, avail).iter().enumerate() {
                    w = w.max(line.main);
                    h += line.cross + if li > 0 { gap } else { 0.0 };
                }
                (w, h)
            }
            Dir::Column => {
                let mut w = 0.0f64;
                let mut h = 0.0f64;
                for (li, line) in self.flow_lines(node, avail).iter().enumerate() {
                    h = h.max(line.main);
                    w += line.cross + if li > 0 { gap } else { 0.0 };
                }
                (w, h)
            }
        }
    }

    /// Greedy line filler. Non-wrapping containers yield one line.
    fn flow_lines(&mut self, node: &Node<W>, avail: (f64, f64)) -> Vec<Line> {
        let row = node.style.dir != Dir::Column;
        let gap = node.style.gap;
        let main_avail = if row { avail.0 } else { avail.1 };
        let mut lines: Vec<Line> = Vec::new();
        let mut cur = Line {
            idx: Vec::new(),
            main: 0.0,
            cross: 0.0,
            fill: 0.0,
        };
        for (i, c) in node.children.iter().enumerate() {
            let (cw, ch) = self.outer_size(c, avail);
            let (cmain, ccross) = if row {
                (cw + c.style.margin.h(), ch + c.style.margin.v())
            } else {
                (ch + c.style.margin.v(), cw + c.style.margin.h())
            };
            let main_dim = if row { c.style.width } else { c.style.height };
            let is_fill = matches!(main_dim, Dim::Fill(_)) && !node.style.wrap;
            let add = if cur.idx.is_empty() {
                cmain
            } else {
                cmain + gap
            };
            if node.style.wrap && !cur.idx.is_empty() && cur.main + add > main_avail {
                lines.push(cur);
                cur = Line {
                    idx: Vec::new(),
                    main: 0.0,
                    cross: 0.0,
                    fill: 0.0,
                };
                cur.main = cmain;
            } else {
                cur.main += add;
            }
            if is_fill {
                if let Dim::Fill(wt) = main_dim {
                    cur.fill += wt.max(0.0);
                }
                // Fill children contribute their min-content as a basis;
                // the line's slack is shared out on top of it.
                let (mw, mh) = self.outer_size(c, (0.0, 0.0));
                let basis = if row { mw + c.style.margin.h() } else { mh + c.style.margin.v() };
                cur.main += basis - cmain;
            }
            // A Fill on the cross axis stretches to the line; it never
            // sizes it (a spacer in a row must not make the row tall).
            let cross_dim = if row { c.style.height } else { c.style.width };
            if matches!(cross_dim, Dim::Fill(_)) {
                // Counts with its min-content only.
                let (mw, mh) = self.outer_size(c, (0.0, 0.0));
                let min = if row { mh + c.style.margin.v() } else { mw + c.style.margin.h() };
                cur.cross = cur.cross.max(min);
            } else {
                cur.cross = cur.cross.max(ccross);
            }
            cur.idx.push(i);
        }
        if !cur.idx.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
        lines
    }

    fn place(&mut self, node: &Node<W>, rect: Rect) -> Layout {
        let (it, ir, ib, il) = inset(node);
        let content = Rect {
            x: rect.x + il,
            y: rect.y + it,
            w: (rect.w - il - ir).max(0.0),
            h: (rect.h - it - ib).max(0.0),
        };
        let mut children = Vec::with_capacity(node.children.len());
        match node.style.dir {
            Dir::Grid(cols) => {
                let cols = cols.max(1) as usize;
                let gap = node.style.gap;
                let gap_row = node.style.gap_y.unwrap_or(gap);
                let col_w = (content.w - gap * (cols.saturating_sub(1)) as f64) / cols as f64;
                let mut y = 0.0f64;
                let mut row_h = 0.0f64;
                for (i, c) in node.children.iter().enumerate() {
                    let col = i % cols;
                    let (cw, ch) = self.child_final_size(c, (col_w, content.h), None, None);
                    let cw = cw.min((col_w - c.style.margin.h()).max(0.0));
                    let x = content.x + col as f64 * (col_w + gap) + c.style.margin.l;
                    children.push(self.place(
                        c,
                        Rect {
                            x,
                            y: content.y + y + c.style.margin.t,
                            w: cw,
                            h: ch,
                        },
                    ));
                    row_h = row_h.max(ch + c.style.margin.v());
                    if col + 1 == cols || i + 1 == node.children.len() {
                        y += row_h + gap_row;
                        row_h = 0.0;
                    }
                }
            }
            Dir::Stack => {
                for c in &node.children {
                    let (cw, ch) = self.child_final_size(c, (content.w, content.h), None, None);
                    let free_x = content.w - cw - c.style.margin.h();
                    let free_y = content.h - ch - c.style.margin.v();
                    let x = content.x
                        + c.style.margin.l
                        + match node.style.justify {
                            Justify::Start | Justify::SpaceBetween => 0.0,
                            Justify::Center => free_x * 0.5,
                            Justify::End => free_x,
                        };
                    let y = content.y
                        + c.style.margin.t
                        + match node.style.align {
                            Align::Start | Align::Stretch => 0.0,
                            Align::Center => free_y * 0.5,
                            Align::End => free_y,
                        };
                    children.push(self.place(c, Rect { x, y, w: cw, h: ch }));
                }
            }
            dir => {
                let row = dir == Dir::Row;
                let gap = node.style.gap;
                let line_gap = node.style.gap_y.unwrap_or(gap);
                let lines = self.flow_lines(node, (content.w, content.h));
                let main_avail = if row { content.w } else { content.h };
                let single = !node.style.wrap && lines.len() == 1;
                let mut cross_pos = 0.0f64;
                for (li, line) in lines.iter().enumerate() {
                    if li > 0 {
                        cross_pos += line_gap;
                    }
                    // A lone line spans the container cross (CSS
                    // align-content: stretch), so align works against
                    // the box, not the line's natural extent.
                    let line = &Line {
                        idx: line.idx.clone(),
                        main: line.main,
                        cross: if single {
                            (if row { content.h } else { content.w }).max(line.cross)
                        } else {
                            line.cross
                        },
                        fill: line.fill,
                    };
                    let remainder = (main_avail - line.main).max(0.0);
                    let fill_unit = if line.fill > 0.0 {
                        remainder / line.fill
                    } else {
                        0.0
                    };
                    let extra = if line.fill > 0.0 { 0.0 } else { remainder };
                    let n = line.idx.len();
                    let (mut main_pos, between) = match node.style.justify {
                        Justify::Start => (0.0, 0.0),
                        Justify::Center => (extra * 0.5, 0.0),
                        Justify::End => (extra, 0.0),
                        Justify::SpaceBetween if n > 1 => (0.0, extra / (n - 1) as f64),
                        Justify::SpaceBetween => (0.0, 0.0),
                    };
                    for (k, &ci) in line.idx.iter().enumerate() {
                        let c = &node.children[ci];
                        if k > 0 {
                            main_pos += gap + between;
                        }
                        let (cw, ch) = self.child_final_size(
                            c,
                            (content.w, content.h),
                            Some((row, fill_unit)),
                            Some((node.style.align, line.cross)),
                        );
                        let (cmain, ccross) = if row {
                            (cw + c.style.margin.h(), ch + c.style.margin.v())
                        } else {
                            (ch + c.style.margin.v(), cw + c.style.margin.h())
                        };
                        let cross_free = line.cross - ccross;
                        let cross_off = match node.style.align {
                            Align::Start | Align::Stretch => 0.0,
                            Align::Center => cross_free * 0.5,
                            Align::End => cross_free,
                        };
                        let (x, y) = if row {
                            (
                                content.x + main_pos + c.style.margin.l,
                                content.y + cross_pos + cross_off + c.style.margin.t,
                            )
                        } else {
                            (
                                content.x + cross_pos + cross_off + c.style.margin.l,
                                content.y + main_pos + c.style.margin.t,
                            )
                        };
                        children.push(self.place(c, Rect { x, y, w: cw, h: ch }));
                        main_pos += cmain;
                    }
                    cross_pos += line.cross;
                }
            }
        }
        Layout { rect, children }
    }

    /// Final size of a flow child: Fill resolved against the line
    /// remainder on main; Stretch grows Auto-cross to the line extent.
    fn child_final_size(
        &mut self,
        c: &Node<W>,
        avail: (f64, f64),
        fill_ctx: Option<(bool, f64)>,
        align_ctx: Option<(Align, f64)>,
    ) -> (f64, f64) {
        let (mut cw, mut ch) = self.outer_size(c, avail);
        if let Some((row, unit)) = fill_ctx {
            let (mw, mh) = self.outer_size(c, (0.0, 0.0));
            if row {
                if let Dim::Fill(wt) = c.style.width {
                    cw = (mw + unit * wt.max(0.0)).max(0.0);
                }
            } else if let Dim::Fill(wt) = c.style.height {
                ch = (mh + unit * wt.max(0.0)).max(0.0);
            }
        } else {
            // Stack: Fill means the whole content box.
            if let Dim::Fill(_) = c.style.width {
                cw = (avail.0 - c.style.margin.h()).max(0.0);
            }
            if let Dim::Fill(_) = c.style.height {
                ch = (avail.1 - c.style.margin.v()).max(0.0);
            }
        }
        if let Some((align, line_cross)) = align_ctx {
            if align == Align::Stretch {
                if let Some((row, _)) = fill_ctx {
                    if row && c.style.height == Dim::Auto {
                        ch = (line_cross - c.style.margin.v()).max(0.0);
                    }
                    if !row && c.style.width == Dim::Auto {
                        cw = (line_cross - c.style.margin.h()).max(0.0);
                    }
                }
            }
        }
        (cw, ch)
    }
}

/// Content inset: the padding.
fn inset<W>(node: &Node<W>) -> (f64, f64, f64, f64) {
    let p = node.style.padding;
    (p.t, p.r, p.b, p.l)
}

struct Line {
    /// Child indices in this line.
    idx: Vec<usize>,
    /// Sum of outer main extents + gaps.
    main: f64,
    /// Max outer cross extent.
    cross: f64,
    /// Total Fill weight on the main axis (0 in wrapping containers).
    fill: f64,
}


#[cfg(test)]
mod tests {
    use super::Solver;
    use crate::node::{column, row, spacer, stack, widget, Node};
    use crate::style::Dim;

    /// A face-like tree `depth` levels deep: at each level a padded box
    /// (alternating row and column, a stack every third) holding a leaf,
    /// a spacer, a Fill column and the next level. (The same tree as
    /// `tests/layout_complexity.rs`.)
    fn nested(depth: u32) -> Node<u32> {
        let mut inner = widget(depth).width(Dim::Fill(1.0));
        for d in (0..depth).rev() {
            let kids = vec![
                widget(d),
                spacer(),
                column(vec![widget(d + 100), widget(d + 200)]).width(Dim::Fill(1.0)).gap(2.0),
                inner,
            ];
            inner = match d % 3 {
                0 => row(kids).gap(4.0),
                1 => column(kids).gap(3.0),
                _ => stack(kids),
            }
            .pad(2.0)
            .margin(1.0);
        }
        inner
    }

    /// The work is polynomial in the depth, not exponential: doubling
    /// the depth of the tree multiplies the sizes computed (memo misses)
    /// by at most ten. (Each level adds a few distinct spaces its subtree
    /// is asked at, so it is about cubic in the depth; before the memo,
    /// every level doubled it.)
    #[test]
    fn a_solve_is_polynomial_in_depth() {
        let computed = |depth: u32| {
            let tree = nested(depth);
            let m = |leaf: &u32| (20.0 + f64::from(*leaf % 7), 10.0 + f64::from(*leaf % 5));
            let mut s = Solver::new(&m);
            let _ = s.solve(&tree, (900.0, 700.0));
            s.computed
        };
        for depth in [8, 16, 32] {
            let (a, b) = (computed(depth), computed(2 * depth));
            assert!(b <= 10 * a, "depth {depth}: {a} sizes; depth {}: {b}", 2 * depth);
        }
    }
}
