//! Listening-test statistics: ABX by the exact binomial (p, power, the
//! critical score, d′) and MUSHRA-style rating summaries (mean, median,
//! a t-based 95 % interval). Pure numbers in, numbers out; stave-play's
//! voicing sessions log what these return.
//!
//! The ABX rules follow the literature the measurement design cites:
//!
//! - **One-sided exact binomial.** A guesser is right half the time, so
//!   the p-value is P(X ≥ k | n, ½); no normal approximation.
//! - **Power before verdicts** (Leventhal 1986). At α = 0.05 a listener
//!   whose true hit rate is 0.7 passes a 16-trial run only 45 % of the
//!   time, and a 40-trial run 81 %. A run that fails to reach significance
//!   therefore says nothing about audibility unless its power was high,
//!   and even then only about differences that large: [`AbxVerdict`]
//!   never says "inaudible".
//! - **d′ beside proportion correct** (Boley & Lester 2009): sensitivity
//!   apart from bias, under the ABX differencing model of Macmillan &
//!   Creelman, `Pc = Φ(d′/√2)·Φ(d′/√6) + Φ(−d′/√2)·Φ(−d′/√6)`.
//!
//! ```
//! use stave_measure::listening::{Abx, ALPHA, ASSUMED_HIT_RATE};
//!
//! let run = Abx { trials: 16, correct: 12 };
//! assert!((run.p_value() - 0.0384).abs() < 1e-4);
//! // The 16-trial design detects a 0.7 listener less than half the time.
//! assert!((Abx::power(16, ALPHA, ASSUMED_HIT_RATE) - 0.45).abs() < 0.01);
//! assert_eq!(Abx::critical(40, ALPHA), Some(26));
//! ```

/// The significance level voicing sessions use.
pub const ALPHA: f64 = 0.05;

/// The true hit rate power is quoted at (Leventhal 1986's moderate
/// listener; the measurement design's default).
pub const ASSUMED_HIT_RATE: f64 = 0.7;

/// The ABX default trial count: 81 % power at [`ASSUMED_HIT_RATE`].
pub const DEFAULT_ABX_TRIALS: u32 = 40;

/// The power a run must have before a non-significant result is reported
/// as "no difference found at that size" rather than "inconclusive".
pub const ADEQUATE_POWER: f64 = 0.8;

/// One ABX run: how many trials, how many answered correctly.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Abx {
    pub trials: u32,
    pub correct: u32,
}

/// What an ABX run lets you say. None of these is "inaudible".
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum AbxVerdict {
    /// Fewer trials than planned: no verdict.
    Incomplete { done: u32, planned: u32 },
    /// p < α: the listener told them apart.
    Detected { p: f64 },
    /// Not significant, and the run had too little power to say more.
    Inconclusive { p: f64, power: f64 },
    /// Not significant with adequate power: no difference as large as
    /// the assumed hit rate was found. Smaller ones are not excluded.
    NotDetected { p: f64, power: f64 },
}

impl AbxVerdict {
    /// One line for a session summary.
    pub fn describe(&self) -> String {
        match *self {
            Self::Incomplete { done, planned } => {
                format!("incomplete: {done} of {planned} planned trials, no verdict")
            }
            Self::Detected { p } => format!("difference detected (p = {p:.4})"),
            Self::Inconclusive { p, power } => format!(
                "inconclusive (p = {p:.3}); the run had only {:.0} % power to detect a \
                 {ASSUMED_HIT_RATE} hit rate. Not evidence of inaudibility.",
                power * 100.0
            ),
            Self::NotDetected { p, power } => format!(
                "no difference detected (p = {p:.3}) at {:.0} % power for a {ASSUMED_HIT_RATE} \
                 hit rate; smaller differences are not excluded",
                power * 100.0
            ),
        }
    }
}

impl Abx {
    /// Several runs of the same comparison, pooled (the same listener and
    /// pair across sessions; Reiss 2016 pools rather than judging each).
    pub fn pool(runs: &[Abx]) -> Abx {
        runs.iter().fold(Abx::default(), |a, r| Abx {
            trials: a.trials + r.trials,
            correct: a.correct + r.correct,
        })
    }

    /// Proportion correct (0.5 for no trials).
    pub fn proportion(&self) -> f64 {
        if self.trials == 0 {
            0.5
        } else {
            f64::from(self.correct) / f64::from(self.trials)
        }
    }

    /// One-sided p: the chance of `correct` or more by guessing.
    pub fn p_value(&self) -> f64 {
        binomial_tail(self.trials, self.correct, 0.5)
    }

    /// The smallest score significant at `alpha` in `trials` trials;
    /// `None` when even a perfect score is not (too few trials).
    pub fn critical(trials: u32, alpha: f64) -> Option<u32> {
        (0..=trials).find(|&k| binomial_tail(trials, k, 0.5) <= alpha)
    }

    /// The chance a listener whose true hit rate is `hit_rate` reaches
    /// the critical score in `trials` trials.
    pub fn power(trials: u32, alpha: f64, hit_rate: f64) -> f64 {
        Self::critical(trials, alpha).map_or(0.0, |k| binomial_tail(trials, k, hit_rate))
    }

    /// The fewest trials with at least `power` at `hit_rate` (searched to
    /// 10 000, beyond which a listening test is not the instrument).
    pub fn trials_for_power(hit_rate: f64, alpha: f64, power: f64) -> Option<u32> {
        (1..=10_000).find(|&n| Self::power(n, alpha, hit_rate) >= power)
    }

    /// The verdict once `planned` trials were intended.
    pub fn verdict(&self, planned: u32) -> AbxVerdict {
        if self.trials < planned {
            return AbxVerdict::Incomplete {
                done: self.trials,
                planned,
            };
        }
        let p = self.p_value();
        let power = Self::power(self.trials, ALPHA, ASSUMED_HIT_RATE);
        if p < ALPHA {
            AbxVerdict::Detected { p }
        } else if power >= ADEQUATE_POWER {
            AbxVerdict::NotDetected { p, power }
        } else {
            AbxVerdict::Inconclusive { p, power }
        }
    }

    /// d′ for this run's proportion correct (differencing model). A
    /// perfect or at-chance score is pulled in by half a trial (the
    /// usual 1/(2n) correction), so d′ stays finite.
    pub fn d_prime(&self) -> f64 {
        if self.trials == 0 {
            return 0.0;
        }
        let n = f64::from(self.trials);
        let pc = self.proportion().clamp(0.5 / n, 1.0 - 0.5 / n);
        abx_d_prime(pc)
    }

    /// The exact (Clopper–Pearson) 95 % interval of the true hit rate.
    pub fn hit_rate_interval(&self) -> (f64, f64) {
        let (n, k) = (self.trials, self.correct);
        if n == 0 {
            return (0.0, 1.0);
        }
        let lo = if k == 0 {
            0.0
        } else {
            // P(X ≥ k | p) rises with p: the p where it reaches 2.5 %.
            bisect(|p| binomial_tail(n, k, p) - 0.025, 0.0, 1.0)
        };
        let hi = if k == n {
            1.0
        } else {
            // P(X ≤ k | p) = 1 − P(X ≥ k+1 | p) falls to 2.5 % here.
            bisect(|p| binomial_tail(n, k + 1, p) - 0.975, 0.0, 1.0)
        };
        (lo, hi)
    }
}

/// P(X ≥ k) for X ~ Binomial(n, p), summed exactly in log space.
pub fn binomial_tail(n: u32, k: u32, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let (lp, lq) = (p.ln(), (1.0 - p).ln());
    let ln_n = ln_factorial(n);
    (k..=n)
        .map(|i| {
            let ln_c = ln_n - ln_factorial(i) - ln_factorial(n - i);
            (ln_c + f64::from(i) * lp + f64::from(n - i) * lq).exp()
        })
        .sum::<f64>()
        .min(1.0)
}

fn ln_factorial(n: u32) -> f64 {
    ln_gamma(f64::from(n) + 1.0)
}

/// Proportion correct in ABX for sensitivity `d` (differencing model).
pub fn abx_proportion_correct(d: f64) -> f64 {
    let (a, b) = (d / 2f64.sqrt(), d / 6f64.sqrt());
    normal_cdf(a) * normal_cdf(b) + normal_cdf(-a) * normal_cdf(-b)
}

/// The d′ giving proportion correct `pc` (inverse of
/// [`abx_proportion_correct`]); 0 at or below chance.
pub fn abx_d_prime(pc: f64) -> f64 {
    if pc <= 0.5 {
        return 0.0;
    }
    bisect(|d| abx_proportion_correct(d) - pc, 0.0, 12.0)
}

/// The standard normal CDF.
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / 2f64.sqrt())
}

/// Complementary error function (Numerical Recipes' Chebyshev fit,
/// fractional error below 1.2e-7 everywhere).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let r = t * poly.exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

/// ln Γ(x) for x > 0 (Lanczos, g = 7, nine terms; ~1e-15 relative).
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection keeps the series in its accurate range.
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let t = x + 7.5;
    let sum = G[1..]
        .iter()
        .enumerate()
        .fold(G[0], |s, (i, g)| s + g / (x + i as f64 + 1.0));
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + sum.ln()
}

/// The regularised incomplete beta I_x(a, b) (continued fraction,
/// modified Lentz).
fn incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_fraction(x, a, b) / a
    } else {
        1.0 - front * beta_fraction(1.0 - x, b, a) / b
    }
}

fn beta_fraction(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let mut c = 1.0;
    let mut d = 1.0 - (a + b) * x / (a + 1.0);
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=300 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        for aa in [
            m * (b - m) * x / ((a + m2 - 1.0) * (a + m2)),
            -(a + m) * (a + b + m) * x / ((a + m2) * (a + m2 + 1.0)),
        ] {
            d = 1.0 + aa * d;
            if d.abs() < TINY {
                d = TINY;
            }
            c = 1.0 + aa / c;
            if c.abs() < TINY {
                c = TINY;
            }
            d = 1.0 / d;
            h *= d * c;
        }
        if (d * c - 1.0).abs() < 1e-15 {
            break;
        }
    }
    h
}

/// Student's t CDF with `df` degrees of freedom.
pub fn student_t_cdf(t: f64, df: f64) -> f64 {
    let tail = 0.5 * incomplete_beta(df / (df + t * t), df / 2.0, 0.5);
    if t >= 0.0 {
        1.0 - tail
    } else {
        tail
    }
}

/// The `q` quantile of Student's t (e.g. 0.975 for a two-sided 95 %
/// interval).
pub fn student_t_quantile(q: f64, df: f64) -> f64 {
    bisect(|t| student_t_cdf(t, df) - q, -1e3, 1e3)
}

/// A root of increasing `f` on [lo, hi] (f(lo) ≤ 0 ≤ f(hi)).
fn bisect(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// One stimulus's ratings across trials (a MUSHRA column), summarised as
/// BS.1534-3 reports them: mean with a 95 % interval from Student's t,
/// plus the median (rating data are bounded and skewed, so the median and
/// non-parametric tests are the robust reading — Mendonça 2018).
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct RatingSummary {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    /// Sample standard deviation (n − 1); 0 for one rating.
    pub sd: f64,
    /// Half-width of the 95 % interval; `None` for one rating.
    pub ci95: Option<f64>,
}

impl RatingSummary {
    /// `None` for no ratings.
    pub fn of(ratings: &[f64]) -> Option<Self> {
        let n = ratings.len();
        if n == 0 {
            return None;
        }
        let mean = ratings.iter().sum::<f64>() / n as f64;
        let mut sorted = ratings.to_vec();
        sorted.sort_by(f64::total_cmp);
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
        };
        let (sd, ci95) = if n < 2 {
            (0.0, None)
        } else {
            let df = (n - 1) as f64;
            let sd = (ratings.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / df).sqrt();
            let t = student_t_quantile(0.975, df);
            (sd, Some(t * sd / (n as f64).sqrt()))
        };
        Some(Self {
            count: n,
            mean,
            median,
            sd,
            ci95,
        })
    }
}

/// BS.1534-3 post-screening: the hidden reference must score at least
/// this.
pub const HIDDEN_REFERENCE_FLOOR: f64 = 90.0;

/// BS.1534-3 post-screening: the share of items on which an assessor may
/// rate the hidden reference below [`HIDDEN_REFERENCE_FLOOR`] before
/// their ratings are excluded.
pub const SCREENING_SHARE: f64 = 0.15;

/// Whether ratings of the hidden reference (one per item) pass
/// BS.1534-3's post-screening.
pub fn passes_screening(hidden_reference: &[f64]) -> bool {
    if hidden_reference.is_empty() {
        return true;
    }
    let low = hidden_reference
        .iter()
        .filter(|&&r| r < HIDDEN_REFERENCE_FLOOR)
        .count();
    (low as f64) / (hidden_reference.len() as f64) <= SCREENING_SHARE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn critical_scores_match_the_published_table() {
        // α = 0.05, one-sided: 9/10, 12/16, 15/20, 18/25, 26/40.
        for (n, k) in [(10, 9), (16, 12), (20, 15), (25, 18), (40, 26)] {
            assert_eq!(Abx::critical(n, ALPHA), Some(k), "n = {n}");
        }
        // Four trials can never reach significance (1/16 > 0.05).
        assert_eq!(Abx::critical(4, ALPHA), None);
        assert_eq!(Abx::power(4, ALPHA, 0.9), 0.0);
    }

    #[test]
    fn power_matches_leventhal() {
        // Leventhal 1986: 16 trials detect a 0.7 listener 45 % of the
        // time; 40 trials 81 %. The rest of the 16-trial row: 0.8 → 0.80,
        // 0.9 → 0.98.
        assert!(close(Abx::power(16, ALPHA, 0.7), 0.4499, 1e-4));
        assert!(close(Abx::power(16, ALPHA, 0.8), 0.7982, 1e-4));
        assert!(close(Abx::power(16, ALPHA, 0.9), 0.9830, 1e-4));
        assert!(close(Abx::power(40, ALPHA, 0.7), 0.8074, 1e-4));
        assert!(close(Abx::power(DEFAULT_ABX_TRIALS, ALPHA, ASSUMED_HIT_RATE), 0.81, 0.01));
        // The default is the first trial count past 80 % … nearly: power
        // is not monotone in n (the critical score steps), so ask.
        let n = Abx::trials_for_power(0.7, ALPHA, 0.8).unwrap();
        assert!(n <= DEFAULT_ABX_TRIALS && Abx::power(n, ALPHA, 0.7) >= 0.8, "{n}");
    }

    #[test]
    fn p_values_are_exact_binomial_tails() {
        assert!(close(Abx { trials: 16, correct: 12 }.p_value(), 0.038_406_372, 1e-9));
        assert!(close(Abx { trials: 10, correct: 9 }.p_value(), 11.0 / 1024.0, 1e-12));
        assert_eq!(Abx { trials: 10, correct: 0 }.p_value(), 1.0);
        assert!(close(binomial_tail(20, 10, 0.5), 0.588_098_526, 1e-8));
    }

    #[test]
    fn verdicts_never_claim_inaudibility() {
        let planned = DEFAULT_ABX_TRIALS;
        assert!(matches!(
            Abx { trials: 10, correct: 5 }.verdict(planned),
            AbxVerdict::Incomplete { done: 10, .. }
        ));
        assert!(matches!(
            Abx { trials: 40, correct: 30 }.verdict(planned),
            AbxVerdict::Detected { .. }
        ));
        assert!(matches!(
            Abx { trials: 40, correct: 21 }.verdict(planned),
            AbxVerdict::NotDetected { .. }
        ));
        // A short planned run: not significant, and too weak to say more.
        let short = Abx { trials: 16, correct: 10 }.verdict(16);
        assert!(matches!(short, AbxVerdict::Inconclusive { .. }));
        for v in [short, Abx { trials: 40, correct: 21 }.verdict(planned)] {
            let text = v.describe().to_lowercase();
            assert!(!text.contains("inaudible") || text.contains("not evidence"), "{text}");
        }
    }

    #[test]
    fn d_prime_inverts_the_differencing_model() {
        assert!(close(abx_proportion_correct(0.0), 0.5, 1e-7));
        // Closed form at d′ = 1, 2, 3.
        assert!(close(abx_proportion_correct(1.0), 0.582_475, 1e-5));
        assert!(close(abx_proportion_correct(2.0), 0.746_820, 1e-5));
        assert!(close(abx_proportion_correct(3.0), 0.876_457, 1e-5));
        for d in [0.3, 1.0, 2.5, 4.0] {
            assert!(close(abx_d_prime(abx_proportion_correct(d)), d, 1e-5), "{d}");
        }
        assert_eq!(abx_d_prime(0.4), 0.0);
        // Perfect runs stay finite.
        let perfect = Abx { trials: 40, correct: 40 }.d_prime();
        assert!(perfect.is_finite() && perfect > 4.0, "{perfect}");
    }

    #[test]
    fn normal_cdf_matches_tables() {
        for (x, p) in [(0.0, 0.5), (1.0, 0.841_344_7), (1.959_964, 0.975), (-2.0, 0.022_750_1)] {
            assert!(close(normal_cdf(x), p, 2e-7), "{x}");
        }
    }

    #[test]
    fn t_quantiles_match_the_table() {
        // Two-sided 95 % critical values.
        for (df, t) in [(1.0, 12.706), (2.0, 4.303), (5.0, 2.571), (10.0, 2.228), (30.0, 2.042)] {
            assert!(close(student_t_quantile(0.975, df), t, 1e-3), "df = {df}");
        }
    }

    #[test]
    fn hit_rate_interval_brackets_the_proportion() {
        let (lo, hi) = Abx { trials: 40, correct: 26 }.hit_rate_interval();
        // Clopper–Pearson for 26/40: 0.483–0.794.
        assert!(close(lo, 0.4832, 1e-3) && close(hi, 0.7937, 1e-3), "{lo} {hi}");
        assert_eq!(Abx { trials: 5, correct: 5 }.hit_rate_interval().1, 1.0);
    }

    #[test]
    fn rating_summaries() {
        let s = RatingSummary::of(&[80.0, 90.0, 70.0, 100.0]).unwrap();
        assert!(close(s.mean, 85.0, 1e-12) && close(s.median, 85.0, 1e-12));
        assert!(close(s.sd, 12.909_944, 1e-6));
        // t(0.975, 3) = 3.182: half-width 3.182 × 12.91 / 2.
        assert!(close(s.ci95.unwrap(), 20.542, 1e-2));
        assert_eq!(RatingSummary::of(&[50.0]).unwrap().ci95, None);
        assert!(RatingSummary::of(&[]).is_none());
    }

    #[test]
    fn screening_follows_bs1534() {
        assert!(passes_screening(&[100.0; 10]));
        // One low item in ten: 10 % ≤ 15 %.
        assert!(passes_screening(&[100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 85.0]));
        assert!(!passes_screening(&[100.0, 80.0, 100.0, 70.0]));
    }

    #[test]
    fn pooling_sums_runs() {
        let p = Abx::pool(&[Abx { trials: 16, correct: 12 }, Abx { trials: 24, correct: 14 }]);
        assert_eq!(p, Abx { trials: 40, correct: 26 });
    }
}
