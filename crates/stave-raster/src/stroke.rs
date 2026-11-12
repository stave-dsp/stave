//! Stroking: a flattened polyline at a width into the OUTLINE of the
//! stroked area, which the coverage pass fills under the nonzero rule.
//!
//! An open subpath becomes one polygon: its left offset walked forward,
//! the end cap, its left offset walked backward (the right side), the
//! start cap. A closed subpath becomes two rings, one per side, wound
//! opposite ways so the inside of the path is not filled. Joins are
//! MITERED with a miter limit of 10, beyond which they bevel
//! (CoreGraphics' defaults); caps are butt, round or square. A round cap
//! is still a whole disc unioned over each end, so in the edge pixels
//! where it overlaps the line's end both count (ledger T9).
//!
//! On the inside of a turn the two offset edges are cut at the point
//! where they cross, so every boundary edge is a true edge of the
//! stroked area and an anti-aliased edge pixel is covered by exactly the
//! area of the stroke inside it — nothing counts twice. When a segment
//! is too short to reach that crossing (a fat stroke round a tight
//! bend), the inside goes through the vertex instead: the outline is
//! then, edge for edge, the union of the segments' own rectangles, so
//! the area is still filled with nonzero winding everywhere and only
//! interior pixels can see the overlap.

use std::vec::Vec;

use core::f64::consts::TAU;

use stave_ui::scene::{Cap, Pt};

use crate::path::Poly;

/// Points per full turn for an arc of radius `r` px, so its chords stay
/// within the flattening tolerance.
fn arc_steps(r: f64) -> usize {
    ((TAU / (2.0 * (1.0 - (0.15 / r.max(0.2)).min(1.0)).acos()).max(1e-3)).ceil() as usize).clamp(8, 256)
}

/// A disc as a polygon fine enough at its radius.
pub fn disc(c: Pt, r: f64) -> Vec<Pt> {
    let n = arc_steps(r);
    (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            Pt { x: c.x + r * a.cos(), y: c.y + r * a.sin() }
        })
        .collect()
}

/// CG's miter limit: beyond this ratio of miter length to half-width
/// the join is beveled.
const MITER_LIMIT: f64 = 10.0;

#[inline]
fn at(p: Pt, dx: f64, dy: f64) -> Pt {
    Pt { x: p.x + dx, y: p.y + dy }
}

/// Unit direction of `a → b` and its length.
#[inline]
fn dir(a: Pt, b: Pt) -> ((f64, f64), f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = dx.hypot(dy);
    ((dx / l, dy / l), l)
}

/// The left normal of unit direction `u`, at length `hw`.
#[inline]
fn left(u: (f64, f64), hw: f64) -> (f64, f64) {
    (-u.1 * hw, u.0 * hw)
}

/// `pts` without consecutive repeats (and, when `closed`, without a last
/// point repeating the first): every segment has a direction.
fn distinct(pts: &[Pt], closed: bool) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_none_or(|q: &Pt| (q.x - p.x).hypot(q.y - p.y) > 1e-9) {
            out.push(p);
        }
    }
    if closed {
        while out.len() > 1 && (out[0].x - out[out.len() - 1].x).hypot(out[0].y - out[out.len() - 1].y) <= 1e-9 {
            out.pop();
        }
    }
    out
}

/// The left side's turn at `v`, from the segment `a → v` (direction
/// `u1`) to `v → b` (`u2`), appended to `out`. `room1`/`room2` are how
/// far back along each segment this join may cut its inside edge: the
/// whole segment when no other join shares it, else half.
fn join(out: &mut Vec<Pt>, v: Pt, (u1, room1): ((f64, f64), f64), (u2, room2): ((f64, f64), f64), hw: f64) {
    let (n1, n2) = (left(u1, hw), left(u2, hw));
    let (p1, p2) = (at(v, n1.0, n1.1), at(v, n2.0, n2.1));
    let cross = u1.0 * u2.1 - u1.1 * u2.0;
    let dot = u1.0 * u2.0 + u1.1 * u2.1;
    if cross.abs() < 1e-9 {
        if dot > 0.0 {
            out.push(p1); // straight on
        } else {
            out.extend([p1, v, p2]); // doubling back: a flat (bevel) end
        }
        return;
    }
    if cross > 0.0 {
        // Turning toward the left: this is the inside. The offset edges
        // cross `t` back from their ends; cut them there when both
        // segments have the room.
        let t = hw * cross / (1.0 + dot);
        if t <= room1.min(room2) {
            out.push(at(p1, -u1.0 * t, -u1.1 * t));
        } else {
            out.extend([p1, v, p2]);
        }
        return;
    }
    // The outside: a miter, or a bevel past the limit.
    let (bx, by) = (n1.0 + n2.0, n1.1 + n2.1);
    let blen = bx.hypot(by);
    let cos_half = blen / (2.0 * hw);
    if cos_half < 1e-9 || 1.0 / cos_half > MITER_LIMIT {
        out.extend([p1, p2]);
    } else {
        let reach = hw / cos_half;
        out.extend([p1, at(v, bx / blen * reach, by / blen * reach), p2]);
    }
}

/// The left offset of the open polyline `pts` (≥ 2 distinct points),
/// from its first point's offset to its last's, joins included.
fn open_side(out: &mut Vec<Pt>, pts: &[Pt], hw: f64) {
    let n = pts.len();
    // A segment between two joins lends each half its length; the end
    // segments have one join to serve.
    let seg = |i: usize| {
        let (u, l) = dir(pts[i], pts[i + 1]);
        (u, if i == 0 || i == n - 2 { l } else { 0.5 * l })
    };
    let first = seg(0);
    let n0 = left(first.0, hw);
    out.push(at(pts[0], n0.0, n0.1));
    let mut prev = first;
    for i in 1..n - 1 {
        let next = seg(i);
        join(out, pts[i], prev, next, hw);
        prev = next;
    }
    let nl = left(prev.0, hw);
    out.push(at(pts[n - 1], nl.0, nl.1));
}

/// The left offset of the closed polyline `pts` (≥ 2 distinct points)
/// as a ring, a join at every vertex.
fn closed_side(pts: &[Pt], hw: f64) -> Vec<Pt> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n * 2);
    let seg = |i: usize| {
        let (u, l) = dir(pts[i % n], pts[(i + 1) % n]);
        (u, 0.5 * l)
    };
    let mut prev = seg(n - 1);
    for i in 0..n {
        let next = seg(i);
        join(&mut out, pts[i], prev, next, hw);
        prev = next;
    }
    out
}

/// The cap at `end`, leaving along unit direction `u`: the points
/// strictly between the left offset `end + n` and the right `end − n`.
/// (A round cap is a disc of its own; see [`stroke`].)
fn cap_points(out: &mut Vec<Pt>, end: Pt, u: (f64, f64), hw: f64, cap: Cap) {
    if cap == Cap::Square {
        let n = left(u, hw);
        let (ex, ey) = (u.0 * hw, u.1 * hw);
        out.extend([at(end, n.0 + ex, n.1 + ey), at(end, -n.0 + ex, -n.1 + ey)]);
    }
}

/// Signed area ×2 (positive = clockwise in y-down screen space).
fn area2(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x * v.y - v.x * u.y;
    }
    a
}

/// Wind `p` positively (in place), so pieces union under nonzero.
fn orient(mut p: Vec<Pt>) -> Vec<Pt> {
    if area2(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// The polygons of `polys` stroked at `width` px with `cap`.
pub fn stroke(polys: &[Poly], width: f64, cap: Cap) -> Vec<Vec<Pt>> {
    let hw = 0.5 * width;
    let mut out = Vec::new();
    if hw <= 0.0 {
        return out;
    }
    for poly in polys {
        let pts = distinct(&poly.pts, poly.closed);
        match pts.len() {
            0 => {}
            1 => match cap {
                // A lone point: a dot in the cap's shape (CG draws none
                // for butt).
                Cap::Butt => {}
                Cap::Round => out.push(disc(pts[0], hw)),
                Cap::Square => {
                    let p = pts[0];
                    out.push(std::vec![at(p, -hw, -hw), at(p, hw, -hw), at(p, hw, hw), at(p, -hw, hw)]);
                }
            },
            _ if poly.closed && pts.len() > 2 => {
                out.push(closed_side(&pts, hw));
                let back: Vec<Pt> = pts.iter().rev().copied().collect();
                out.push(closed_side(&back, hw));
            }
            _ => {
                // Open (a closed pair of points strokes as a line there
                // and back, which is the same area).
                let back: Vec<Pt> = pts.iter().rev().copied().collect();
                let mut outline = Vec::with_capacity(pts.len() * 4);
                open_side(&mut outline, &pts, hw);
                let m = pts.len();
                cap_points(&mut outline, pts[m - 1], dir(pts[m - 2], pts[m - 1]).0, hw, cap);
                open_side(&mut outline, &back, hw);
                cap_points(&mut outline, pts[0], dir(pts[1], pts[0]).0, hw, cap);
                out.push(orient(outline));
                if cap == Cap::Round {
                    // Whole discs over the ends, unioned with the outline:
                    // where a disc overlaps the line's end, an edge pixel
                    // still counts both (the committed goldens hold this
                    // look; a half-disc cap on the outline is exact).
                    out.push(orient(disc(pts[0], hw)));
                    out.push(orient(disc(pts[m - 1], hw)));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    fn area(p: &[Pt]) -> f64 {
        let mut a = 0.0;
        for i in 0..p.len() {
            let (u, v) = (p[i], p[(i + 1) % p.len()]);
            a += u.x * v.y - v.x * u.y;
        }
        0.5 * a
    }

    fn open(pts: &[(f64, f64)]) -> Poly {
        Poly { pts: pts.iter().map(|&(x, y)| Pt { x, y }).collect(), closed: false }
    }

    #[test]
    fn a_right_angle_is_one_outline_with_the_area_of_its_arms() {
        // Arms of 10 and 6 to the corner, 2 wide, mitered: 2 × 16.
        let polys = stroke(&[open(&[(0.0, 0.0), (10.0, 0.0), (10.0, 6.0)])], 2.0, Cap::Butt);
        assert_eq!(polys.len(), 1, "one polygon, no overlapping pieces");
        assert!((area(&polys[0]).abs() - 32.0).abs() < 1e-9, "area {}", area(&polys[0]));
    }

    #[test]
    fn square_caps_extend_by_half_the_width() {
        let polys = stroke(&[open(&[(0.0, 0.0), (10.0, 0.0)])], 2.0, Cap::Square);
        assert!((area(&polys[0]).abs() - 24.0).abs() < 1e-9);
        let xs = polys[0].iter().map(|p| p.x);
        assert_eq!(xs.clone().fold(f64::MAX, f64::min), -1.0);
        assert_eq!(xs.fold(f64::MIN, f64::max), 11.0);
    }

    #[test]
    fn round_caps_are_discs_over_the_ends() {
        let polys = stroke(&[open(&[(0.0, 0.0), (10.0, 0.0)])], 4.0, Cap::Round);
        assert_eq!(polys.len(), 3, "the outline and a disc at each end");
        assert!((area(&polys[0]).abs() - 40.0).abs() < 1e-9);
        // Chords within 0.15 px of the arc: a little under the disc.
        let disc = area(&polys[1]).abs();
        assert!(disc < PI * 4.0 && disc > 0.9 * PI * 4.0, "disc {disc}");
    }

    #[test]
    fn a_closed_square_is_a_ring_with_a_hole() {
        let sq = Poly { closed: true, ..open(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]) };
        let rings = stroke(&[sq], 2.0, Cap::Butt);
        assert_eq!(rings.len(), 2);
        let (a, b) = (area(&rings[0]), area(&rings[1]));
        assert!(a * b < 0.0, "the rings wind opposite ways");
        assert!(((a + b).abs() - (144.0 - 64.0)).abs() < 1e-9, "{a} {b}");
    }
}
