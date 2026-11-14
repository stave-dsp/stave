//! Bounded nonlinear least squares: Levenberg–Marquardt inside physical
//! bounds, and the L_p continuation toward a minimax fit. The one solver
//! every fit in stave shares — E11's device fits to datasheet curves
//! (`crates/stave-test/tests/fit_datasheet.rs`) and G4's design fits
//! ([`design::fit`](mod@crate::design::fit)).
//!
//! # Models
//!
//! A fit chooses parameter values `v` minimising `Σ|rᵢ(v)|^p`, each
//! residual `rᵢ` already in units of its budget (a residual within ±1
//! is a point within budget). Each parameter is held inside `[lo, hi]`
//! by a logistic map from an unbounded coordinate `z`
//! (`v = lo + (hi − lo)/(1 + e^{−z})`), on a log scale when the bounds
//! are positive and span more than a decade, so a resistor searched
//! from 100 Ω to 1 MΩ moves by ratios, not ohms. No step can leave the
//! bounds, and none is clipped.
//!
//! # Derivation
//!
//! Marquardt's algorithm (1963) with his diagonal scaling: the step
//! solves `(JᵀJ + λ·diag(JᵀJ))·δ = −Jᵀr`, `λ` divided by 3 on a step
//! that lowers the cost and multiplied by 4 on one that does not. The
//! Jacobian is central differences in `z` (step 1e-5), its columns
//! shared among the caller's threads. `p ≠ 2` maps each residual to
//! `sign(r)·|r|^{p/2}` so the same machinery minimises `Σ|r|^p`;
//! [`continuation`] runs p = 2, 4, 8, 16 each from the last, Lawson's
//! way of reaching a Chebyshev (minimax) fit through least squares —
//! the judge of a budget is the worst point, and least squares alone
//! can leave one end of a curve over budget to fit the bulk a little
//! better. It stops when no λ below 1e12 improves the cost, after three
//! successive steps that improve it by under 1e-10 of itself, or after
//! 150 iterations.
//!
//! # Verified
//!
//! `crates/stave-measure/src/levenberg_marquardt.rs::tests`: a
//! Rosenbrock valley's minimum found from (−1.2, 1); an exponential
//! decay's rate and amplitude recovered from exact samples to 1e-9; a
//! bound respected when the unconstrained minimum lies outside it (the
//! value pinned at the bound, the fit reporting it); the L_p
//! continuation lowering the worst residual of an over-determined
//! line fit to the minimax value; the result independent of the thread
//! count, bit for bit. E11's fits (`fit_datasheet.rs`) and G4's
//! (`design::fit`) run through it.
//!
//! # Cost
//!
//! Each iteration evaluates the residuals `2n + 1` times or more (`n`
//! free parameters), plus one dense `n×n` solve per λ tried.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Bounds finite with `lo < hi`; a log-scaled parameter positive.
//! Starting values are clamped just inside their bounds. A local
//! method: a non-convex problem wants several starts (the caller's).

/// A parameter's bounds and its starting value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bounded {
    pub start: f64,
    pub lo: f64,
    pub hi: f64,
}

impl Bounded {
    pub const fn new(start: f64, lo: f64, hi: f64) -> Self {
        Self { start, lo, hi }
    }

    /// Log-scaled when positive and spanning more than a decade.
    pub fn log(&self) -> bool {
        self.lo > 0.0 && self.hi / self.lo > 10.0
    }

    fn span(&self) -> (f64, f64) {
        if self.log() {
            (self.lo.ln(), self.hi.ln())
        } else {
            (self.lo, self.hi)
        }
    }

    /// The bounded value of the free coordinate `z`.
    pub fn value(&self, z: f64) -> f64 {
        let (a, b) = self.span();
        let u = a + (b - a) / (1.0 + (-z).exp());
        if self.log() {
            u.exp()
        } else {
            u
        }
    }

    /// The free coordinate of `v` (clamped just inside the bounds).
    pub fn free(&self, v: f64) -> f64 {
        let (a, b) = self.span();
        let u = if self.log() { v.ln() } else { v };
        let s = ((u - a) / (b - a)).clamp(1e-6, 1.0 - 1e-6);
        (s / (1.0 - s)).ln()
    }

    /// Whether `v` sits at a bound (within 0.1 % of the span).
    pub fn at_bound(&self, v: f64) -> bool {
        let span = self.hi - self.lo;
        (v - self.lo).abs() < 1e-3 * span || (self.hi - v).abs() < 1e-3 * span
    }
}

/// A finished fit: the values, and the residuals there (in budgets, as
/// the caller's function returns them).
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub values: Vec<f64>,
    pub residuals: Vec<f64>,
}

impl Solution {
    /// The mean squared residual.
    pub fn chi2(&self) -> f64 {
        self.residuals.iter().map(|r| r * r).sum::<f64>() / self.residuals.len().max(1) as f64
    }

    /// The largest |residual|.
    pub fn worst(&self) -> f64 {
        self.residuals.iter().fold(0.0, |m: f64, r| m.max(r.abs()))
    }
}

/// Residuals as a function of the parameter values.
pub type Residuals<'a> = &'a (dyn Fn(&[f64]) -> Vec<f64> + Sync);

/// `a·x = b` by Gaussian elimination with partial pivoting.
fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[p][k].abs() < 1e-300 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        for i in k + 1..n {
            let m = a[i][k] / a[k][k];
            for j in k..n {
                a[i][j] -= m * a[k][j];
            }
            b[i] -= m * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k][j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k][k];
    }
    Some(x)
}

/// Levenberg–Marquardt over the bounded parameters from their starting
/// values, minimising `Σ|r|^p` (p = 2 is least squares); the Jacobian's
/// columns shared among at most `threads` threads.
pub fn solve(params: &[Bounded], p: f64, raw: Residuals, threads: usize) -> Solution {
    let residuals = |v: &[f64]| -> Vec<f64> {
        let r = raw(v);
        if p == 2.0 {
            return r;
        }
        r.into_iter().map(|r| r.signum() * r.abs().powf(0.5 * p)).collect()
    };
    let values = |z: &[f64]| -> Vec<f64> { params.iter().zip(z).map(|(p, &z)| p.value(z)).collect() };
    let cost = |r: &[f64]| r.iter().map(|r| r * r).sum::<f64>();
    let mut z: Vec<f64> = params.iter().map(|p| p.free(p.start)).collect();
    let mut r = residuals(&values(&z));
    let mut c = cost(&r);
    let mut lambda = 1e-3;
    let n = z.len();
    let mut stalls = 0;
    for _ in 0..150 {
        let h = 1e-5;
        let column = |k: usize| -> Vec<f64> {
            let mut up = z.clone();
            let mut down = z.clone();
            up[k] += h;
            down[k] -= h;
            let (ru, rd) = (residuals(&values(&up)), residuals(&values(&down)));
            ru.iter().zip(&rd).map(|(a, b)| (a - b) / (2.0 * h)).collect()
        };
        let workers = threads.min(n).max(1);
        let jac: Vec<Vec<f64>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|w| {
                    let column = &column;
                    scope.spawn(move || (w..n).step_by(workers).map(|k| (k, column(k))).collect::<Vec<_>>())
                })
                .collect();
            let mut columns: Vec<(usize, Vec<f64>)> =
                handles.into_iter().flat_map(|h| h.join().expect("a Jacobian column")).collect();
            columns.sort_by_key(|(k, _)| *k);
            columns.into_iter().map(|(_, c)| c).collect()
        });
        let jtj: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| jac[i].iter().zip(&jac[j]).map(|(a, b)| a * b).sum()).collect())
            .collect();
        let jtr: Vec<f64> = (0..n).map(|i| jac[i].iter().zip(&r).map(|(a, b)| a * b).sum()).collect();
        let mut improved = false;
        while lambda < 1e12 {
            let mut a = jtj.clone();
            for i in 0..n {
                a[i][i] += lambda * jtj[i][i].max(1e-12);
            }
            let Some(step) = solve_dense(a, jtr.iter().map(|g| -g).collect()) else {
                lambda *= 4.0;
                continue;
            };
            let trial: Vec<f64> = z.iter().zip(&step).map(|(z, s)| z + s).collect();
            let rt = residuals(&values(&trial));
            let ct = cost(&rt);
            if ct.is_finite() && ct < c {
                stalls = if (c - ct) < 1e-10 * c { stalls + 1 } else { 0 };
                z = trial;
                r = rt;
                c = ct;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
        }
        if !improved || stalls >= 3 {
            break;
        }
    }
    let values = values(&z);
    Solution {
        residuals: raw(&values),
        values,
    }
}

/// The exponents of the continuation toward minimax: least squares,
/// then p = 4, 8, 16.
pub const MINIMAX: [f64; 4] = [2.0, 4.0, 8.0, 16.0];

/// [`solve`] at each exponent of `exponents` in turn, each from the
/// last; `stage` sees every stage's exponent and result as it ends (a
/// slow fit's progress survives an interruption).
pub fn continuation(
    params: &[Bounded],
    exponents: &[f64],
    residuals: Residuals,
    threads: usize,
    stage: &mut dyn FnMut(f64, &Solution),
) -> Solution {
    let mut params = params.to_vec();
    let mut fit = None;
    for &p in exponents {
        let f = solve(&params, p, residuals, threads);
        stage(p, &f);
        for (q, &v) in params.iter_mut().zip(&f.values) {
            q.start = v;
        }
        fit = Some(f);
    }
    fit.expect("at least one exponent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rosenbrock_valley() {
        let params = [Bounded::new(-1.2, -5.0, 5.0), Bounded::new(1.0, -5.0, 5.0)];
        let f = solve(&params, 2.0, &|v| vec![10.0 * (v[1] - v[0] * v[0]), 1.0 - v[0]], 1);
        assert!((f.values[0] - 1.0).abs() < 1e-6 && (f.values[1] - 1.0).abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn exponential_decay_recovered() {
        let t: Vec<f64> = (0..40).map(|i| f64::from(i) * 0.05).collect();
        let data: Vec<f64> = t.iter().map(|t| 2.5 * (-1.7 * t).exp()).collect();
        let residuals = |v: &[f64]| -> Vec<f64> { t.iter().zip(&data).map(|(t, d)| (v[0] * (-v[1] * t).exp() - d) / 1e-3).collect() };
        // Log-scaled bounds (a decade and more), from far off.
        let params = [Bounded::new(0.5, 0.01, 100.0), Bounded::new(10.0, 0.1, 100.0)];
        let f = solve(&params, 2.0, &residuals, 1);
        assert!((f.values[0] - 2.5).abs() < 1e-9 && (f.values[1] - 1.7).abs() < 1e-9, "{f:?}");
    }

    #[test]
    fn bounds_hold() {
        // Unconstrained minimum at 3; the bound stops it at 2.
        let params = [Bounded::new(1.0, 0.0, 2.0)];
        let f = solve(&params, 2.0, &|v| vec![v[0] - 3.0], 1);
        assert!(f.values[0] <= 2.0 && params[0].at_bound(f.values[0]), "{f:?}");
    }

    #[test]
    fn continuation_reaches_minimax() {
        // The best constant through 0, 0, 0, 1 by least squares is 0.25
        // (worst residual 0.75); the minimax one is 0.5 (worst 0.5).
        let data = [0.0, 0.0, 0.0, 1.0];
        let residuals = |v: &[f64]| -> Vec<f64> { data.iter().map(|d| v[0] - d).collect() };
        let params = [Bounded::new(0.9, -2.0, 2.0)];
        let ls = solve(&params, 2.0, &residuals, 1);
        assert!((ls.values[0] - 0.25).abs() < 1e-6);
        let mut seen = Vec::new();
        let mm = continuation(&params, &MINIMAX, &residuals, 1, &mut |p, s| seen.push((p, s.worst())));
        assert_eq!(seen.len(), 4);
        assert!(mm.worst() < 0.56 && mm.worst() < ls.worst(), "{seen:?}");
    }

    #[test]
    fn threads_do_not_change_the_result() {
        let t: Vec<f64> = (0..30).map(|i| f64::from(i) * 0.1).collect();
        let residuals = |v: &[f64]| -> Vec<f64> { t.iter().map(|t| v[0] * (v[1] * t).sin() + v[2] - (1.3 * (0.8 * t).sin() + 0.2)).collect() };
        let params = [Bounded::new(1.0, 0.1, 5.0), Bounded::new(0.7, 0.1, 2.0), Bounded::new(0.0, -1.0, 1.0)];
        let one = solve(&params, 2.0, &residuals, 1);
        let three = solve(&params, 2.0, &residuals, 3);
        assert_eq!(one, three);
    }
}
