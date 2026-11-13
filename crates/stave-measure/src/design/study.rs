//! Sweeps over a grid of parameter values, and each parameter's
//! sensitivity at the nominal design (DSP.md G2).

use super::{evaluate, index_of, nominal, Bench, Design, Error, Quantity};

/// A grid's measurements: one row per point.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    /// The swept parameters, in the grid's order.
    pub axes: Vec<String>,
    pub quantities: Vec<Quantity>,
    /// `(values of the axes, quantities)`, the last axis fastest.
    pub rows: Vec<(Vec<f64>, Vec<f64>)>,
}

/// Run `design` over every combination of the axes' values (the other
/// parameters at nominal), measuring `quantities` at each point.
///
/// # Models
/// No model: a study of whatever `design` builds.
///
/// # Derivation
/// The Cartesian product of the axes, the last fastest; each point a
/// fresh build of the model, measured on `bench`.
///
/// # Verified
/// `crates/stave-measure/src/design/tests.rs::a_sweep_follows_an_rc_corner`
/// (an RC's response over a grid of R against the analytic `|H|`, to
/// 1e-9 dB).
///
/// # Cost
/// One build and measurement per point, on [`Bench::threads`] threads.
///
/// # Aliasing
/// The models' own; a sine above a model's band aliases as it would in a host.
///
/// # Valid ranges
/// Any grid the model builds at.
pub fn sweep<D: Design>(design: &D, axes: &[(&str, Vec<f64>)], quantities: &[Quantity], bench: &Bench) -> Result<Sweep, Error> {
    if axes.is_empty() || axes.iter().any(|(_, v)| v.is_empty()) {
        return Err(Error::Empty("grid axis values"));
    }
    let index: Vec<usize> = axes.iter().map(|(n, _)| index_of(design, n)).collect::<Result<_, _>>()?;
    let base = nominal(design);
    let total: usize = axes.iter().map(|(_, v)| v.len()).product();
    let mut grid = Vec::with_capacity(total);
    let mut points = Vec::with_capacity(total);
    for mut k in 0..total {
        let mut at = vec![0.0; axes.len()];
        for a in (0..axes.len()).rev() {
            let n = axes[a].1.len();
            at[a] = axes[a].1[k % n];
            k /= n;
        }
        let mut values = base.clone();
        for (a, &i) in index.iter().enumerate() {
            values[i] = at[a];
        }
        grid.push(at);
        points.push(values);
    }
    let measured = evaluate(design, &points, quantities, bench)?;
    Ok(Sweep {
        axes: axes.iter().map(|(n, _)| n.to_ascii_lowercase()).collect(),
        quantities: quantities.to_vec(),
        rows: grid.into_iter().zip(measured).collect(),
    })
}

/// Each parameter's effect on each quantity at the nominal design.
#[derive(Clone, Debug, PartialEq)]
pub struct Sensitivity {
    pub parameters: Vec<String>,
    pub quantities: Vec<Quantity>,
    /// The quantities at the nominal design.
    pub nominal: Vec<f64>,
    /// `[parameter][quantity]`: the change per 1 % change of the
    /// parameter (dB or degrees per %), the slope at nominal.
    pub slopes: Vec<Vec<f64>>,
}

impl Sensitivity {
    /// The slope of `quantity` (its index) in `parameter`, per %.
    pub fn per_percent(&self, parameter: &str, quantity: usize) -> Option<f64> {
        let p = self.parameters.iter().position(|n| n.eq_ignore_ascii_case(parameter))?;
        self.slopes.get(p)?.get(quantity).copied()
    }

    /// Parameters by their largest |slope| over the quantities, largest
    /// first: which part moves the response most.
    pub fn ranked(&self) -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = self
            .parameters
            .iter()
            .zip(&self.slopes)
            .map(|(n, s)| (n.clone(), s.iter().fold(0.0f64, |m, x| m.max(x.abs()))))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }
}

/// Each parameter's sensitivity: the slope of every quantity in it, per
/// 1 % change, at the nominal design.
///
/// # Models
/// No model: a study of whatever `design` builds.
///
/// # Derivation
/// A central difference in each parameter's logarithm, relative step
/// `bench.step` (0.01 %): `dq/d(ln x) ≈ (q(x·(1+h)) − q(x·(1−h))) / (ln(1+h) − ln(1−h))`,
/// times 0.01, so a slope is the linearised change per 1 % of the value
/// (`x/100 · dq/dx`), phases unwrapped across the step. The truncation
/// error is O(h²) relative to the curvature.
///
/// # Verified
/// `crates/stave-measure/src/design/tests.rs::rc_and_rlc_sensitivities_match_analytic`
/// (an RC and a series RLC's response and phase slopes against their
/// closed forms, to 1e-6 of the slope) and
/// `::a_stage_thd_sensitivity_matches_analytic` (a cubic stage's THD and
/// gain slopes in its coefficient against the closed form, measured
/// through samples, to 1e-4).
///
/// # Cost
/// `2·parameters + 1` builds and measurements, on [`Bench::threads`] threads.
///
/// # Aliasing
/// The models' own.
///
/// # Valid ranges
/// Parameters with nonzero nominal values; quantities smooth across ±`step`.
pub fn sensitivity<D: Design>(design: &D, quantities: &[Quantity], bench: &Bench) -> Result<Sensitivity, Error> {
    let base = nominal(design);
    let h = bench.step;
    let mut points = vec![base.clone()];
    for i in 0..base.len() {
        for sign in [1.0, -1.0] {
            let mut v = base.clone();
            v[i] *= 1.0 + sign * h;
            points.push(v);
        }
    }
    let measured = evaluate(design, &points, quantities, bench)?;
    let per_percent = 0.01 / ((1.0 + h).ln() - (1.0 - h).ln());
    let slopes = (0..base.len())
        .map(|i| {
            let (up, down) = (&measured[1 + 2 * i], &measured[2 + 2 * i]);
            quantities
                .iter()
                .enumerate()
                .map(|(q, quantity)| {
                    let mut d = up[q] - down[q];
                    if matches!(quantity, Quantity::PhaseDegrees { .. }) {
                        d = (d + 180.0).rem_euclid(360.0) - 180.0;
                    }
                    d * per_percent
                })
                .collect()
        })
        .collect();
    Ok(Sensitivity {
        parameters: design.parameters().iter().map(|p| p.name.clone()).collect(),
        quantities: quantities.to_vec(),
        nominal: measured[0].clone(),
        slopes,
    })
}
