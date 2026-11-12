//! `Geometry` — where the sixteen neurons sit, and how the room LEARNS
//! its own shape.
//!
//! Each node has a DRAWN position (the face's x/y params — yours) and
//! a learned OFFSET. Where it actually sits is the sum. PLASTICITY is
//! Hebbian: the room watches its own lines, and lines that ring
//! together (positively correlated) are drawn toward each other — they
//! wire together — while lines that oppose each other are pushed
//! apart, and a short-range repulsion keeps any two from merging. A
//! spring always pulls every offset back toward your drawing; turn
//! PLASTICITY down and the room relaxes into what you drew.
//!
//! Distances are measured on the field as the face shows it (its
//! width is FIELD_ASPECT heights), so what looks close IS close.

use super::room::LINES;
use crate::FIELD_ASPECT;

/// Correlation estimate time (field steps) and Hebbian rate at full
/// PLASTICITY (field heights per block per unit correlation).
const CORR_K: f32 = 0.002;
const HEBB: f32 = 0.0016;
/// The spring back to the drawing: per block, at PLASTICITY 1 and 0.
const SPRING: (f32, f32) = (0.0004, 0.01);
/// Short-range repulsion (field heights).
const PERSONAL: f32 = 0.06;

pub(super) struct Geometry {
    /// Smoothed second moments of the lines' outputs.
    cov: [[f32; LINES]; LINES],
    offset: [[f32; 2]; LINES],
    pos: [[f32; 2]; LINES],
}

impl Geometry {
    pub(super) fn new() -> Self {
        Self { cov: [[0.0; LINES]; LINES], offset: [[0.0; 2]; LINES], pos: [[0.5; 2]; LINES] }
    }

    pub(super) fn reset(&mut self) {
        self.cov = [[0.0; LINES]; LINES];
        self.offset = [[0.0; 2]; LINES];
    }

    /// Accumulate one observation of the lines (call at the memory's
    /// rate, not every frame).
    #[inline]
    pub(super) fn observe(&mut self, v: &[f32; LINES]) {
        for i in 0..LINES {
            for j in i..LINES {
                let c = &mut self.cov[i][j];
                *c += (v[i] * v[j] - *c) * CORR_K;
            }
        }
    }

    /// One block of learning (if `plasticity` > 0 and not held), then
    /// where every node now sits, given the drawing.
    pub(super) fn step(&mut self, drawn: &[[f32; 2]; LINES], plasticity: f32, frozen: bool) {
        let a = FIELD_ASPECT as f32;
        if !frozen {
            let spring = SPRING.1 + (SPRING.0 - SPRING.1) * plasticity;
            let mut push = [[0.0f32; 2]; LINES];
            for i in 0..LINES {
                for j in 0..LINES {
                    if i == j {
                        continue;
                    }
                    let (p, q) = (self.pos[i], self.pos[j]);
                    let (dx, dy) = ((q[0] - p[0]) * a, q[1] - p[1]);
                    let d = dx.hypot(dy).max(1e-4);
                    let (ux, uy) = (dx / d, dy / d);
                    // Hebbian pull (or push) by normalised correlation.
                    let (lo, hi) = (i.min(j), i.max(j));
                    let var = (self.cov[i][i] * self.cov[j][j]).sqrt();
                    let rho = if var > 1e-9 { self.cov[lo][hi] / var } else { 0.0 };
                    let f = HEBB * plasticity * rho;
                    // Short-range repulsion: never merge.
                    let rep = if d < PERSONAL { -0.004 * (1.0 - d / PERSONAL) } else { 0.0 };
                    push[i][0] += (f + rep) * ux / a;
                    push[i][1] += (f + rep) * uy;
                }
            }
            for i in 0..LINES {
                for k in 0..2 {
                    self.offset[i][k] += push[i][k] - spring * self.offset[i][k];
                    self.offset[i][k] = self.offset[i][k].clamp(-0.5, 0.5);
                }
            }
        }
        for i in 0..LINES {
            for k in 0..2 {
                self.pos[i][k] = (drawn[i][k] + self.offset[i][k]).clamp(0.0, 1.0);
            }
        }
    }

    /// Each node's distance from the field's centre, 0 … 1 (the rim's
    /// corners are 1).
    pub(super) fn radius(&self) -> [f32; LINES] {
        let a = FIELD_ASPECT as f32;
        let max = (0.5 * a).hypot(0.5);
        core::array::from_fn(|i| ((self.pos[i][0] - 0.5) * a).hypot(self.pos[i][1] - 0.5) / max)
    }

    /// Pairwise distances, in field heights.
    pub(super) fn distances(&self) -> [[f32; LINES]; LINES] {
        let a = FIELD_ASPECT as f32;
        core::array::from_fn(|i| {
            core::array::from_fn(|j| ((self.pos[i][0] - self.pos[j][0]) * a).hypot(self.pos[i][1] - self.pos[j][1]))
        })
    }

    pub(super) fn pos(&self, i: usize) -> [f32; 2] {
        self.pos[i]
    }

    #[cfg(test)]
    pub(super) fn drift(&self) -> f32 {
        self.offset.iter().map(|o| o[0].abs() + o[1].abs()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlated_lines_attract_and_the_spring_brings_them_home() {
        let mut g = Geometry::new();
        let drawn: [[f32; 2]; LINES] = core::array::from_fn(|i| [0.1 + 0.05 * i as f32, 0.2 + 0.03 * i as f32]);
        g.step(&drawn, 1.0, false);
        let d0 = g.distances()[0][15];
        // Lines 0 and 15 ring in lockstep; the rest are quiet noise.
        for t in 0..20_000 {
            let s = (t as f32 * 0.05).sin();
            let mut v = [0.0f32; LINES];
            for (k, x) in v.iter_mut().enumerate() {
                *x = 0.05 * ((t * (k + 3)) as f32 * 0.7).sin();
            }
            v[0] = s;
            v[15] = s;
            g.observe(&v);
            if t % 64 == 0 {
                g.step(&drawn, 1.0, false);
            }
        }
        let d1 = g.distances()[0][15];
        assert!(d1 < 0.8 * d0, "lockstep lines did not draw together ({d0:.3} → {d1:.3})");
        // PLASTICITY 0: everything relaxes back to the drawing.
        for _ in 0..3_000 {
            g.step(&drawn, 0.0, false);
        }
        assert!(g.drift() < 1e-3, "the spring left {:.4} of drift", g.drift());
    }
}
