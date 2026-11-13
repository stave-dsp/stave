//! ITU-R BS.1387's FFT ear model, basic version, as Kabal's 2002
//! examination of the Recommendation reads it (section numbers below are
//! Kabal's): the steps from a frame of samples to the excitation pattern
//! a masking threshold is read from.
//!
//! - **Frames** (2.1–2.3): 2048 samples at 48 kHz, hopped by half, under
//!   the Hann window `½(1 − cos(2πn/(N−1)))` and a plain DFT. At another
//!   rate the frame keeps its length in time (the power of two nearest
//!   `2048·rate/48000`): BS.1387 is defined at 48 kHz only, and this is
//!   the one extension.
//! - **Level** (2.4): a full-scale sine at 1019.5 Hz peaks at the
//!   playback level (92 dB SPL unless stated) in the DFT, with Kabal's
//!   analytic `γ` (0.8497 at 48 kHz) for the tone falling between bins,
//!   in place of the Recommendation's measured normalisation.
//! - **Outer and middle ear** (2.5): `A(f) = −2.184·f^−0.8 + 6.5·e^(−0.6(f−3.3)²) − 0.001·f^3.6`
//!   dB, `f` in kHz.
//! - **Critical bands** (2.6): 109 bands a quarter Bark wide, `z = 7·asinh(f/650)`,
//!   from 80 Hz to 18 kHz; each DFT bin spread over its width (eq. 13),
//!   every band floored at 10⁻¹².
//! - **Internal noise** (2.7): `1.456·f^−0.8` dB added to each band.
//! - **Frequency spreading** (2.8): the triangular spreading function,
//!   27 dB/Bark below and `24 + 230/f − 2·log10 E` dB/Bark above (level
//!   dependent), normalised to unit area, summed with exponent 0.4 and
//!   normalised by its response to a flat 0 dB pattern (`B_s`).
//! - **Time spreading** (2.9): first-order smoothing per band with
//!   `τ = 8 ms + (100/f_c)·22 ms`, the excitation following rises at once.
//!
//! The masking offset and the noise pattern belong to the NMR and live
//! in [`nmr`](super::nmr).

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

/// The frame at 48 kHz.
pub const FRAME_48K: usize = 2048;
/// The basic version's band width, in Bark.
pub const DZ: f64 = 0.25;
/// The band range.
pub const LOW_HZ: f64 = 80.0;
pub const HIGH_HZ: f64 = 18_000.0;
/// The floor every band energy is held at.
pub const E_MIN: f64 = 1e-12;
/// The tone the level is calibrated with (BS.1387: 1019.5 Hz).
pub const CALIBRATION_HZ: f64 = 1_019.5;

/// Bark (Kabal eq. 9).
pub fn bark(hz: f64) -> f64 {
    7.0 * (hz / 650.0).asinh()
}

/// Hz of a Bark value (eq. 10).
pub fn bark_hz(z: f64) -> f64 {
    650.0 * (z / 7.0).sinh()
}

/// The outer and middle ear's response in dB at `hz` (eq. 6); −∞ at DC.
pub fn outer_ear_db(hz: f64) -> f64 {
    let f = hz / 1000.0;
    if f <= 0.0 {
        return f64::NEG_INFINITY;
    }
    -2.184 * f.powf(-0.8) + 6.5 * (-0.6 * (f - 3.3).powi(2)).exp() - 0.001 * f.powf(3.6)
}

/// The internal noise's energy in a band centred at `hz` (eq. 18).
pub fn internal_noise(hz: f64) -> f64 {
    10f64.powf(1.456 * (hz / 1000.0).powf(-0.8) / 10.0)
}

/// The 109 bands: lower edge, centre and upper edge in Hz (eqs. 11–12).
#[derive(Clone, Debug, PartialEq)]
pub struct Bands {
    pub lower: Vec<f64>,
    pub centre: Vec<f64>,
    pub upper: Vec<f64>,
}

impl Bands {
    pub fn basic() -> Self {
        let (zl, zu) = (bark(LOW_HZ), bark(HIGH_HZ));
        let count = ((zu - zl) / DZ).ceil() as usize;
        let mut b = Bands {
            lower: Vec::with_capacity(count),
            centre: Vec::with_capacity(count),
            upper: Vec::with_capacity(count),
        };
        for i in 0..count {
            let lo = zl + i as f64 * DZ;
            let hi = (zl + (i + 1) as f64 * DZ).min(zu);
            b.lower.push(bark_hz(lo));
            b.centre.push(bark_hz(0.5 * (lo + hi)));
            b.upper.push(bark_hz(hi));
        }
        b
    }

    pub fn len(&self) -> usize {
        self.centre.len()
    }

    pub fn is_empty(&self) -> bool {
        self.centre.is_empty()
    }
}

/// The frame length at `rate`: 2048 at 48 kHz, the same duration
/// (nearest power of two) elsewhere.
pub fn frame_len(rate: f64) -> usize {
    let n = FRAME_48K as f64 * rate / 48_000.0;
    let lo = (n.log2().floor()).exp2();
    let pick = if n / lo < 2f64.sqrt() { lo } else { 2.0 * lo };
    (pick as usize).max(256)
}

/// BS.1387's Hann window over `n` points.
pub fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos()))
        .collect()
}

/// Kabal's `γ`: the window's response to a tone `delta` bins off a bin
/// centre, relative to its response on the centre (eq. 5's 0.8497 for
/// 1019.5 Hz at 48 kHz).
pub fn gamma(window: &[f64], delta: f64) -> f64 {
    let n = window.len() as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, w) in window.iter().enumerate() {
        let a = -std::f64::consts::TAU * delta * i as f64 / n;
        re += w * a.cos();
        im += w * a.sin();
    }
    (re * re + im * im).sqrt() / window.iter().sum::<f64>()
}

/// One bin's share of a band (eq. 13): `(band, bin, fraction)`.
#[derive(Copy, Clone, Debug)]
struct Share {
    band: u16,
    bin: u32,
    fraction: f64,
}

/// The ear model at one rate and playback level, with an optional extra
/// weighting applied with the outer ear (A-weighting, for the ANMR).
#[derive(Clone, Debug)]
pub struct Ear {
    pub rate: f64,
    pub frame: usize,
    pub bands: Bands,
    window: Vec<f64>,
    fft: Fft<f64>,
    /// Per bin: the level calibration, the outer ear and any extra
    /// weighting, as a power gain.
    gain: Vec<f64>,
    shares: Vec<Share>,
    internal: Vec<f64>,
    /// Per band: the lower slope's factor per band (`10^(−2.7·Δz)`),
    /// the level-free part of the upper slope, `B_s`, and `α`.
    upper_free: Vec<f64>,
    spread_norm: Vec<f64>,
    alpha: Vec<f64>,
}

impl Ear {
    /// The model at `rate`, a full-scale sine (`full_scale`) playing at
    /// `db_spl`, with `weight(hz)` (an amplitude gain) applied with the
    /// outer ear.
    pub fn new(rate: f64, full_scale: f64, db_spl: f64, weight: impl Fn(f64) -> f64) -> Self {
        let frame = frame_len(rate);
        let window = hann(frame);
        let bin_hz = rate / frame as f64;
        let cal_bin = CALIBRATION_HZ / bin_hz;
        let delta = cal_bin - cal_bin.round();
        // A sine of amplitude a on a bin reads a/2·Σw; off a bin, γ less.
        let peak = 0.5 * full_scale * gamma(&window, delta) * window.iter().sum::<f64>();
        let level = 10f64.powf(db_spl / 20.0) / peak;
        let gain = (0..=frame / 2)
            .map(|k| {
                let hz = k as f64 * bin_hz;
                if k == 0 {
                    0.0
                } else {
                    let w = 10f64.powf(outer_ear_db(hz) / 20.0) * weight(hz);
                    (level * w).powi(2)
                }
            })
            .collect();
        let bands = Bands::basic();
        let mut shares = Vec::new();
        for (i, (&lo, &hi)) in bands.lower.iter().zip(&bands.upper).enumerate() {
            let first = ((lo / bin_hz - 0.5).floor().max(0.0)) as u32;
            let last = ((hi / bin_hz + 0.5).ceil() as u32).min((frame / 2) as u32);
            for k in first..=last {
                let (a, b) = ((f64::from(k) - 0.5) * bin_hz, (f64::from(k) + 0.5) * bin_hz);
                let fraction = (hi.min(b) - lo.max(a)).max(0.0) / bin_hz;
                if fraction > 0.0 {
                    shares.push(Share {
                        band: i as u16,
                        bin: k,
                        fraction,
                    });
                }
            }
        }
        let internal = bands.centre.iter().map(|&f| internal_noise(f)).collect();
        let upper_free = bands.centre.iter().map(|&f| -24.0 - 230.0 / f).collect();
        let frame_rate = rate / (frame / 2) as f64;
        let alpha = bands
            .centre
            .iter()
            .map(|&f| {
                let tau = 0.008 + 100.0 / f * (0.030 - 0.008);
                (-1.0 / (frame_rate * tau)).exp()
            })
            .collect();
        let mut ear = Ear {
            rate,
            frame,
            bands,
            window,
            fft: Fft::new(frame),
            gain,
            shares,
            internal,
            upper_free,
            spread_norm: Vec::new(),
            alpha,
        };
        // B_s: the spread of a flat 0 dB pattern (eq. 20).
        let flat = vec![1.0; ear.bands.len()];
        ear.spread_norm = vec![1.0; ear.bands.len()];
        ear.spread_norm = ear.spread_unnormalised(&flat);
        ear
    }

    /// The hop between frames.
    pub fn hop(&self) -> usize {
        self.frame / 2
    }

    /// How many whole frames `len` samples hold.
    pub fn frames(&self, len: usize) -> usize {
        if len < self.frame {
            0
        } else {
            (len - self.frame) / self.hop() + 1
        }
    }

    /// The calibrated, ear-weighted spectrum of frame `index` of `x`:
    /// each bin's amplitude (`|X_w|`, eq. 8).
    pub fn spectrum(&self, x: &[f64], index: usize) -> Vec<f64> {
        let start = index * self.hop();
        let buf: Vec<f64> = x[start..start + self.frame]
            .iter()
            .zip(&self.window)
            .map(|(s, w)| s * w)
            .collect();
        let mut out = vec![Complex::new(0.0, 0.0); self.frame / 2 + 1];
        self.fft.forward_real(&buf, &mut out);
        out.iter()
            .zip(&self.gain)
            .map(|(c, g)| (c.norm_sqr() * g).sqrt())
            .collect()
    }

    /// Bin powers grouped into the bands (eqs. 14–16), floored at
    /// [`E_MIN`].
    pub fn group(&self, power: &[f64]) -> Vec<f64> {
        let mut e = vec![0.0; self.bands.len()];
        for s in &self.shares {
            e[s.band as usize] += s.fraction * power[s.bin as usize];
        }
        for v in &mut e {
            *v = v.max(E_MIN);
        }
        e
    }

    /// A pitch pattern: grouped powers plus the internal noise (eq. 17).
    pub fn pitch(&self, power: &[f64]) -> Vec<f64> {
        let mut e = self.group(power);
        for (v, n) in e.iter_mut().zip(&self.internal) {
            *v += n;
        }
        e
    }

    /// The unsmeared excitation pattern of a pitch pattern (eq. 19).
    pub fn spread(&self, e: &[f64]) -> Vec<f64> {
        self.spread_unnormalised(e)
            .iter()
            .zip(&self.spread_norm)
            .map(|(s, b)| s / b)
            .collect()
    }

    fn spread_unnormalised(&self, e: &[f64]) -> Vec<f64> {
        let n = e.len();
        let lower = 10f64.powf(-2.7 * DZ); // per band, going down
        let mut acc = vec![0.0; n];
        for l in 0..n {
            let upper_db = self.upper_free[l] + 2.0 * e[l].log10();
            let upper = 10f64.powf(upper_db * DZ / 10.0); // per band, going up
            // A(l, E): the spreading function's area over the bands.
            let below: f64 = (1..=l).map(|d| lower.powi(d as i32)).sum();
            let above: f64 = (1..n - l).map(|d| upper.powi(d as i32)).sum();
            let area = 1.0 + below + above;
            let base = (e[l] / area).powf(0.4);
            let (lo4, up4) = (lower.powf(0.4), upper.powf(0.4));
            let mut f = base;
            acc[l] += f;
            for i in (0..l).rev() {
                f *= lo4;
                acc[i] += f;
            }
            f = base;
            for a in acc.iter_mut().skip(l + 1) {
                f *= up4;
                *a += f;
            }
        }
        acc.iter().map(|a| a.powf(2.5)).collect()
    }

    /// Time spreading (eq. 30): `state` holds `E_f` from the frame
    /// before (zeros at the start); returns the excitation pattern.
    pub fn smear(&self, state: &mut [f64], unsmeared: &[f64]) -> Vec<f64> {
        state
            .iter_mut()
            .zip(unsmeared)
            .zip(&self.alpha)
            .map(|((f, &e), &a)| {
                *f = a * *f + (1.0 - a) * e;
                f.max(e)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 109 bands whose edges agree with BS.1387's printed table (Table 6
    /// of BS.1387-2, three decimals) to Kabal's 0.003 Hz.
    #[test]
    fn bands_match_the_recommendation_table() {
        let b = Bands::basic();
        assert_eq!(b.len(), 109);
        for (i, lo, c, hi) in [
            (0, 80.0, 91.708, 103.445),
            (9, 298.317, 311.136, 324.055),
            (17, 517.707, 532.629, 547.721),
            (63, 3455.993, 3519.344, 3583.817),
            (75, 5331.939, 5428.712, 5527.217),
            (107, 16775.035, 17077.504, 17385.42),
            (108, 17385.42, 17690.045, 18000.0),
        ] {
            assert!((b.lower[i] - lo).abs() < 3e-3, "band {i} lower {}", b.lower[i]);
            assert!((b.centre[i] - c).abs() < 3e-3, "band {i} centre {}", b.centre[i]);
            assert!((b.upper[i] - hi).abs() < 3e-3, "band {i} upper {}", b.upper[i]);
        }
    }

    /// Kabal's worked values: γ = 0.8497 for 1019.5 Hz at 48 kHz, and
    /// the outer ear at −1.9 dB at 1 kHz with its +5.6 dB peak near 3.3 kHz.
    #[test]
    fn kabal_worked_values() {
        let w = hann(FRAME_48K);
        let bin = CALIBRATION_HZ * FRAME_48K as f64 / 48_000.0;
        assert!((gamma(&w, bin - bin.round()) - 0.8497).abs() < 1e-4);
        assert!((outer_ear_db(1_000.0) + 1.9).abs() < 0.05);
        let (peak_hz, peak) = (2_000..5_000)
            .map(|f| (f, outer_ear_db(f64::from(f))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!((peak - 5.6).abs() < 0.05 && (3_200..3_500).contains(&peak_hz), "{peak} at {peak_hz}");
        assert!((10.0 * internal_noise(1_000.0).log10() - 1.456).abs() < 1e-9);
    }

    /// The calibration: a full-scale 1019.5 Hz sine's largest bin, before
    /// the outer ear, is the playback level.
    #[test]
    fn a_full_scale_sine_peaks_at_the_playback_level() {
        let ear = Ear::new(48_000.0, 1.0, 92.0, |_| 1.0);
        let x: Vec<f64> = (0..FRAME_48K)
            .map(|i| (std::f64::consts::TAU * CALIBRATION_HZ * i as f64 / 48_000.0).sin())
            .collect();
        let s = ear.spectrum(&x, 0);
        let bin_hz = 48_000.0 / FRAME_48K as f64;
        let (k, peak) = s.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
        let undo = 10f64.powf(outer_ear_db(k as f64 * bin_hz) / 20.0);
        let db = 20.0 * (peak / undo).log10();
        assert!((db - 92.0).abs() < 0.02, "{db}");
    }

    /// Spreading a flat 0 dB pattern returns it (B_s's definition), and
    /// a louder band spreads further upward than downward.
    #[test]
    fn spreading_is_normalised_and_asymmetric() {
        let ear = Ear::new(48_000.0, 1.0, 92.0, |_| 1.0);
        let flat = vec![1.0; ear.bands.len()];
        for v in ear.spread(&flat) {
            assert!((v - 1.0).abs() < 1e-9, "{v}");
        }
        let mut one = vec![E_MIN; ear.bands.len()];
        one[50] = 1e8; // 80 dB
        let s = ear.spread(&one);
        let (below, above) = (s[50 - 8], s[50 + 8]); // ±2 Bark
        assert!(above > 10.0 * below, "{below:e} vs {above:e}");
    }

    #[test]
    fn frames_keep_their_duration_at_other_rates() {
        assert_eq!(frame_len(48_000.0), 2048);
        assert_eq!(frame_len(44_100.0), 2048);
        assert_eq!(frame_len(96_000.0), 4096);
        assert_eq!(frame_len(384_000.0), 16_384);
    }
}
