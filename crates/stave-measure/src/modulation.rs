//! Spectral motion: a time-invariant
//! memoryless system driven by one periodic tone can only emit energy on
//! the integer-harmonic comb. These measure the three ways real systems
//! escape it: multiplicative time-variance (sidebands),
//! constructed/emergent subharmonics (the half-integer comb), and period
//! multiplication (bifurcation).

use crate::spectrum::fft;

/// Off-comb energy around each harmonic line of a bin-centred capture.
pub struct SidebandReport {
    /// Total off-comb power within ±`half_bw` bins of the first `max_h`
    /// harmonic lines, in dB relative to the carrier line. This is the
    /// "skirt" energy a static curve cannot produce.
    pub total_db: f64,
    /// Per-harmonic skirt power (dB rel carrier), index 0 = around H1.
    pub per_harmonic_db: Vec<f64>,
    /// Strongest single off-comb bin: (offset in bins from its comb
    /// line, level in dB rel carrier).
    pub worst: (i64, f64),
}

/// Analyze a one-window bin-centred capture (fundamental at bin `m`).
/// Bins within ±1 of a comb line count as the line itself (guard band);
/// everything else within ±`half_bw` bins is skirt.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SidebandConfig {
    pub carrier_bin: usize,
    /// Bins either side of each comb line counted as its skirt.
    pub skirt_bins: usize,
    pub harmonics: usize,
}

pub fn sideband_report(out: &[f64], config: &SidebandConfig) -> SidebandReport {
    let SidebandConfig {
        carrier_bin: m,
        skirt_bins: half_bw,
        harmonics: max_h,
    } = *config;
    let n = out.len();
    let mut re = out.to_vec();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let pow = |b: usize| re[b] * re[b] + im[b] * im[b];
    let carrier = pow(m).max(1e-300);
    let mut total = 0.0f64;
    let mut per = Vec::new();
    let mut worst = (0i64, f64::NEG_INFINITY);
    for k in 1..=max_h {
        let line = k * m;
        if line + half_bw >= n / 2 {
            break;
        }
        let mut skirt = 0.0f64;
        for b in (line - half_bw)..=(line + half_bw) {
            // Guard: the line itself ±1 bin, and any bin that is ALSO a
            // comb line (adjacent harmonics inside the band).
            if b.abs_diff(line) <= 1 || b % m == 0 {
                continue;
            }
            let p = pow(b);
            skirt += p;
            let db = 10.0 * (p / carrier).log10();
            if db > worst.1 {
                worst = (b as i64 - line as i64, db);
            }
        }
        per.push(10.0 * (skirt / carrier).max(1e-30).log10());
        total += skirt;
    }
    SidebandReport {
        total_db: 10.0 * (total / carrier).max(1e-30).log10(),
        per_harmonic_db: per,
        worst,
    }
}

/// Energy on the half-integer comb (f0/2, 3·f0/2, 5·f0/2, …) relative to
/// the carrier. The carrier bin must be EVEN so the subharmonic comb is
/// bin-centred.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SubharmonicConfig {
    pub carrier_bin: usize,
    pub max_order: usize,
}

pub fn subharmonic_ratio_db(out: &[f64], config: &SubharmonicConfig) -> f64 {
    let SubharmonicConfig {
        carrier_bin: m,
        max_order: k_max,
    } = *config;
    assert!(
        m % 2 == 0,
        "fundamental bin must be even for a coherent half-comb"
    );
    let n = out.len();
    let mut re = out.to_vec();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let pow = |b: usize| re[b] * re[b] + im[b] * im[b];
    let carrier = pow(m).max(1e-300);
    let mut sub = 0.0f64;
    for j in 0..=k_max {
        let b = (2 * j + 1) * m / 2;
        if b >= n / 2 {
            break;
        }
        // ±1 bin around the half-line to catch instrument-window spill.
        sub += pow(b) + pow(b - 1) + pow(b + 1);
    }
    10.0 * (sub / carrier).max(1e-30).log10()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodClass {
    Period1,
    Period2,
    Aperiodic,
}

pub struct PeriodReport {
    pub class: PeriodClass,
    /// Cycle-strided residuals rel signal RMS (dB): d1 compares cycles one
    /// period apart, d2 two periods apart. Period-2 ⇒ d2 ≪ d1.
    pub d1_db: f64,
    pub d2_db: f64,
    /// Depth of the alternating (period-2) component, dB rel signal RMS.
    /// Meaningful when `class == Period2`.
    pub period2_depth_db: f64,
}

/// Classify periodicity by cycle-strided residuals with linear-interp
/// fractional strides. Feed a steady-state capture ≥ 6 periods long.
pub fn period_classify(out: &[f64], period: f64) -> PeriodReport {
    let n = out.len();
    let sig_rms = (out.iter().map(|x| x * x).sum::<f64>() / n as f64)
        .sqrt()
        .max(1e-30);
    let resid = |stride: f64| -> f64 {
        let last = (n as f64 - stride - 2.0) as usize;
        let mut acc = 0.0f64;
        for i in 0..last {
            let pos = i as f64 + stride;
            let j = pos as usize;
            let fr = pos - j as f64;
            let xs = out[j] + (out[j + 1] - out[j]) * fr;
            let d = out[i] - xs;
            acc += d * d;
        }
        (acc / last as f64).sqrt()
    };
    let d1 = resid(period) / sig_rms;
    let d2 = resid(2.0 * period) / sig_rms;
    let d1_db = 20.0 * d1.max(1e-15).log10();
    let d2_db = 20.0 * d2.max(1e-15).log10();
    let class = if d1_db < -40.0 {
        PeriodClass::Period1
    } else if d2_db < d1_db - 12.0 && d2_db < -40.0 {
        PeriodClass::Period2
    } else {
        PeriodClass::Aperiodic
    };
    PeriodReport {
        class,
        d1_db,
        d2_db,
        period2_depth_db: 20.0 * (d1 * 0.5).max(1e-15).log10(),
    }
}
