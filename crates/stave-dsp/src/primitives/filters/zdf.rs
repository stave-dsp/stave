//! What the nonlinear zero-delay-feedback filters share: the knobs every
//! one of them takes, a small dense Newton step, and the tail of the
//! linearised filter. Each filter integrates its own nonlinear ODE by
//! the trapezoidal rule (Zavalishin's topology-preserving transform),
//! solving the implicit step by Newton — no unit delay in any loop.

use crate::kernel::node::Tail;
use crate::primitives::solve::NewtonStats;
use crate::units::{Db, Hz};

/// Newton stops when the largest residual is below this (the states are
/// normalised so that 1 is where the saturators bend; the Jacobian is
/// near the identity, so the residual is the error).
pub(crate) const TOLERANCE: f64 = 1e-10;
/// Newton's cap: a solve that reaches it is counted in
/// [`NewtonStats::capped`], never silent.
pub(crate) const MAX_ITERATIONS: u32 = 24;
/// No single Newton update moves a state further than this, so a
/// transient far into a saturator's flat part converges instead of
/// overshooting.
pub(crate) const MAX_STEP: f64 = 4.0;

/// The knobs every nonlinear filter takes, and the rate it is prepared at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Knobs {
    pub sample_rate: f32,
    pub cutoff: Hz,
    pub resonance: f32,
    /// Linear gain into the saturators; the output is divided by it.
    pub drive: f64,
}

impl Knobs {
    pub fn new(cutoff: Hz, resonance: f32) -> Self {
        Self {
            sample_rate: 48_000.0,
            cutoff,
            resonance,
            drive: 1.0,
        }
    }

    /// `tan(π·f/fs)`, the cutoff held inside (1 Hz, 0.49·fs): the
    /// trapezoidal integrator's gain, prewarped so the filter's
    /// characteristic frequency lands exactly.
    pub fn warped(&self) -> f64 {
        let fs = f64::from(self.sample_rate);
        let f = f64::from(self.cutoff.get()).clamp(1.0, 0.49 * fs);
        (core::f64::consts::PI * f / fs).tan()
    }

    pub fn set_drive(&mut self, drive: Db) {
        self.drive = f64::from(drive.to_gain().get()).max(1e-6);
    }

    pub fn resonance(&self) -> f64 {
        f64::from(self.resonance.max(0.0))
    }
}

/// Solve `J·d = r` in place (`r` becomes `d`) by Gaussian elimination
/// with partial pivoting. `J` is never singular here: every filter's
/// Jacobian is the identity plus a step times a bounded slope.
pub(crate) fn solve<const N: usize>(j: &mut [[f64; N]; N], r: &mut [f64; N]) {
    for col in 0..N {
        let pivot = (col..N)
            .max_by(|&a, &b| j[a][col].abs().total_cmp(&j[b][col].abs()))
            .unwrap_or(col);
        j.swap(col, pivot);
        r.swap(col, pivot);
        let p = j[col][col];
        for row in col + 1..N {
            let f = j[row][col] / p;
            if f != 0.0 {
                for k in col..N {
                    j[row][k] -= f * j[col][k];
                }
                r[row] -= f * r[col];
            }
        }
    }
    for col in (0..N).rev() {
        let mut v = r[col];
        for k in col + 1..N {
            v -= j[col][k] * r[k];
        }
        r[col] = v / j[col][col];
    }
}

/// One Newton update, `y -= d` with each step clamped.
pub(crate) fn update<const N: usize>(y: &mut [f64; N], d: &[f64; N]) {
    for (y, d) in y.iter_mut().zip(d) {
        *y -= d.clamp(-MAX_STEP, MAX_STEP);
    }
}

/// Whether every residual is inside [`TOLERANCE`].
pub(crate) fn small<const N: usize>(r: &[f64; N]) -> bool {
    r.iter().all(|r| r.abs() < TOLERANCE)
}

/// `tanh` by `expm1`: accurate to a few ulps everywhere (no cancellation
/// near 0), and about twice as fast as the libm `tanh` it replaces.
#[inline]
pub(crate) fn tanh(x: f64) -> f64 {
    let e = (2.0 * x.clamp(-20.0, 20.0)).exp_m1();
    e / (e + 2.0)
}

/// Record a solve that took `iterations` and did or did not converge.
pub(crate) fn record(stats: &mut NewtonStats, iterations: u32, converged: bool) {
    stats.record(iterations, converged, 0.0);
}

/// The tail of the linearised filter whose characteristic polynomial,
/// in `s` normalised to the prewarped frequency, has the real
/// coefficients `poly` (highest power first): every root mapped through
/// the bilinear transform `z = (1 + g·s)/(1 − g·s)`, the slowest one's
/// decay. Saturation only lowers a loop's gain, so the driven filter
/// rings no longer than this.
pub(crate) fn linear_tail(poly: &[f64], g: f64) -> Tail {
    let radius = roots(poly)
        .iter()
        .take(poly.len() - 1)
        .map(|&(re, im)| {
            let (nr, ni) = (1.0 + g * re, g * im);
            let (dr, di) = (1.0 - g * re, -g * im);
            ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
        })
        .fold(0.0, f64::max);
    Tail::from_pole(radius)
}

/// The roots of a real polynomial of degree ≤ 4 (Durand–Kerner), as
/// `(re, im)`; off the audio path (a stage's `tail`).
fn roots(poly: &[f64]) -> [(f64, f64); 4] {
    let n = poly.len() - 1;
    debug_assert!((1..=4).contains(&n), "degree 1 to 4");
    let lead = poly[0];
    let mut c = [0.0; 5];
    for (c, p) in c.iter_mut().zip(poly) {
        *c = p / lead;
    }
    let c = &c[..=n];
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let eval = |z: (f64, f64)| {
        c.iter().fold((0.0, 0.0), |acc, &k| {
            let m = mul(acc, z);
            (m.0 + k, m.1)
        })
    };
    let mut z = [(0.0, 0.0); 4];
    let seed = (0.4, 0.9);
    let mut w = (1.0, 0.0);
    for slot in z.iter_mut().take(n) {
        *slot = w;
        w = mul(w, seed);
    }
    for _ in 0..500 {
        for i in 0..n {
            let mut den = (1.0, 0.0);
            for k in 0..n {
                if k != i {
                    den = mul(den, (z[i].0 - z[k].0, z[i].1 - z[k].1));
                }
            }
            let num = eval(z[i]);
            let m = den.0 * den.0 + den.1 * den.1;
            if m > 0.0 {
                let q = (
                    (num.0 * den.0 + num.1 * den.1) / m,
                    (num.1 * den.0 - num.0 * den.1) / m,
                );
                z[i] = (z[i].0 - q.0, z[i].1 - q.1);
            }
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_solve_and_roots() {
        let mut j = [[2.0, 1.0, 0.0], [1.0, 3.0, 1.0], [0.0, 1.0, 4.0]];
        let mut r = [3.0, 5.0, 5.0];
        solve(&mut j, &mut r);
        for (got, want) in r.iter().zip([1.0, 1.0, 1.0]) {
            assert!((got - want).abs() < 1e-12, "{r:?}");
        }
        // (s + 1)⁴ + 4: roots at −1 ± 1 ± j.
        let z = roots(&[1.0, 4.0, 6.0, 4.0, 5.0]);
        for (re, im) in z {
            assert!(((re + 1.0).abs() - 1.0).abs() < 1e-9, "{z:?}");
            assert!((im.abs() - 1.0).abs() < 1e-9, "{z:?}");
        }
    }
}
