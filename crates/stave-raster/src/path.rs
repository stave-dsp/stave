//! Flattening: a Scene `Path` (lines, quadratics, cubics, circular arcs)
//! into polylines in DEVICE pixels, within a fixed tolerance.
//!
//! Arcs follow the Scene IR's convention, which is CoreGraphics'
//! `CGContextAddArc` as seen in the scene's own y-DOWN space: angles in
//! radians from +x; `cw == false` sweeps toward INCREASING angle (which
//! in y-down is visually clockwise), `cw == true` toward decreasing; a
//! sweep of a full turn or more draws the whole circle. As in CG, an arc
//! with a current point first draws a line to its start.

use std::vec::Vec;

use core::f64::consts::TAU;

use stave_ui::scene::{Path, Pt, Seg};

/// Maximum distance (px) between a curve and its flattened chords.
const TOLERANCE: f64 = 0.15;

/// One flattened subpath.
#[derive(Clone, Debug, Default)]
pub struct Poly {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

#[inline]
fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    Pt { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }
}

/// Flatten `path`, scaling every coordinate by `scale` (points → px).
pub fn flatten(path: &Path, scale: f64) -> Vec<Poly> {
    let s = |p: Pt| Pt { x: p.x * scale, y: p.y * scale };
    let mut out: Vec<Poly> = Vec::new();
    let mut cur: Option<Poly> = None;
    let mut last = Pt { x: 0.0, y: 0.0 };
    let mut start = last;
    let push = |cur: &mut Option<Poly>, p: Pt| {
        if let Some(poly) = cur.as_mut() {
            if poly.pts.last().is_none_or(|q| (q.x - p.x).abs() > 1e-9 || (q.y - p.y).abs() > 1e-9) {
                poly.pts.push(p);
            }
        }
    };
    for seg in &path.segs {
        match *seg {
            Seg::MoveTo(p) => {
                if let Some(poly) = cur.take() {
                    out.push(poly);
                }
                let p = s(p);
                cur = Some(Poly { pts: std::vec![p], closed: false });
                last = p;
                start = p;
            }
            Seg::LineTo(p) => {
                let p = s(p);
                if cur.is_none() {
                    cur = Some(Poly { pts: std::vec![last], closed: false });
                }
                push(&mut cur, p);
                last = p;
            }
            Seg::QuadTo(c, p) => {
                let (c, p) = (s(c), s(p));
                if cur.is_none() {
                    cur = Some(Poly { pts: std::vec![last], closed: false });
                }
                let dd = ((last.x - 2.0 * c.x + p.x).powi(2) + (last.y - 2.0 * c.y + p.y).powi(2)).sqrt();
                let n = ((dd / (4.0 * TOLERANCE)).sqrt().ceil() as usize).clamp(1, 256);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    push(&mut cur, lerp(lerp(last, c, t), lerp(c, p, t), t));
                }
                last = p;
            }
            Seg::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (s(c1), s(c2), s(p));
                if cur.is_none() {
                    cur = Some(Poly { pts: std::vec![last], closed: false });
                }
                let d1 = ((last.x - 2.0 * c1.x + c2.x).powi(2) + (last.y - 2.0 * c1.y + c2.y).powi(2)).sqrt();
                let d2 = ((c1.x - 2.0 * c2.x + p.x).powi(2) + (c1.y - 2.0 * c2.y + p.y).powi(2)).sqrt();
                let n = ((0.75 * d1.max(d2) / TOLERANCE).sqrt().ceil() as usize).clamp(1, 256);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let a = lerp(lerp(last, c1, t), lerp(c1, c2, t), t);
                    let b = lerp(lerp(c1, c2, t), lerp(c2, p, t), t);
                    push(&mut cur, lerp(a, b, t));
                }
                last = p;
            }
            Seg::Arc { c, r, a0, a1, cw } => {
                let (c, r) = (s(c), r * scale);
                let sweep = arc_sweep(a0, a1, cw);
                let at = |a: f64| Pt { x: c.x + r * a.cos(), y: c.y + r * a.sin() };
                let first = at(a0);
                match cur.as_mut() {
                    Some(_) => push(&mut cur, first),
                    None => {
                        cur = Some(Poly { pts: std::vec![first], closed: false });
                        start = first;
                    }
                }
                let step = if r > TOLERANCE { 2.0 * (1.0 - TOLERANCE / r).acos() } else { TAU };
                let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
                for i in 1..=n {
                    push(&mut cur, at(a0 + sweep * i as f64 / n as f64));
                }
                last = at(a0 + sweep);
            }
            Seg::Close => {
                if let Some(mut poly) = cur.take() {
                    poly.closed = true;
                    out.push(poly);
                }
                last = start;
            }
        }
    }
    if let Some(poly) = cur.take() {
        out.push(poly);
    }
    out
}

/// The signed sweep of an arc from `a0` to `a1` (CG semantics, y-down).
fn arc_sweep(a0: f64, a1: f64, cw: bool) -> f64 {
    let d = a1 - a0;
    if !cw {
        if d >= TAU { TAU } else { d.rem_euclid(TAU) }
    } else if d <= -TAU {
        -TAU
    } else {
        -((-d).rem_euclid(TAU))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::pt;

    #[test]
    fn a_circle_flattens_closed_and_round() {
        let polys = flatten(&Path::circle(pt(10.0, 10.0), 5.0), 2.0);
        assert_eq!(polys.len(), 1);
        for p in &polys[0].pts {
            let r = (p.x - 20.0).hypot(p.y - 20.0);
            assert!((r - 10.0).abs() < 1e-9);
        }
        let (a, b) = (polys[0].pts[0], *polys[0].pts.last().unwrap());
        assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6, "a full turn returns to its start");
    }

    #[test]
    fn arc_directions_follow_cg() {
        assert!((arc_sweep(0.0, 1.0, false) - 1.0).abs() < 1e-12);
        assert!((arc_sweep(0.0, 1.0, true) + (TAU - 1.0)).abs() < 1e-12);
        assert!((arc_sweep(1.0, 0.0, true) + 1.0).abs() < 1e-12);
        assert!((arc_sweep(0.0, TAU, false) - TAU).abs() < 1e-12);
    }
}
