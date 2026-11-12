//! Zipper noise: the off-carrier spray of a
//! parameter ramp. A legitimate gain ramp amplitude-modulates the carrier
//! with a smooth envelope, so its energy lives in a narrow skirt around
//! the carrier (bandwidth ~ 1/ramp-time). Zipper — block-rate steps — is
//! wideband: components at the block rate and its harmonics, mirrored
//! around the carrier across the whole audible band. A guard band around
//! the carrier counts as "the ramp"; everything else audible is spray.
//!
//! Departure from the no-window convention: a ramp capture is
//! non-stationary, so the circular FFT boundary sees an amplitude step. A
//! Hann window kills that wrap leakage; its sidelobes are below −140 dBc
//! beyond ~100 bins, far under any limit set here.

use crate::spectrum::fft;
use crate::window::Window;
use crate::AUDIBLE_MAX_HZ;

/// Spray classification of a gain-ramped tone capture.
pub struct ZipperReport {
    /// Total audible-band power outside the carrier guard band, dB
    /// relative to the in-guard (carrier + ramp skirt) power.
    pub spray_db: f64,
    /// Strongest single spray bin: (frequency in Hz, dB rel carrier).
    pub worst: (f64, f64),
}

/// How to read a capture whose gain ramped during the window.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ZipperConfig {
    pub carrier_bin: usize,
    pub sample_rate: f64,
    /// Half-width around the carrier counted as legitimate ramp
    /// modulation (a few × 1/ramp-time).
    pub guard_hz: f64,
    /// Spray below this is ignored — for engines whose automation moves
    /// a DC operating point (infrasonic by physics, not zipper).
    pub low_cut_hz: f64,
}

pub fn zipper_report(out: &[f64], config: &ZipperConfig) -> ZipperReport {
    let ZipperConfig {
        carrier_bin: m,
        sample_rate: fs,
        guard_hz,
        low_cut_hz: lo_hz,
    } = *config;
    let n = out.len();
    let hann = Window::Hann.coefficients(n);
    let mut re: Vec<f64> = out.iter().zip(&hann).map(|(x, w)| x * w).collect();
    let mut im = vec![0.0f64; n];
    fft(&mut re, &mut im);
    let hz_per_bin = fs / n as f64;
    let guard_bins = (guard_hz / hz_per_bin).ceil() as usize;
    let audible_top = ((AUDIBLE_MAX_HZ / hz_per_bin) as usize).min(n / 2 - 1);
    let pow = |b: usize| re[b] * re[b] + im[b] * im[b];

    let mut carrier = 0.0f64;
    for b in m.saturating_sub(guard_bins)..=(m + guard_bins).min(n / 2 - 1) {
        carrier += pow(b);
    }
    let carrier = carrier.max(1e-300);

    let lo_bin = ((lo_hz / hz_per_bin).floor() as usize).max(1);
    let mut spray = 0.0f64;
    let mut worst = (0.0f64, f64::NEG_INFINITY);
    for b in lo_bin..=audible_top {
        if b.abs_diff(m) <= guard_bins {
            continue;
        }
        let p = pow(b);
        spray += p;
        let db = 10.0 * (p / carrier).log10();
        if db > worst.1 {
            worst = (b as f64 * hz_per_bin, db);
        }
    }
    ZipperReport {
        spray_db: 10.0 * (spray / carrier).max(1e-30).log10(),
        worst,
    }
}
