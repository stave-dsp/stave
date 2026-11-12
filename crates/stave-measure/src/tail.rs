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
