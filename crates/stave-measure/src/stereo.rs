//! The image of a channel pair: how min_correlation the channels are, how wide,
//! how balanced, how far apart in time, and what summing to mono costs.

use crate::level::{correlation, mean_square, power_db};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StereoAnalysis {
    /// The largest inter-channel delay searched for, in samples.
    pub max_lag: usize,
    /// A delay is reported only when the channels correlate at least
    /// this well at it.
    pub min_correlation: f64,
}

impl Default for StereoAnalysis {
    fn default() -> Self {
        Self {
            max_lag: 48,
            min_correlation: 0.5,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct StereoImage {
    /// +1 mono, 0 decorrelated, −1 opposite polarity.
    pub correlation: f64,
    /// Side level relative to mid: very negative for mono, 0 when equal.
    pub side_to_mid_db: f64,
    /// Right level relative to left.
    pub balance_db: f64,
    /// The mono sum (L+R)/2 relative to the mean channel power: 0 for
    /// mono, about −3 for decorrelated, very negative for opposite polarity.
    pub mono_sum_db: f64,
    /// Samples by which right lags left, when some lag makes the
    /// channels min_correlation.
    pub delay_samples: Option<i64>,
    pub identical: bool,
}

impl StereoAnalysis {
    pub fn analyze(&self, left: &[f32], right: &[f32]) -> StereoImage {
        let n = left.len().min(right.len());
        let (l, r) = (&left[..n], &right[..n]);
        let mid: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect();
        let side: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a - b)).collect();
        let (pl, pr) = (mean_square(l), mean_square(r));
        let pm = mean_square(&mid);
        StereoImage {
            correlation: correlation(l, r),
            side_to_mid_db: power_db(mean_square(&side)) - power_db(pm),
            balance_db: power_db(pr) - power_db(pl),
            mono_sum_db: power_db(pm) - power_db(0.5 * (pl + pr)),
            delay_samples: self.best_lag(l, r),
            identical: l.iter().zip(r).all(|(a, b)| a.to_bits() == b.to_bits()),
        }
    }

    fn best_lag(&self, l: &[f32], r: &[f32]) -> Option<i64> {
        let max = self.max_lag.min(l.len().saturating_sub(1)) as i64;
        (-max..=max)
            .map(|lag| {
                let (a, b) = if lag >= 0 {
                    (&l[..l.len() - lag as usize], &r[lag as usize..])
                } else {
                    (&l[(-lag) as usize..], &r[..r.len() - (-lag) as usize])
                };
                (lag, correlation(a, b))
            })
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .filter(|&(_, c)| c >= self.min_correlation)
            .map(|(lag, _)| lag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{Noise, Signal};

    fn noise(seed: u64) -> Vec<f32> {
        let n = Noise {
            seed,
            amplitude: 0.5,
        };
        (0..9_600).map(|f| n.at(0, f)).collect()
    }

    fn analyze(l: &[f32], r: &[f32]) -> StereoImage {
        StereoAnalysis::default().analyze(l, r)
    }

    #[test]
    fn mono_is_identical_correlated_and_loses_nothing_summed() {
        let x = noise(1);
        let s = analyze(&x, &x);
        assert!(s.identical);
        assert!((s.correlation - 1.0).abs() < 1e-9);
        assert!(s.mono_sum_db.abs() < 1e-9 && s.balance_db.abs() < 1e-9);
        assert!(s.side_to_mid_db < -200.0);
        assert_eq!(s.delay_samples, Some(0));
    }

    #[test]
    fn decorrelated_channels_sum_3_db_down_with_equal_mid_and_side() {
        let s = analyze(&noise(1), &noise(2));
        assert!(s.correlation.abs() < 0.05, "{s:?}");
        assert!((s.mono_sum_db + 3.01).abs() < 0.2, "{s:?}");
        assert!(s.side_to_mid_db.abs() < 0.5, "{s:?}");
        assert_eq!(s.delay_samples, None);
    }

    #[test]
    fn opposite_polarity_cancels_in_mono() {
        let x = noise(1);
        let inv: Vec<f32> = x.iter().map(|v| -v).collect();
        let s = analyze(&x, &inv);
        assert!((s.correlation + 1.0).abs() < 1e-9);
        assert!(s.mono_sum_db < -200.0);
    }

    #[test]
    fn finds_the_delay_and_the_balance() {
        let x = noise(1);
        let late: Vec<f32> = (0..x.len())
            .map(|i| if i >= 7 { 0.5 * x[i - 7] } else { 0.0 })
            .collect();
        let s = analyze(&x, &late);
        assert_eq!(s.delay_samples, Some(7));
        assert!((s.balance_db + 6.02).abs() < 0.1, "{s:?}");
    }
}
