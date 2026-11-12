//! Comparing two runs: bit-exact divergence, and the residual of a
//! null test (how far apart, in dB, two signals that should match are).

use crate::level::{db, mean_square, power_db};

/// Where two multichannel signals first differ bitwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Divergence {
    pub channel: usize,
    pub frame: usize,
}

/// First bitwise difference, comparing channels pairwise over their
/// common length. (Bitwise: −0.0 ≠ 0.0, and a NaN never matches — a
/// run that differs in either is not reproducible.)
pub fn first_divergence(a: &[Vec<f32>], b: &[Vec<f32>]) -> Option<Divergence> {
    if a.len() != b.len() {
        return Some(Divergence {
            channel: a.len().min(b.len()),
            frame: 0,
        });
    }
    a.iter().zip(b).enumerate().find_map(|(channel, (x, y))| {
        let frame = x
            .iter()
            .zip(y)
            .position(|(p, q)| p.to_bits() != q.to_bits() || p.is_nan());
        let frame = frame.or((x.len() != y.len()).then(|| x.len().min(y.len())));
        frame.map(|frame| Divergence { channel, frame })
    })
}

/// A null test: `actual` against `expected`, sample-aligned.
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct Null {
    /// RMS of the difference, dBFS.
    pub residual_dbfs: f64,
    /// Difference power relative to `expected`'s (0 dB = as loud as
    /// the signal; −∞ toward a perfect null).
    pub residual_rel_db: f64,
    /// Largest single-sample difference, dBFS.
    pub peak_diff_dbfs: f64,
}

pub fn null_test(actual: &[f32], expected: &[f32]) -> Null {
    let n = actual.len().min(expected.len());
    let diff: Vec<f32> = actual[..n]
        .iter()
        .zip(&expected[..n])
        .map(|(a, e)| a - e)
        .collect();
    let dpow = mean_square(&diff);
    Null {
        residual_dbfs: power_db(dpow),
        residual_rel_db: power_db(dpow) - power_db(mean_square(&expected[..n])),
        peak_diff_dbfs: db(diff.iter().fold(0.0f64, |m, &d| m.max(f64::from(d).abs()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_renders_do_not_diverge() {
        let a = vec![vec![0.1, 0.2], vec![0.3, 0.4]];
        assert_eq!(first_divergence(&a, &a.clone()), None);
    }

    #[test]
    fn reports_first_channel_and_frame() {
        let a = vec![vec![0.0; 4], vec![0.0; 4]];
        let mut b = a.clone();
        b[1][2] = 1e-30;
        assert_eq!(
            first_divergence(&a, &b),
            Some(Divergence {
                channel: 1,
                frame: 2
            })
        );
    }

    #[test]
    fn signed_zero_nan_and_length_mismatches_diverge() {
        assert!(first_divergence(&[vec![0.0]], &[vec![-0.0]]).is_some());
        assert!(first_divergence(&[vec![f32::NAN]], &[vec![f32::NAN]]).is_some());
        assert_eq!(
            first_divergence(&[vec![1.0, 2.0]], &[vec![1.0]]),
            Some(Divergence {
                channel: 0,
                frame: 1
            })
        );
        assert!(first_divergence(&[vec![]], &[]).is_some());
    }

    #[test]
    fn null_of_a_six_db_mismatch() {
        let e: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.1).sin()).collect();
        let a: Vec<f32> = e.iter().map(|v| v * 0.5).collect();
        let n = null_test(&a, &e);
        assert!((n.residual_rel_db + 6.0206).abs() < 1e-3, "{n:?}");
        assert!(null_test(&e, &e).residual_rel_db < -200.0);
    }
}
