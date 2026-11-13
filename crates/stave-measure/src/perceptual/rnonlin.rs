//! Rnonlin: how much a system's nonlinear distortion is heard, by the
//! correlation, band by band, between what went in and what came out
//! (Tan, Moore, Zacharov & Mattila 2004, as Moore 2020 and De Santis
//! 2007 describe it).
//!
//! # Models
//!
//! The ear's frequency analysis: both signals through an outer- and
//! middle-ear filter, then 40 fourth-order gammatone filters one ERB_N
//! wide, centred one ERB apart from 50 Hz. In each 30 ms frame `i` and
//! filter `j`, the distorted signal's frame is cross-correlated with the
//! original over frames `i−1, i, i+1` at lags within ±10 ms, normalised
//! (Moore 2020, eq. 6); `X_max` is the largest. Each frame's `X_max`
//! values are weighted by the distorted signal's level in that filter —
//! a full weight within 40 dB of the frame's loudest filter, none 80 dB
//! down, linear in dB between (De Santis 2007, 3.2) — normalised to sum
//! to one, summed, and the frames averaged. 1 is no distortion; lower is
//! more. A linear filter barely moves it (within one ERB a filter is
//! nearly a gain), a delay not at all.
//!
//! The mapping to a predicted rating, `S = (a + b·R^c)/(a + R^c)`, is
//! fitted per listening test and not applied here: fingerprints keep
//! `R`.
//!
//! # Derivation
//!
//! From the descriptions above; two substitutions, stated: the outer-
//! and middle-ear filter is BS.1387's (Terhardt's) response
//! ([`super::ear::outer_ear_db`]) applied in the frequency
//! domain, where Tan et al. use Glasberg & Moore's 4097-tap FIR (both
//! fall below 500 Hz and above 5 kHz); and each gammatone filter is
//! its frequency response `(1 + j(f − f_c)/b)^−4`, `b = 1.019·ERB(f_c)`,
//! applied to the whole signal's spectrum (zero-padded), in place of a
//! time-domain filter. Filters above 0.45 of the rate are left out.
//!
//! # Verified
//!
//! `crates/stave-measure/src/perceptual/rnonlin.rs::tests`: 1 for the
//! signal itself, a delay or a gain; a harder clip always lower; at
//! equal distortion power, higher-order distortion lower (Tan et al.'s
//! finding that the order, not the power, decides what is heard).
//!
//! # Cost
//!
//! `"Rnonlin (40 gammatone bands, 48 kHz)"` per analysed sample: 80
//! inverse FFTs of the padded signal, then three 4096-point FFTs per
//! frame and filter.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Signals of at least three frames (90 ms), the same length, roughly
//! time-aligned (within ±10 ms), any rate from 32 kHz.

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

use super::ear::outer_ear_db;
use super::{erb_hz, erb_number, erb_number_hz, real_spectrum};

pub const VERSION: u32 = 1;
/// Gammatone filters, one ERB apart.
pub const FILTERS: usize = 40;
/// The lowest filter's centre.
pub const LOW_HZ: f64 = 50.0;
pub const FRAME_SECONDS: f64 = 0.030;
pub const LAG_SECONDS: f64 = 0.010;

/// An Rnonlin measurement.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rnonlin {
    /// 1 for no distortion; lower is more.
    pub value: f64,
    pub frames: usize,
    pub filters: usize,
    pub version: u32,
}

/// The filters' centres at `rate`.
pub fn centres(rate: f64) -> Vec<f64> {
    let e0 = erb_number(LOW_HZ);
    (0..FILTERS).map(|j| erb_number_hz(e0 + j as f64)).filter(|&f| f < 0.45 * rate).collect()
}

/// Rnonlin of `distorted` against `original` (the same length, at `rate`).
pub fn rnonlin(original: &[f64], distorted: &[f64], rate: f64) -> Rnonlin {
    assert_eq!(original.len(), distorted.len(), "the signals differ in length");
    let len = original.len();
    let frame = (FRAME_SECONDS * rate).round() as usize;
    let lag = (LAG_SECONDS * rate).round() as usize;
    let frames = len / frame;
    assert!(frames >= 1, "a signal shorter than one 30 ms frame");
    // The whole signals, padded past the filters' ring, in the frequency domain.
    let n = (len + (0.1 * rate) as usize).next_power_of_two();
    let ome = |k: usize| {
        let hz = k as f64 * rate / n as f64;
        if k == 0 {
            0.0
        } else {
            10f64.powf(outer_ear_db(hz) / 20.0)
        }
    };
    let weigh = |x: &[f64]| -> Vec<Complex<f64>> {
        let mut s = real_spectrum(x, n);
        for (k, c) in s.iter_mut().enumerate() {
            *c = c.scale(ome(k));
        }
        s
    };
    let (xs, ys) = (weigh(original), weigh(distorted));
    let full = Fft::new(n);
    let band = |s: &[Complex<f64>], fc: f64| -> Vec<f64> {
        let b = 1.019 * erb_hz(fc);
        let mut spec: Vec<Complex<f64>> = s
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let hz = k as f64 * rate / n as f64;
                // (1 + j·u)^−4 = (1 − j·u)^4 / (1 + u²)^4
                let u = (hz - fc) / b;
                let d = Complex::new(1.0, -u);
                let d2 = d * d;
                (*c * (d2 * d2)).scale(1.0 / (1.0 + u * u).powi(4))
            })
            .collect();
        let mut out = vec![0.0; n];
        full.inverse_real(&mut spec, &mut out);
        out.truncate(len);
        for v in &mut out {
            *v /= n as f64;
        }
        out
    };
    let m = (frame + 2 * lag).next_power_of_two();
    let small = Fft::new(m);
    let centres = centres(rate);
    // Per filter and frame: X_max and the level.
    let mut xmax = vec![vec![0.0; centres.len()]; frames];
    let mut level = vec![vec![f64::NEG_INFINITY; centres.len()]; frames];
    let mut yf = vec![0.0; m];
    let mut xf = vec![0.0; m];
    let mut ys_ = vec![Complex::new(0.0, 0.0); m / 2 + 1];
    let mut xs_ = vec![Complex::new(0.0, 0.0); m / 2 + 1];
    let mut corr = vec![0.0; m];
    for (j, &fc) in centres.iter().enumerate() {
        let (x, y) = (band(&xs, fc), band(&ys, fc));
        // Prefix sums of x² for each lag's window energy.
        let mut cum = vec![0.0; len + 1];
        for (i, v) in x.iter().enumerate() {
            cum[i + 1] = cum[i] + v * v;
        }
        let energy = |a: isize, b: isize| {
            let (a, b) = (a.clamp(0, len as isize) as usize, b.clamp(0, len as isize) as usize);
            cum[b] - cum[a]
        };
        for i in 0..frames {
            let start = i * frame;
            let ey: f64 = y[start..start + frame].iter().map(|v| v * v).sum();
            level[i][j] = 10.0 * (ey / frame as f64).log10();
            if ey <= 0.0 {
                continue;
            }
            yf.iter_mut().for_each(|v| *v = 0.0);
            yf[..frame].copy_from_slice(&y[start..start + frame]);
            xf.iter_mut().for_each(|v| *v = 0.0);
            for (s, v) in xf.iter_mut().enumerate().take(frame + 2 * lag) {
                let at = start as isize + s as isize - lag as isize;
                if at >= 0 && (at as usize) < len {
                    *v = x[at as usize];
                }
            }
            small.forward_real(&yf, &mut ys_);
            small.forward_real(&xf, &mut xs_);
            for (a, b) in xs_.iter_mut().zip(&ys_) {
                *a = *a * b.conj();
            }
            small.inverse_real(&mut xs_, &mut corr);
            let mut best = f64::NEG_INFINITY;
            for s in 0..=2 * lag {
                let a = start as isize + s as isize - lag as isize;
                let ex = energy(a, a + frame as isize);
                if ex <= 0.0 {
                    continue;
                }
                let r = corr[s] / m as f64 / (ex * ey).sqrt();
                best = best.max(r);
            }
            xmax[i][j] = if best.is_finite() { best } else { 0.0 };
        }
    }
    let mut total = 0.0;
    let mut counted = 0;
    for i in 0..frames {
        let top = level[i].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !top.is_finite() {
            continue;
        }
        let w: Vec<f64> = level[i].iter().map(|&l| ((l - (top - 80.0)) / 40.0).clamp(0.0, 1.0)).collect();
        let sum: f64 = w.iter().sum();
        total += w.iter().zip(&xmax[i]).map(|(w, x)| w * x).sum::<f64>() / sum;
        counted += 1;
    }
    Rnonlin {
        value: if counted == 0 { 1.0 } else { total / counted as f64 },
        frames: counted,
        filters: centres.len(),
        version: VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    /// Noise shaped like program material: white noise through a gentle
    /// low-pass, peak 0.5.
    fn programme(len: usize) -> Vec<f64> {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut lp = 0.0;
        let raw: Vec<f64> = (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                lp = 0.9 * lp + 0.1 * (s as f64 / u64::MAX as f64 - 0.5);
                lp
            })
            .collect();
        let peak = raw.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        raw.iter().map(|v| 0.5 * v / peak).collect()
    }

    #[test]
    fn the_signal_itself_a_delay_and_a_gain_read_one() {
        let x = programme(9_600);
        assert!(rnonlin(&x, &x, RATE).value > 0.9999);
        let mut late = vec![0.0; 96]; // 2 ms
        late.extend(&x[..x.len() - 96]);
        assert!(rnonlin(&x, &late, RATE).value > 0.99);
        let half: Vec<f64> = x.iter().map(|v| 0.5 * v).collect();
        assert!(rnonlin(&x, &half, RATE).value > 0.9999);
    }

    #[test]
    fn a_harder_clip_reads_lower() {
        let x = programme(9_600);
        let mut last = 1.0;
        for limit in [0.4, 0.25, 0.15, 0.08] {
            let y: Vec<f64> = x.iter().map(|v| v.clamp(-limit, limit)).collect();
            let r = rnonlin(&x, &y, RATE).value;
            assert!(r < last, "clip at {limit}: {r} after {last}");
            last = r;
        }
        assert!(last < 0.95, "{last}");
    }

    /// Equal distortion power, rising order: the higher order is heard
    /// more (its products land farther from what masks them).
    #[test]
    fn higher_orders_read_lower_at_equal_power() {
        let x = programme(9_600);
        let distortion = |k: i32| -> Vec<f64> {
            // x + a·x^k with the residual after the best linear fit at
            // −30 dB re the signal.
            let d: Vec<f64> = x.iter().map(|v| (2.0 * v).powi(k)).collect();
            let fit = d.iter().zip(&x).map(|(a, b)| a * b).sum::<f64>() / x.iter().map(|v| v * v).sum::<f64>();
            let r: Vec<f64> = d.iter().zip(&x).map(|(a, b)| a - fit * b).collect();
            let (pr, px) = (r.iter().map(|v| v * v).sum::<f64>(), x.iter().map(|v| v * v).sum::<f64>());
            let a = (px / pr * 1e-3).sqrt();
            x.iter().zip(&r).map(|(v, e)| v + a * e).collect()
        };
        let r: Vec<f64> = [2, 3, 5, 7].iter().map(|&k| rnonlin(&x, &distortion(k), RATE).value).collect();
        assert!(r.windows(2).all(|w| w[1] < w[0]), "{r:?}");
    }
}
