//! Order statistics over a sample (block timings, mostly).

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct Summary {
    pub count: usize,
    pub mean: f64,
    pub min: f64,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Summary {
    /// `None` for an empty sample. NaNs sort last (never silently a min).
    pub fn of(xs: &[f64]) -> Option<Self> {
        if xs.is_empty() {
            return None;
        }
        let mut s = xs.to_vec();
        s.sort_by(f64::total_cmp);
        Some(Self {
            count: s.len(),
            mean: s.iter().sum::<f64>() / s.len() as f64,
            min: s[0],
            median: percentile(&s, 0.5),
            p95: percentile(&s, 0.95),
            p99: percentile(&s, 0.99),
            max: s[s.len() - 1],
        })
    }

    /// Every statistic mapped through `f` (unit conversions).
    pub fn map(self, f: impl Fn(f64) -> f64) -> Self {
        Self {
            count: self.count,
            mean: f(self.mean),
            min: f(self.min),
            median: f(self.median),
            p95: f(self.p95),
            p99: f(self.p99),
            max: f(self.max),
        }
    }
}

/// Nearest-rank percentile of an ascending sample, `p` ∈ [0, 1].
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_one_to_hundred() {
        let xs: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let s = Summary::of(&xs).unwrap();
        assert_eq!((s.count, s.min, s.max, s.mean), (100, 1.0, 100.0, 50.5));
        assert_eq!((s.median, s.p95, s.p99), (51.0, 95.0, 99.0));
    }

    #[test]
    fn empty_and_singleton() {
        assert!(Summary::of(&[]).is_none());
        let s = Summary::of(&[3.0]).unwrap();
        assert_eq!((s.min, s.median, s.p99, s.max), (3.0, 3.0, 3.0, 3.0));
    }

    #[test]
    fn nan_sorts_last() {
        let s = Summary::of(&[2.0, f64::NAN, 1.0]).unwrap();
        assert_eq!(s.min, 1.0);
        assert!(s.max.is_nan());
    }

    #[test]
    fn map_converts_units() {
        let s = Summary::of(&[1000.0, 2000.0])
            .unwrap()
            .map(|ns| ns / 1000.0);
        assert_eq!((s.min, s.max), (1.0, 2.0));
    }
}
