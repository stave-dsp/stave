//! The static nonlinearity of a block-oriented model: a polynomial or a
//! spline over the domain it was fitted on, held at its edge values
//! outside it.

use crate::behavioral::curve::Curve;

/// A memoryless curve `y = f(x)` over a domain; outside it `f` holds its
/// edge value (a fit says nothing there, and holding keeps the output
/// bounded).
#[derive(Clone, Debug, PartialEq)]
pub enum Nonlinearity {
    /// `f(x) = Σ c[k]·xᵏ`, `k = 0, 1, …`, over `[low, high]`.
    Polynomial {
        coefficients: Vec<f64>,
        low: f64,
        high: f64,
    },
    /// A cubic Hermite spline through `(knots[i], values[i])` whose
    /// tangents are the finite differences of the neighbouring knots
    /// (Catmull-Rom on a non-uniform grid; one-sided at the ends).
    /// Linear in its values, so a least-squares fit of them is one solve
    /// ([`spline_weights`]); knots need not be evenly spaced, so a fit
    /// puts them where the signal spends its time.
    Spline { knots: Vec<f64>, values: Vec<f64> },
}

/// The tangent at knot `i` as `(index, coefficient)` pairs over the
/// values.
fn tangent(knots: &[f64], i: usize) -> [(usize, f64); 2] {
    let last = knots.len() - 1;
    let (a, b) = if i == 0 {
        (0, 1)
    } else if i == last {
        (last - 1, last)
    } else {
        (i - 1, i + 1)
    };
    let d = 1.0 / (knots[b] - knots[a]);
    [(a, -d), (b, d)]
}

/// The knots and weights the spline on `knots` blends at `x`: `f(x) =
/// Σ w·values[i]`, at most four knots (`i − 1 … i + 2` around the
/// interval holding `x`; a slot may repeat an index with weight 0).
/// Outside the knots, the edge knot alone.
pub fn spline_weights(knots: &[f64], x: f64) -> [(usize, f64); 4] {
    let n = knots.len();
    debug_assert!(n >= 2);
    let last = n - 1;
    if x <= knots[0] {
        return [(0, 1.0), (0, 0.0), (0, 0.0), (0, 0.0)];
    }
    if x >= knots[last] {
        return [(last, 1.0), (last, 0.0), (last, 0.0), (last, 0.0)];
    }
    // The interval [knots[i], knots[i + 1]) holding x.
    let i = knots.partition_point(|&k| k <= x).saturating_sub(1).min(last - 1);
    let h = knots[i + 1] - knots[i];
    let t = (x - knots[i]) / h;
    let (t2, t3) = (t * t, t * t * t);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    // Slots for knots i−1, i, i+1, i+2.
    let base = i.saturating_sub(1);
    let mut w = [0.0f64; 4];
    let mut put = |k: usize, v: f64| w[k - base] += v;
    put(i, h00);
    put(i + 1, h01);
    for (k, c) in tangent(knots, i) {
        put(k, h * h10 * c);
    }
    for (k, c) in tangent(knots, i + 1) {
        put(k, h * h11 * c);
    }
    let at = |s: usize| (base + s).min(last);
    [(at(0), w[0]), (at(1), w[1]), (at(2), w[2]), (at(3), w[3])]
}

impl Nonlinearity {
    /// `f(x)`, held at the edge values outside the domain.
    #[inline]
    pub fn at(&self, x: f64) -> f64 {
        match self {
            Nonlinearity::Polynomial { coefficients, low, high } => {
                let x = x.clamp(*low, *high);
                coefficients.iter().rev().fold(0.0, |acc, c| acc * x + c)
            }
            Nonlinearity::Spline { knots, values } => {
                spline_weights(knots, x).iter().map(|&(i, w)| w * values[i]).sum()
            }
        }
    }

    /// `[low, high]`.
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Nonlinearity::Polynomial { low, high, .. } => (*low, *high),
            Nonlinearity::Spline { knots, .. } => (knots[0], knots[knots.len() - 1]),
        }
    }

    /// `f` never falls (or never rises) across its domain, judged on a
    /// grid of 4096 points.
    pub fn is_monotone(&self) -> bool {
        let (lo, hi) = self.domain();
        let ys: Vec<f64> = (0..4096).map(|i| self.at(lo + (hi - lo) * i as f64 / 4095.0)).collect();
        let rises = ys.windows(2).all(|w| w[1] >= w[0]);
        let falls = ys.windows(2).all(|w| w[1] <= w[0]);
        rises || falls
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        match self {
            Nonlinearity::Polynomial { coefficients, low, high } => {
                if !(low.is_finite() && high.is_finite() && high > low) {
                    Err(format!("domain [{low}, {high}] is empty or not finite"))
                } else if coefficients.is_empty() {
                    Err("a polynomial with no coefficients".into())
                } else if !coefficients.iter().all(|c| c.is_finite()) {
                    Err("a coefficient is not finite".into())
                } else {
                    Ok(())
                }
            }
            Nonlinearity::Spline { knots, values } => {
                if knots.len() < 2 || knots.len() != values.len() {
                    Err(format!("a spline needs two knots or more, a value each ({} knots, {} values)", knots.len(), values.len()))
                } else if !knots.iter().chain(values).all(|c| c.is_finite()) {
                    Err("a knot or value is not finite".into())
                } else if !knots.windows(2).all(|w| w[1] > w[0]) {
                    Err("the knots do not increase".into())
                } else {
                    Ok(())
                }
            }
        }
    }

    /// The curve tabulated with its antiderivatives for ADAA, referenced
    /// at `rest` (where the signal sits).
    pub(crate) fn tabulate(&self, rest: f64) -> Curve {
        let (lo, hi) = self.domain();
        let mut curve = Curve::flat();
        curve.rebuild(lo, hi, rest.clamp(lo, hi), |x| self.at(x));
        curve
    }
}
