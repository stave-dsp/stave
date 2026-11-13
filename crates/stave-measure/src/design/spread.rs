//! Monte Carlo over tolerances (DSP.md G3): built units drawn from the
//! parameters' grades, the spread of each quantity across them, and
//! which grade dominates it.

use stave_dsp::circuit::netlist::tolerance::Tolerance;

use super::{evaluate, sensitivity, Bench, Design, Error, Quantity};

/// The quantities across a batch of built units.
#[derive(Clone, Debug, PartialEq)]
pub struct Spread {
    pub parameters: Vec<String>,
    pub quantities: Vec<Quantity>,
    /// The batch: the same seed builds the same units.
    pub seed: u64,
    /// The quantities at the nominal design.
    pub nominal: Vec<f64>,
    /// `[quantity][unit]`, in unit order.
    pub samples: Vec<Vec<f64>>,
    /// Each quantity's standard deviation as the sensitivities predict it
    /// (`√Σ (slope · σ)²`, the parts independent and the response
    /// linear across their tolerances).
    pub linear_sigma: Vec<f64>,
    /// The parameters by how much of the spread their grade causes,
    /// largest first.
    pub advice: Vec<Advice>,
}

/// One parameter's part in a spread.
#[derive(Clone, Debug, PartialEq)]
pub struct Advice {
    pub parameter: String,
    pub tolerance: Tolerance,
    /// Per quantity: the share of its variance this grade causes, to
    /// first order (0–1; the shares of a quantity sum to 1).
    pub shares: Vec<f64>,
    /// The largest of `shares`.
    pub share: f64,
}

impl Spread {
    pub fn units(&self) -> usize {
        self.samples.first().map_or(0, Vec::len)
    }

    /// The `p`th percentile (0–100) of quantity `q` across the units,
    /// interpolated between order statistics (Hyndman & Fan's type 7).
    pub fn percentile(&self, q: usize, p: f64) -> f64 {
        let mut x = self.samples[q].clone();
        x.sort_by(f64::total_cmp);
        let at = (p / 100.0).clamp(0.0, 1.0) * (x.len() - 1) as f64;
        let (lo, hi) = (at.floor() as usize, at.ceil() as usize);
        x[lo] + (at - lo as f64) * (x[hi] - x[lo])
    }

    pub fn mean(&self, q: usize) -> f64 {
        let x = &self.samples[q];
        x.iter().sum::<f64>() / x.len() as f64
    }

    /// The sample standard deviation of quantity `q`.
    pub fn sigma(&self, q: usize) -> f64 {
        let x = &self.samples[q];
        let m = self.mean(q);
        (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (x.len().max(2) - 1) as f64).sqrt()
    }

    /// One line of advice: the grade to tighten first, and the share it
    /// causes.
    pub fn verdict(&self) -> String {
        match self.advice.first() {
            Some(a) if a.share > 0.0 => format!(
                "{} (±{} %) causes up to {:.0} % of a quantity's variance: tighten it first",
                a.parameter,
                a.tolerance.fraction * 100.0,
                a.share * 100.0
            ),
            _ => "no part varies: every grade is exact".into(),
        }
    }
}

/// `units` built units of `design`, each parameter drawn from its grade
/// ([`Tolerance::factor`], seeded by `seed`, the unit's number and the
/// parameter's name — for a netlist, the very units
/// [`Tolerances::unit`](stave_dsp::circuit::netlist::tolerance::Tolerances::unit)
/// builds), each measured; with the sensitivities' first-order account
/// of which grade causes the spread.
///
/// # Models
/// No model: a study of whatever `design` builds.
///
/// # Derivation
/// Plain Monte Carlo: independent draws per part and unit. Percentiles
/// by interpolated order statistics; the advice's shares from the
/// slopes at nominal ([`sensitivity`]) and each grade's σ, the
/// first-order (delta-method) variance decomposition.
///
/// # Verified
/// `crates/stave-measure/src/design/tests.rs::rc_spread_matches_its_exact_distribution`:
/// an RC low-pass's response at three frequencies over 20 000 units
/// (R ±1 % and C ±10 % uniform), its 5th, 50th and 95th percentiles
/// against the exact distribution of `RC` (a product of independent
/// uniforms), within four of the order statistic's standard errors;
/// its σ against the linear prediction; the capacitor's share against
/// its exact 100²/(100² + 1²).
///
/// # Cost
/// `units + 2·parameters + 1` builds and measurements, on
/// [`Bench::threads`] threads.
///
/// # Aliasing
/// The models' own.
///
/// # Valid ranges
/// `units ≥ 2`; percentiles' sampling error falls as `1/√units`.
pub fn monte_carlo<D: Design>(
    design: &D,
    quantities: &[Quantity],
    units: usize,
    seed: u64,
    bench: &Bench,
) -> Result<Spread, Error> {
    if units < 2 {
        return Err(Error::Empty("units (at least two)"));
    }
    let params = design.parameters();
    let points: Vec<Vec<f64>> = (0..units as u64)
        .map(|u| {
            params
                .iter()
                .map(|p| p.nominal * p.tolerance.factor(seed, u, &p.name))
                .collect()
        })
        .collect();
    let measured = evaluate(design, &points, quantities, bench)?;
    let samples = (0..quantities.len())
        .map(|q| measured.iter().map(|m| m[q]).collect())
        .collect();
    let slopes = sensitivity(design, quantities, bench)?;
    // A slope per 1 % is 0.01·dq/dδ; a grade's σ is in δ.
    let contribution = |i: usize, q: usize| (slopes.slopes[i][q] * 100.0 * params[i].tolerance.sigma()).powi(2);
    let totals: Vec<f64> = (0..quantities.len())
        .map(|q| (0..params.len()).map(|i| contribution(i, q)).sum())
        .collect();
    let mut advice: Vec<Advice> = params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let shares: Vec<f64> = (0..quantities.len())
                .map(|q| if totals[q] > 0.0 { contribution(i, q) / totals[q] } else { 0.0 })
                .collect();
            Advice {
                parameter: p.name.clone(),
                tolerance: p.tolerance,
                share: shares.iter().fold(0.0f64, |m, s| m.max(*s)),
                shares,
            }
        })
        .collect();
    advice.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.parameter.cmp(&b.parameter)));
    Ok(Spread {
        parameters: params.iter().map(|p| p.name.clone()).collect(),
        quantities: quantities.to_vec(),
        seed,
        nominal: slopes.nominal.clone(),
        samples,
        linear_sigma: totals.iter().map(|t| t.sqrt()).collect(),
        advice,
    })
}
