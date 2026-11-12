//! The flexbox solver — PURE: `solve(tree, avail, measure)` → rects.
//! No OS, no side effects; snapshot tests pin the geometry headless.
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

extern crate alloc;
use alloc::vec::Vec;

use crate::node::{Kind, Node};
use crate::style::{Align, Dim, Dir, Justify};

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

pub fn solve<W>(node: &Node<W>, avail: (f64, f64)) -> Layout {
    let (w, h) = outer_size(node, avail);
    place(
        node,
        Rect {
            x: node.style.margin.l,
            y: node.style.margin.t,
            w,
            h,
        },
    )
}

/// Content inset: the padding.
fn inset<W>(node: &Node<W>) -> (f64, f64, f64, f64) {
    let p = node.style.padding;
    (p.t, p.r, p.b, p.l)
}

/// The node's size EXCLUDING margin, given the parent's content box.
fn outer_size<W>(
    node: &Node<W>,
    avail: (f64, f64),
) -> (f64, f64) {
    let dim = |d: Dim, avail_axis: f64, intrinsic: &dyn Fn() -> f64| -> f64 {
        match d {
            Dim::Px(v) => v,
            Dim::Pct(f) => f * avail_axis,
            // At least the content (min-content), else the space given;
            // flow parents override with basis + share.
            Dim::Fill(_) => avail_axis.max(intrinsic()),
            Dim::Auto => intrinsic(),
        }
    };
    let (it, ir, ib, il) = inset(node);
    match &node.kind {
        Kind::Widget(_) | Kind::Spacer => (
            dim(node.style.width, avail.0, &|| 0.0),
            dim(node.style.height, avail.1, &|| 0.0),
        ),
        Kind::Box => {
            let content_avail = ((avail.0 - il - ir).max(0.0), (avail.1 - it - ib).max(0.0));
            let w = dim(node.style.width, avail.0, &|| {
                content_size(node, content_avail).0 + il + ir
            });
            // Height flows at the RESOLVED width: a fixed-width box
            // wraps inside its own width, not the parent's avail.
            let h_avail = ((w - il - ir).max(0.0), content_avail.1);
            let h = dim(node.style.height, avail.1, &|| {
                content_size(node, h_avail).1 + it + ib
            });
            (w, h)
        }
    }
}

/// Natural content extent of a Box (children flowed at Auto).
fn content_size<W>(
    node: &Node<W>,
    avail: (f64, f64),
) -> (f64, f64) {
    let gap = node.style.gap_y.unwrap_or(node.style.gap);
    let lines = flow_lines(node, avail);
    match node.style.dir {
        Dir::Grid(cols) => {
            let cols = cols.max(1) as usize;
            let mut col_w = 0.0f64;
            let mut h = 0.0f64;
            let mut row_h = 0.0f64;
            for (i, c) in node.children.iter().enumerate() {
                let (cw, ch) = outer_size(c, avail);
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
                let (cw, ch) = outer_size(c, avail);
                // A Fill child counts with its min-content only.
                let (mw, mh) = outer_size(c, (0.0, 0.0));
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
            for (li, line) in lines.iter().enumerate() {
                w = w.max(line.main);
                h += line.cross + if li > 0 { gap } else { 0.0 };
            }
            (w, h)
        }
        Dir::Column => {
            let mut w = 0.0f64;
            let mut h = 0.0f64;
            for (li, line) in lines.iter().enumerate() {
                h = h.max(line.main);
                w += line.cross + if li > 0 { gap } else { 0.0 };
            }
            (w, h)
        }
    }
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

/// Greedy line filler. Non-wrapping containers yield one line.
fn flow_lines<W>(
    node: &Node<W>,
    avail: (f64, f64),
) -> Vec<Line> {
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
        let (cw, ch) = outer_size(c, avail);
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
            let (mw, mh) = outer_size(c, (0.0, 0.0));
            let basis = if row { mw + c.style.margin.h() } else { mh + c.style.margin.v() };
            cur.main += basis - cmain;
        }
        // A Fill on the cross axis stretches to the line; it never
        // sizes it (a spacer in a row must not make the row tall).
        let cross_dim = if row { c.style.height } else { c.style.width };
        if matches!(cross_dim, Dim::Fill(_)) {
            // Counts with its min-content only.
            let (mw, mh) = outer_size(c, (0.0, 0.0));
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

fn place<W>(node: &Node<W>, rect: Rect) -> Layout {
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
                let (cw, ch) = child_final_size(c, (col_w, content.h), None, None);
                let cw = cw.min((col_w - c.style.margin.h()).max(0.0));
                let x = content.x + col as f64 * (col_w + gap) + c.style.margin.l;
                children.push(place(
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
                let (cw, ch) = child_final_size(c, (content.w, content.h), None, None);
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
                children.push(place(c, Rect { x, y, w: cw, h: ch }));
            }
        }
        dir => {
            let row = dir == Dir::Row;
            let gap = node.style.gap;
            let line_gap = node.style.gap_y.unwrap_or(gap);
            let lines = flow_lines(node, (content.w, content.h));
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
                    let (cw, ch) = child_final_size(
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
                    children.push(place(c, Rect { x, y, w: cw, h: ch }));
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
fn child_final_size<W>(
    c: &Node<W>,
    avail: (f64, f64),
    fill_ctx: Option<(bool, f64)>,
    align_ctx: Option<(Align, f64)>,
) -> (f64, f64) {
    let (mut cw, mut ch) = outer_size(c, avail);
    if let Some((row, unit)) = fill_ctx {
        let (mw, mh) = outer_size(c, (0.0, 0.0));
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
