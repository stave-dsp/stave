//! Tail-quality metrics — reverb character as numbers, not
//! opinions. Promoted from eddy's tail invariants so every field/reverb
//! project measures the same three things the ears grade:
//! * [`ned`] — normalized echo density (Abel/Huang): 1.0 is a fully
//!   dense Gaussian wash; spiky echo trains score low.
//! * [`autocorr_peak`] — comb-ness: a flangy/periodic tail shows
//!   its loop as a strong normalized-autocorrelation peak (eddy's
//!   retired single-loop plate measured ~0.9; its tank holds under
//!   0.35).
//! * [`edc_ripple_db`] — decay smoothness: RMS deviation of the
//!   Schroeder curve from a straight dB line between −5 and −30 dB;
//!   beating and warble read as ripple, a smooth exponential is
//!   what ears call "expensive".

/// Normalized echo density over a window: the fraction of samples
/// beyond the window's std, over the Gaussian expectation (0.3173).
pub fn ned(w: &[f32]) -> f64 {
    let n = w.len() as f64;
    let mean: f64 = w.iter().map(|x| f64::from(*x)).sum::<f64>() / n;
    let var: f64 = w
        .iter()
        .map(|x| {
            let d = f64::from(*x) - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    let sd = var.sqrt();
    let frac = w
        .iter()
        .filter(|x| (f64::from(**x) - mean).abs() > sd)
        .count() as f64
        / n;
    frac / 0.3173
}

/// Peak normalized autocorrelation over `lags` (samples), scanned on a
/// prime stride (7) so no comb can hide between probes.
pub fn autocorr_peak(w: &[f32], lags: std::ops::Range<usize>) -> f64 {
    let (min_lag, max_lag) = (lags.start, lags.end);
    let e: f64 = w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    let mut best = 0.0f64;
    let mut lag = min_lag;
    while lag < max_lag.min(w.len() / 2) {
        let mut acc = 0.0f64;
        for i in 0..(w.len() - lag) {
            acc += f64::from(w[i]) * f64::from(w[i + lag]);
        }
        let r = (acc / e.max(1e-30)).abs();
        if r > best {
            best = r;
        }
        lag += 7;
    }
    best
}

/// RMS deviation (dB) of the Schroeder energy-decay curve from the
/// straight line between its −5 dB and −30 dB crossings. Panics if
/// the tail is too short to hold that span.
pub fn edc_ripple_db(w: &[f32]) -> f64 {
    let mut edc: Vec<f64> = Vec::with_capacity(w.len());
    let mut acc = 0.0f64;
    for x in w.iter().rev() {
        acc += f64::from(*x) * f64::from(*x);
        edc.push(acc);
    }
    edc.reverse();
    let total = edc[0].max(1e-30);
    let db: Vec<f64> = edc.iter().map(|e| 10.0 * (e / total).log10()).collect();
    let i5 = db.iter().position(|d| *d <= -5.0).unwrap_or(0);
    let i30 = db.iter().position(|d| *d <= -30.0).unwrap_or(db.len() - 1);
    assert!(i30 > i5 + 10, "tail too short to measure a decay");
    let (x0, y0, x1, y1) = (i5 as f64, db[i5], i30 as f64, db[i30]);
    let slope = (y1 - y0) / (x1 - x0);
    let squared_error: f64 = db[i5..i30]
        .iter()
        .enumerate()
        .map(|(k, d)| (d - (y0 + slope * (k as f64 + i5 as f64 - x0))).powi(2))
        .sum();
    (squared_error / (i30 - i5) as f64).sqrt()
}

/// Reverberation time in seconds from an impulse response `w` at
/// `sample_rate`: the least-squares slope of the Schroeder energy-decay
/// curve between −5 and −35 dB (T30), extrapolated to 60 dB (ISO 3382-1).
/// `None` when the curve never falls to −35 dB inside `w` (truncate the
/// response where it meets the noise floor, or make it long enough).
pub fn t60_s(w: &[f32], sample_rate: f64) -> Option<f64> {
    let mut edc: Vec<f64> = Vec::with_capacity(w.len());
    let mut acc = 0.0f64;
    for x in w.iter().rev() {
        acc += f64::from(*x) * f64::from(*x);
        edc.push(acc);
    }
    edc.reverse();
    let total = *edc.first()?;
    if total <= 0.0 {
        return None;
    }
    let db = |e: f64| 10.0 * (e / total).log10();
    let start = edc.iter().position(|&e| db(e) <= -5.0)?;
    let end = edc.iter().position(|&e| db(e) <= -35.0)?;
    if end <= start + 2 {
        return None;
    }
    // Least-squares line through (n, dB) over [start, end).
    let n = (end - start) as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for (i, &e) in edc[start..end].iter().enumerate() {
        let (x, y) = (i as f64, db(e));
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let slope_db_per_sample = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    (slope_db_per_sample < 0.0).then(|| -60.0 / slope_db_per_sample / sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An exponential decay of known T60 measures as that T60.
    #[test]
    fn t60_of_an_exponential_decay() {
        let sample_rate = 48_000.0;
        for t60 in [0.3f64, 1.0, 2.5] {
            // Amplitude falls 60 dB in t60 seconds; noise-like signs.
            let per_sample = 10f64.powf(-3.0 / (t60 * sample_rate));
            let mut lcg = 1u32;
            let w: Vec<f32> = (0..(t60 * 1.5 * sample_rate) as usize)
                .map(|i| {
                    lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let sign = if lcg >> 31 == 0 { 1.0 } else { -1.0 };
                    (sign * per_sample.powi(i as i32)) as f32
                })
                .collect();
            let measured = t60_s(&w, sample_rate).unwrap();
            assert!((measured / t60 - 1.0).abs() < 0.02, "{t60}: {measured}");
        }
        assert_eq!(t60_s(&[0.0; 100], sample_rate), None);
        assert_eq!(t60_s(&[1.0; 100], sample_rate), None);
    }
}
