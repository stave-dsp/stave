//! Coverage: exact-area anti-aliasing by signed-area accumulation (the
//! technique of Raph Levien's font-rs, written out here). Each polygon
//! edge deposits, into the cells it crosses, the signed area it sweeps;
//! a running sum along each row then gives every pixel's covered
//! fraction exactly. Winding adds up — the NONZERO rule, with |winding|
//! clamped to one — so overlapping same-direction shapes union cleanly.
//!
//! Works in a small bbox-local buffer per draw (a dot costs a dot, not a
//! full canvas), clipped to the canvas.

use std::vec;
use std::vec::Vec;

use stave_ui::scene::{FillRule, Pt};

/// A coverage mask over the pixel rect `x0..x0+w`, `y0..y0+h`.
pub struct Mask {
    pub x0: i64,
    pub y0: i64,
    pub w: usize,
    pub h: usize,
    /// Row-major coverage 0..1 (after `finish`).
    pub cov: Vec<f32>,
    acc: Vec<f32>,
    stride: usize,
}

impl Mask {
    /// A mask for polygons whose bounds are `(minx, miny, maxx, maxy)`,
    /// clipped to a `cw × ch` canvas. None if nothing is on the canvas.
    pub fn new(bounds: (f64, f64, f64, f64), cw: usize, ch: usize) -> Option<Self> {
        let x0 = (bounds.0.floor() as i64).max(0);
        let y0 = (bounds.1.floor() as i64).max(0);
        // Saturating: an extent at infinity (`as` saturates it to
        // i64::MAX) covers the canvas, it does not wrap to nothing.
        // Found by the `raster` fuzz target.
        let x1 = (bounds.2.ceil() as i64).saturating_add(1).min(cw as i64);
        let y1 = (bounds.3.ceil() as i64).saturating_add(1).min(ch as i64);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let stride = w + 3;
        Some(Self { x0, y0, w, h, cov: Vec::new(), acc: vec![0.0; stride * h], stride })
    }

    /// Deposit one closed polygon (the last point joins the first).
    pub fn polygon(&mut self, pts: &[Pt]) {
        if pts.len() < 2 {
            return;
        }
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            self.line(a.x - self.x0 as f64, a.y - self.y0 as f64, b.x - self.x0 as f64, b.y - self.y0 as f64);
        }
    }

    /// One edge, in mask-local pixels.
    fn line(&mut self, ax: f64, ay: f64, bx: f64, by: f64) {
        let (dir, (x0, y0), (x1, y1)) = if ay < by { (1.0f32, (ax, ay), (bx, by)) } else { (-1.0, (bx, by), (ax, ay)) };
        let h = self.h as f64;
        if y1 <= 0.0 || y0 >= h || (y1 - y0) < 1e-12 {
            return;
        }
        let dxdy = (x1 - x0) / (y1 - y0);
        // Clip to the mask's rows.
        let (ys, ye) = (y0.max(0.0), y1.min(h));
        let mut x = x0 + (ys - y0) * dxdy;
        let wmax = self.w as f64 + 1.0;
        let mut y = ys;
        while y < ye {
            let row = y.floor();
            let yn = (row + 1.0).min(ye);
            let dy = (yn - y) as f32;
            let xn = x + dxdy * (yn - y);
            // x beyond the left edge collapses onto column 0 (it still
            // winds every pixel to its right); beyond the right, it is
            // off the mask.
            let (xa, xb) = (x.clamp(0.0, wmax), xn.clamp(0.0, wmax));
            self.cell_span(row as usize, xa, xb, dy * dir);
            x = xn;
            y = yn;
        }
    }

    /// Deposit an edge crossing one row from `xa` to `xb` (it moves
    /// linearly in y across the row), carrying signed height `d`.
    ///
    /// A pixel's share is the area of it lying RIGHT of the edge,
    /// averaged over the row's height: with H(u) the fraction of the
    /// row's height where the edge is left of u (0, ramping to 1 across
    /// [l, r]) and G its integral, pixel x gets G(x+1) − G(x). The cells
    /// store differences, so the row's prefix sum restores each pixel's
    /// value — and every pixel right of the edge receives the full `d`.
    fn cell_span(&mut self, row: usize, xa: f64, xb: f64, d: f32) {
        let base = row * self.stride;
        let (l, r) = if xa < xb { (xa, xb) } else { (xb, xa) };
        let li = l.floor() as usize;
        if r - l < 1e-9 {
            let xm = (l - li as f64) as f32;
            self.acc[base + li] += d * (1.0 - xm);
            self.acc[base + li + 1] += d * xm;
            return;
        }
        let span = r - l;
        let g = |u: f64| -> f64 {
            if u <= l {
                0.0
            } else if u >= r {
                0.5 * span + (u - r)
            } else {
                0.5 * (u - l) * (u - l) / span
            }
        };
        let ri = r.floor() as usize;
        let mut prev = 0.0;
        for x in li..=ri {
            let a = g(x as f64 + 1.0) - g(x as f64);
            self.acc[base + x] += d * (a - prev) as f32;
            prev = a;
        }
        self.acc[base + ri + 1] += d * (1.0 - prev) as f32;
    }

    /// Prefix-sum every row into coverage under the nonzero rule.
    pub fn finish(&mut self) {
        self.finish_with(FillRule::NonZero);
    }

    /// Prefix-sum every row into coverage: the row's running sum is the
    /// pixel's (area-weighted) winding. Nonzero clamps |winding| to one;
    /// even-odd folds it onto a triangle wave, 0 at even windings and 1
    /// at odd ones, so an area wound twice is a hole.
    pub fn finish_with(&mut self, rule: FillRule) {
        self.cov = vec![0.0; self.w * self.h];
        for row in 0..self.h {
            let mut s = 0.0f32;
            for x in 0..self.w {
                s += self.acc[row * self.stride + x];
                let w = s.abs();
                self.cov[row * self.w + x] = match rule {
                    FillRule::NonZero => w.min(1.0),
                    FillRule::EvenOdd => {
                        let m = w % 2.0;
                        if m.is_nan() {
                            1.0
                        } else if m > 1.0 {
                            2.0 - m
                        } else {
                            m
                        }
                    }
                };
            }
        }
    }
}

/// Bounds of a set of polygons.
pub fn bounds(polys: &[Vec<Pt>]) -> Option<(f64, f64, f64, f64)> {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut any = false;
    for p in polys.iter().flatten() {
        any = true;
        b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
    }
    any.then_some(b)
}

#[cfg(test)]
#[allow(clippy::identity_op, clippy::erasing_op)] // `row * width + col` reads as a grid
mod tests {
    use super::*;
    use stave_ui::scene::pt;

    fn mask_of(polys: &[Vec<Pt>], w: usize, h: usize) -> Mask {
        let mut m = Mask::new((0.0, 0.0, w as f64, h as f64), w, h).unwrap();
        for p in polys {
            m.polygon(p);
        }
        m.finish();
        m
    }

    #[test]
    fn a_pixel_aligned_square_covers_exactly() {
        let sq = vec![pt(2.0, 2.0), pt(6.0, 2.0), pt(6.0, 6.0), pt(2.0, 6.0)];
        let m = mask_of(&[sq], 8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let want = if (2..6).contains(&x) && (2..6).contains(&y) { 1.0 } else { 0.0 };
                assert!((m.cov[y * 8 + x] - want).abs() < 1e-5, "({x},{y}) = {}", m.cov[y * 8 + x]);
            }
        }
    }

    #[test]
    fn half_pixel_edges_cover_half() {
        let sq = vec![pt(1.5, 1.0), pt(4.5, 1.0), pt(4.5, 3.0), pt(1.5, 3.0)];
        let m = mask_of(&[sq], 6, 4);
        assert!((m.cov[1 * 6 + 1] - 0.5).abs() < 1e-5);
        assert!((m.cov[1 * 6 + 2] - 1.0).abs() < 1e-5);
        assert!((m.cov[1 * 6 + 4] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_diagonal_halves_the_pixels_it_crosses_and_area_is_exact() {
        // A right triangle of area 8 on a 4×4 square.
        let tri = vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(0.0, 4.0)];
        let m = mask_of(&[tri], 4, 4);
        let total: f32 = m.cov.iter().sum();
        assert!((total - 8.0).abs() < 1e-4, "area {total}");
        assert!((m.cov[0 * 4 + 3] - 0.5).abs() < 1e-5, "a pixel cut corner-to-corner is half covered");
    }

    #[test]
    fn same_direction_overlap_unions_and_reversed_cuts_a_hole() {
        let a = vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)];
        let m = mask_of(&[a.clone(), a.clone()], 4, 4);
        assert!(m.cov.iter().all(|c| (*c - 1.0).abs() < 1e-5), "overlap clamps to full");
        let hole: Vec<Pt> = vec![pt(1.0, 1.0), pt(1.0, 3.0), pt(3.0, 3.0), pt(3.0, 1.0)];
        let m = mask_of(&[a, hole], 4, 4);
        assert!(m.cov[1 * 4 + 1] < 1e-5 && (m.cov[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shapes_off_the_left_edge_still_fill_what_is_visible() {
        let sq = vec![pt(-3.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0), pt(-3.0, 2.0)];
        let mut m = Mask::new((-3.0, 0.0, 2.0, 2.0), 4, 4).unwrap();
        m.polygon(&sq);
        m.finish();
        assert!((m.cov[0] - 1.0).abs() < 1e-5 && (m.cov[1] - 1.0).abs() < 1e-5);
    }
}

/// T9: a self-overlapping star (a pentagram) has a hole under the
/// even-odd rule — its pentagon centre is wound twice — and none under
/// nonzero.
#[cfg(test)]
mod fill_rule {
    use super::*;

    fn star_centre(rule: FillRule) -> f32 {
        let pts: Vec<Pt> = (0..5)
            .map(|k| {
                let a = -std::f64::consts::FRAC_PI_2 + f64::from(k * 2) * std::f64::consts::TAU / 5.0;
                Pt { x: 20.0 + 18.0 * a.cos(), y: 20.0 + 18.0 * a.sin() }
            })
            .collect();
        let mut mask = Mask::new(bounds(std::slice::from_ref(&pts)).unwrap(), 40, 40).unwrap();
        mask.polygon(&pts);
        mask.finish_with(rule);
        let (x, y) = (20 - mask.x0 as usize, 20 - mask.y0 as usize);
        mask.cov[y * mask.w + x]
    }

    #[test]
    fn a_pentagram_has_a_hole_under_even_odd() {
        assert_eq!(star_centre(FillRule::NonZero), 1.0);
        assert!(star_centre(FillRule::EvenOdd) < 1e-3);
    }
}

/// Found by the `raster` fuzz target: a shape reaching infinity covered
/// nothing (its extent wrapped past i64::MAX) instead of the canvas.
#[cfg(test)]
mod infinite_extent {
    use super::*;

    #[test]
    fn a_shape_reaching_infinity_still_covers_the_canvas() {
        let pts = [Pt { x: 0.0, y: 0.0 }, Pt { x: f64::INFINITY, y: 0.0 }, Pt { x: f64::INFINITY, y: f64::INFINITY }, Pt { x: 0.0, y: f64::INFINITY }];
        let mut mask = Mask::new(bounds(&[pts.to_vec()]).unwrap(), 4, 4).expect("on the canvas");
        assert_eq!((mask.w, mask.h), (4, 4));
        mask.polygon(&pts);
        mask.finish();
    }
}

/// Every edge on a grid of representative coordinates — NaN, the
/// infinities, off-canvas, on and between pixel boundaries — on every
/// canvas up to 3×3, exhaustively (a few thousand cases, milliseconds):
/// deposits stay inside the mask (an index past it panics), and every
/// finished pixel's coverage is in [0, 1]. Arbitrary f64s are the
/// `raster` fuzz target's; Kani's float encoding does not finish here.
#[cfg(test)]
mod exhaustive {
    use super::*;

    const COORDS: [f64; 9] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -2.5, -0.25, 0.0, 1.0, 1.75, 4.5];

    #[test]
    #[cfg_attr(miri, ignore = "safe code: the native run covers it")]
    fn every_grid_edge_stays_inside_the_mask() {
        for (cw, ch) in (0..=3).flat_map(|w| (0..=3).map(move |h| (w, h))) {
            for &ax in &COORDS {
                for &ay in &COORDS {
                    for &bx in &COORDS {
                        for &by in &COORDS {
                            let pts = [Pt { x: ax, y: ay }, Pt { x: bx, y: by }, Pt { x: ax, y: by }];
                            let Some(mut mask) = Mask::new(bounds(&[pts.to_vec()]).unwrap(), cw, ch) else {
                                continue;
                            };
                            mask.polygon(&pts);
                            mask.finish();
                            assert!(mask.cov.iter().all(|c| (0.0..=1.0).contains(c)), "{pts:?}");
                        }
                    }
                }
            }
        }
    }
}
