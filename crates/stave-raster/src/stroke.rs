//! Stroking: a flattened polyline at a width into polygons the coverage
//! pass can fill. Each segment becomes a quad; joins are MITERED with a
//! miter limit of 10, beyond which they bevel (CoreGraphics' defaults —
//! the join every Stave scene has drawn with); caps are butt or round.
//! Every polygon is wound the SAME way, so the nonzero union of
//! overlapping pieces is exact.

use std::vec::Vec;

use core::f64::consts::TAU;

use stave_ui::scene::{Cap, Pt};

use crate::path::Poly;

/// Signed area ×2 (positive = clockwise in y-down screen space).
fn area2(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x * v.y - v.x * u.y;
    }
    a
}

/// Wind `p` positively (in place).
fn orient(mut p: Vec<Pt>) -> Vec<Pt> {
    if area2(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// A disc as a polygon fine enough at its radius.
pub fn disc(c: Pt, r: f64) -> Vec<Pt> {
    let n = ((TAU / (2.0 * (1.0 - (0.15 / r.max(0.2)).min(1.0)).acos()).max(1e-3)).ceil() as usize).clamp(8, 256);
    (0..n).map(|i| {
        let a = TAU * i as f64 / n as f64;
        Pt { x: c.x + r * a.cos(), y: c.y + r * a.sin() }
    }).collect()
}

/// CG's miter limit: beyond this ratio of miter length to half-width
/// the join is beveled.
const MITER_LIMIT: f64 = 10.0;

/// The wedge that fills the outside of the turn at `v` (between the
/// segment quads a→v and v→b): a miter, or a bevel past the limit.
fn miter_join(a: Pt, v: Pt, b: Pt, hw: f64) -> Option<Vec<Pt>> {
    let (d1x, d1y) = (v.x - a.x, v.y - a.y);
    let (d2x, d2y) = (b.x - v.x, b.y - v.y);
    let (l1, l2) = (d1x.hypot(d1y), d2x.hypot(d2y));
    if l1 < 1e-9 || l2 < 1e-9 {
        return None;
    }
    let (u1, u2) = ((d1x / l1, d1y / l1), (d2x / l2, d2y / l2));
    let cross = u1.0 * u2.1 - u1.1 * u2.0;
    if cross.abs() < 1e-9 {
        return None; // straight on (or doubling back): nothing to fill
    }
    // The outside of the turn is opposite the turn's direction.
    let side = if cross > 0.0 { -1.0 } else { 1.0 };
    let n1 = (-u1.1 * side * hw, u1.0 * side * hw);
    let n2 = (-u2.1 * side * hw, u2.0 * side * hw);
    let p1 = Pt { x: v.x + n1.0, y: v.y + n1.1 };
    let p2 = Pt { x: v.x + n2.0, y: v.y + n2.1 };
    // The miter tip: along the bisector of the two offset normals.
    let (bx, by) = (n1.0 + n2.0, n1.1 + n2.1);
    let blen = bx.hypot(by);
    let cos_half = blen / (2.0 * hw);
    if cos_half < 1e-9 || 1.0 / cos_half > MITER_LIMIT {
        return Some(std::vec![v, p1, p2]);
    }
    let reach = hw / cos_half;
    let tip = Pt { x: v.x + bx / blen * reach, y: v.y + by / blen * reach };
    Some(std::vec![v, p1, tip, p2])
}

/// The polygons of `polys` stroked at `width` px with `cap`.
pub fn stroke(polys: &[Poly], width: f64, cap: Cap) -> Vec<Vec<Pt>> {
    let hw = 0.5 * width;
    let mut out = Vec::new();
    for poly in polys {
        let pts = &poly.pts;
        if pts.is_empty() {
            continue;
        }
        if pts.len() == 1 {
            if cap == Cap::Round {
                out.push(orient(disc(pts[0], hw)));
            }
            continue;
        }
        let n = pts.len();
        let segs = if poly.closed { n } else { n - 1 };
        for i in 0..segs {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            out.push(orient(std::vec![
                Pt { x: a.x + nx, y: a.y + ny },
                Pt { x: b.x + nx, y: b.y + ny },
                Pt { x: b.x - nx, y: b.y - ny },
                Pt { x: a.x - nx, y: a.y - ny },
            ]));
        }
        // Joins: every interior vertex (all of them on a closed path).
        let joins = if poly.closed { 0..n } else { 1..n - 1 };
        for i in joins {
            let (a, v, b) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            if let Some(j) = miter_join(a, v, b, hw) {
                out.push(orient(j));
            }
        }
        if !poly.closed && cap == Cap::Round {
            out.push(orient(disc(pts[0], hw)));
            out.push(orient(disc(pts[n - 1], hw)));
        }
    }
    out
}
