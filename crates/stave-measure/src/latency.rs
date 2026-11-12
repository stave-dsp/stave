//! Latency from an impulse response: when the response first arrives
//! (onset) and where its energy peaks. For a linear-phase FIR chain the
//! peak sits at the group delay; for a minimum-phase chain the onset is
//! the honest number.

/// Onset threshold, relative to the response's peak magnitude.
pub const ONSET_THRESHOLD: f32 = 1e-3;

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ImpulseLatency {
    /// First frame above `ONSET_THRESHOLD` × peak.
    pub onset: usize,
    /// Frame of the largest magnitude (first, on ties).
    pub peak: usize,
}

/// `None` when the response is silent (or non-finite): no arrival.
pub fn impulse_latency(response: &[f32]) -> Option<ImpulseLatency> {
    let (peak, peak_mag) = response
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .fold((0, 0.0f32), |(bi, bm), (i, v)| {
            if v.abs() > bm {
                (i, v.abs())
            } else {
                (bi, bm)
            }
        });
    if peak_mag == 0.0 {
        return None;
    }
    let onset = response
        .iter()
        .position(|v| v.abs() > peak_mag * ONSET_THRESHOLD)?;
    Some(ImpulseLatency { onset, peak })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_delay() {
        let mut r = vec![0.0; 64];
        r[17] = 1.0;
        assert_eq!(
            impulse_latency(&r),
            Some(ImpulseLatency {
                onset: 17,
                peak: 17
            })
        );
    }

    #[test]
    fn symmetric_fir_peaks_at_its_centre_but_arrives_early() {
        let mut r = vec![0.0; 32];
        r[3..12].copy_from_slice(&[0.01, 0.05, 0.2, 0.6, 1.0, 0.6, 0.2, 0.05, 0.01]);
        assert_eq!(
            impulse_latency(&r),
            Some(ImpulseLatency { onset: 3, peak: 7 })
        );
    }

    #[test]
    fn negative_polarity_counts() {
        let mut r = vec![0.0; 8];
        r[2] = -0.5;
        assert_eq!(impulse_latency(&r).unwrap().peak, 2);
    }

    #[test]
    fn silence_and_nan_have_no_arrival() {
        assert_eq!(impulse_latency(&[0.0; 16]), None);
        assert_eq!(impulse_latency(&[f32::NAN; 4]), None);
        assert_eq!(impulse_latency(&[]), None);
    }
}
