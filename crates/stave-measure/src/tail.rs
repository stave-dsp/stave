//! Tail-quality metrics — reverb character as numbers, not
//! opinions. Promoted from eddy's tail invariants so every field/reverb
//! project measures the same three things the ears grade:
//! * [`ned`] — normalized echo density (Abel/Huang): 1.0 is a fully
//!   dense Gaussian wash; spiky echo trains score low.
//! * [`autocorr_peak`] — comb-ness: a flangy/periodic tail shows
//!   its loop as a strong normalized-autocorrelation peak (eddy's
//!   retired single-loop plate measured ~0.9; its tank holds under
//!   0.35).
//! * [`t60_s`] and [`t60_band_s`] — the reverberation time, broadband
//!   and per octave band (ISO 3382-1's T30).
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

/// The reverberation time in one octave band (ISO 3382-1 T30, ISO 266
/// octaves): [`t60_s`] of `w` through a sixth-order Butterworth band
/// from `centre_hz/√2` to `centre_hz·√2` (IEC 61260's octave filter
/// shape: three high-pass and three low-pass sections). The filter runs
/// backwards in time, so its own ringing falls before the response and
/// outside the −5…−35 dB fit (the usual reverse filtering for short
/// decays). `None` as [`t60_s`], or when the band's top edge reaches
/// 0.49·fs.
pub fn t60_band_s(w: &[f32], sample_rate: f64, centre_hz: f64) -> Option<f64> {
    let (lo, hi) = (centre_hz / 2f64.sqrt(), centre_hz * 2f64.sqrt());
    if hi >= 0.49 * sample_rate || lo <= 0.0 {
        return None;
    }
    // Butterworth, sixth order: the three section Qs.
    const QS: [f64; 3] = [
        0.517_638_090_205_041_5,
        std::f64::consts::FRAC_1_SQRT_2,
        1.931_851_652_578_136_6,
    ];
    let mut x: Vec<f64> = w.iter().rev().map(|&v| f64::from(v)).collect();
    for highpass in [true, false] {
        let f = if highpass { lo } else { hi };
        for q in QS {
            let w0 = std::f64::consts::TAU * f / sample_rate;
            let (cos, alpha) = (w0.cos(), w0.sin() / (2.0 * q));
            let a0 = 1.0 + alpha;
            let (b0, b1) = if highpass {
                ((1.0 + cos) / 2.0, -(1.0 + cos))
            } else {
                ((1.0 - cos) / 2.0, 1.0 - cos)
            };
            let (b0, b1, b2) = (b0 / a0, b1 / a0, b0 / a0);
            let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha) / a0);
            let (mut z1, mut z2) = (0.0, 0.0);
            for v in x.iter_mut() {
                let y = b0 * *v + z1;
                z1 = b1 * *v - a1 * y + z2;
                z2 = b2 * *v - a2 * y;
                *v = y;
            }
        }
    }
    let band: Vec<f32> = x.iter().rev().map(|&v| v as f32).collect();
    t60_s(&band, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decaying noise whose T60 differs per octave band measures as its
    /// band's T60 in every band from 63 Hz to 8 kHz.
    #[test]
    fn t60_per_octave_band_of_band_limited_decays() {
        let rate = 48_000.0;
        let centres = [62.5, 250.0, 1_000.0, 4_000.0, 8_000.0];
        let t60s = [2.4, 1.8, 1.2, 0.8, 0.5];
        let n = (3.0 * rate) as usize;
        let mut w = vec![0.0f32; n];
        // Each band: noise through a resonator at its centre, decaying at
        // its T60.
        for (&fc, &t60) in centres.iter().zip(&t60s) {
            let mut lcg = 7u32 ^ fc as u32;
            let w0 = std::f64::consts::TAU * fc / rate;
            let (r, mut y1, mut y2) = (1.0 - w0 * 0.05, 0.0f64, 0.0f64);
            let per_sample = 10f64.powf(-3.0 / (t60 * rate));
            for (i, out) in w.iter_mut().enumerate() {
                lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = f64::from(lcg >> 8) / f64::from(1u32 << 23) - 1.0;
                let y = x + 2.0 * r * w0.cos() * y1 - r * r * y2;
                (y2, y1) = (y1, y);
                *out += (y * per_sample.powi(i as i32)) as f32;
            }
        }
        for (&fc, &t60) in centres.iter().zip(&t60s) {
            let measured = t60_band_s(&w, rate, fc).unwrap();
            assert!(
                (measured / t60 - 1.0).abs() < 0.03,
                "{fc} Hz: {measured} for {t60}"
            );
        }
        assert_eq!(t60_band_s(&w, rate, 20_000.0), None);
    }

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
