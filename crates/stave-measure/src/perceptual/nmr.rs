//! The noise-to-mask ratio (NMR): a residual's energy in each critical
//! band against the masking threshold the reference sets there, per
//! ITU-R BS.1387's basic version as Kabal 2002 implements it (his
//! section 5.5), and its A-weighted form for aliasing (Kahles, Esqueda &
//! Välimäki 2019).
//!
//! # Models
//!
//! Masking as BS.1387's FFT ear model predicts it ([`ear`](super::ear)):
//! the mask is the reference's time- and frequency-spread excitation
//! lowered by the masking offset `m(k)` — 3 dB up to 12 Bark above the
//! lowest band, then `0.25·k·Δz` dB (Kabal eq. 112). The noise is the
//! difference of the two signals' ear-weighted magnitudes,
//! `(|X_T| − |X_R|)²` (eq. 115), grouped into the same bands (no
//! internal noise, no spreading), or a given noise signal's own
//! spectrum ([`Nmr::of_noise`]). Per frame and band `NMR = E_N / M`;
//! the report holds BS.1387's **total NMR** (eq. 117: the mean over
//! frames of the mean over bands, in dB), the **worst band** (the
//! highest band NMR in any frame, and where), the per-band mean, and the
//! **fraction of disturbed frames** (a frame's worst band above 1.5 dB,
//! BS.1387's RelDistFrames). Frames before the reference's first sound
//! and after its last are skipped (Kabal 5.1: five samples summing past
//! 200 of 32768 of full scale).
//!
//! The ANMR ([`Nmr::anmr`]) A-weights both signals before the ear model
//! and takes the noise as given: Kahles et al. isolate a tone's aliases
//! by subtracting its harmonic resynthesis, and judge aliasing audible
//! above −10 dB ([`AUDIBLE_ANMR_DB`]), the threshold Lehtonen et al.'s
//! listening tests set.
//!
//! # Derivation
//!
//! From the text of BS.1387-2 and Kabal's examination of it: Kabal's
//! analytic level calibration (`γ`) in place of the Recommendation's
//! measured one, his reading of the band grouping (eq. 13) and of the
//! spreading normalisation (eq. 20). No PEAQ quality grade: the NMR is
//! one model output variable, used here as a masking-weighted residual
//! (Delgado & Herre 2020).
//!
//! # Verified
//!
//! `crates/stave-measure/src/perceptual/nmr.rs::tests` and the ear
//! model's: the band table against BS.1387's printed edges (0.003 Hz),
//! Kabal's `γ` and outer-ear values, the calibration, then the
//! documented behaviour of masking with white noise under a tone: the
//! NMR follows the noise level dB for dB, noise above a masker is masked
//! more than noise as far below it, the upward spread grows with the
//! masker's level, and a residual of zero sits at the floor. The BS.1387
//! conformance items (audio files) are not in the research library; the
//! PEAQ quality grade they check is not computed here.
//!
//! # Cost
//!
//! `"Nmr (BS.1387 basic, 48 kHz)"` per analysed sample: two FFTs of 2048
//! and a 109 × 109 spreading per hop of 1024.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Any rate from 32 kHz up (the bands stop at 18 kHz); a signal at least
//! one frame (43 ms) long; time-aligned signals (`between` compares
//! magnitudes frame by frame, so a delay of a large fraction of a frame
//! reads as noise).

use super::ear::Ear;
use super::{a_weighting, Playback};

/// The NMR implementation's version (fingerprints pin it).
pub const VERSION: u32 = 1;
/// The ANMR's version.
pub const ANMR_VERSION: u32 = 1;
/// Kahles et al.'s audibility threshold for aliasing: ANMR above −10 dB
/// is audible.
pub const AUDIBLE_ANMR_DB: f64 = -10.0;
/// A disturbed frame: its worst band's NMR above 1.5 dB (BS.1387).
pub const DISTURBED_DB: f64 = 1.5;

/// One band's NMR.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BandNmr {
    pub hz: f64,
    /// The band's NMR averaged over frames (in power), dB.
    pub mean_db: f64,
    /// Its worst frame, dB.
    pub max_db: f64,
}

/// An NMR measurement.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NmrReport {
    /// BS.1387's total NMR (eq. 117), dB.
    pub total_db: f64,
    /// The worst band NMR in any frame, dB, and its band's centre.
    pub max_db: f64,
    pub max_hz: f64,
    /// The fraction of frames whose worst band exceeds 1.5 dB.
    pub disturbed: f64,
    /// Frames measured (inside the reference's data boundary).
    pub frames: usize,
    pub bands: Vec<BandNmr>,
    /// The playback level a full-scale sine was taken to play at.
    pub playback_db_spl: f64,
    pub version: u32,
}

impl NmrReport {
    /// The total NMR is below the audibility threshold `threshold_db`.
    pub fn masked(&self, threshold_db: f64) -> bool {
        self.total_db <= threshold_db
    }
}

/// The ear weighting a measurement runs under.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Weighting {
    /// BS.1387's outer and middle ear alone.
    Ear,
    /// A-weighting as well (the ANMR).
    A,
}

/// An NMR measurement's settings.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Nmr {
    pub playback: Playback,
    pub weighting: Weighting,
}

impl Nmr {
    pub fn new(playback: Playback) -> Self {
        Self {
            playback,
            weighting: Weighting::Ear,
        }
    }

    /// The ANMR's settings: A-weighted.
    pub fn a_weighted(playback: Playback) -> Self {
        Self {
            playback,
            weighting: Weighting::A,
        }
    }

    fn ear(&self, rate: f64) -> Ear {
        let a = self.weighting == Weighting::A;
        Ear::new(rate, self.playback.full_scale, self.playback.db_spl, move |hz| {
            if a {
                a_weighting(hz)
            } else {
                1.0
            }
        })
    }

    /// BS.1387's NMR of `test` against `reference` (time-aligned, the
    /// same length, at `rate`): the noise is the difference of their
    /// magnitudes in each bin.
    pub fn between(&self, reference: &[f64], test: &[f64], rate: f64) -> NmrReport {
        assert_eq!(reference.len(), test.len(), "the reference and test differ in length");
        let ear = self.ear(rate);
        self.measure(&ear, reference, |i| {
            let (a, b) = (ear.spectrum(reference, i), ear.spectrum(test, i));
            let noise = a.iter().zip(&b).map(|(x, y)| (y - x).powi(2)).collect();
            (a, noise)
        })
    }

    /// The NMR of a given `noise` under the mask `masker` sets (the same
    /// length, at `rate`).
    pub fn of_noise(&self, masker: &[f64], noise: &[f64], rate: f64) -> NmrReport {
        assert_eq!(masker.len(), noise.len(), "the masker and noise differ in length");
        let ear = self.ear(rate);
        self.measure(&ear, masker, |i| {
            let n = ear.spectrum(noise, i).iter().map(|x| x * x).collect();
            (ear.spectrum(masker, i), n)
        })
    }

    /// Kahles et al.'s ANMR: `noise` (a tone's aliases) under the
    /// harmonic `masker`, both A-weighted.
    pub fn anmr(playback: Playback, masker: &[f64], noise: &[f64], rate: f64) -> NmrReport {
        Self::a_weighted(playback).of_noise(masker, noise, rate)
    }

    fn measure(
        &self,
        ear: &Ear,
        reference: &[f64],
        mut frame: impl FnMut(usize) -> (Vec<f64>, Vec<f64>),
    ) -> NmrReport {
        let count = ear.frames(reference.len());
        assert!(count > 0, "a signal shorter than one frame ({} samples)", ear.frame);
        let (first, last) = boundary(reference, self.playback.full_scale, ear, count);
        let nb = ear.bands.len();
        let offset: Vec<f64> = (0..nb)
            .map(|k| {
                let k = k as f64;
                let m = if k <= 12.0 / super::ear::DZ { 3.0 } else { 0.25 * k * super::ear::DZ };
                10f64.powf(-m / 10.0)
            })
            .collect();
        let mut state = vec![0.0; nb];
        let mut sum = 0.0;
        let mut band_sum = vec![0.0; nb];
        let mut band_max = vec![0.0f64; nb];
        let (mut worst, mut worst_band) = (0.0f64, 0usize);
        let mut disturbed = 0usize;
        let mut measured = 0usize;
        let threshold = 10f64.powf(DISTURBED_DB / 10.0);
        for i in 0..count {
            let (r, noise) = frame(i);
            let power: Vec<f64> = r.iter().map(|x| x * x).collect();
            let excitation = ear.smear(&mut state, &ear.spread(&ear.pitch(&power)));
            if i < first || i > last {
                continue;
            }
            let en = ear.group(&noise);
            let mut frame_sum = 0.0;
            let mut frame_max = 0.0f64;
            for k in 0..nb {
                let ratio = en[k] / (offset[k] * excitation[k]);
                frame_sum += ratio;
                band_sum[k] += ratio;
                band_max[k] = band_max[k].max(ratio);
                if ratio > frame_max {
                    frame_max = ratio;
                }
                if ratio > worst {
                    worst = ratio;
                    worst_band = k;
                }
            }
            sum += frame_sum / nb as f64;
            if frame_max > threshold {
                disturbed += 1;
            }
            measured += 1;
        }
        let db = |x: f64| 10.0 * x.log10();
        let n = measured.max(1) as f64;
        NmrReport {
            total_db: db(sum / n),
            max_db: db(worst),
            max_hz: ear.bands.centre[worst_band],
            disturbed: disturbed as f64 / n,
            frames: measured,
            bands: (0..nb)
                .map(|k| BandNmr {
                    hz: ear.bands.centre[k],
                    mean_db: db(band_sum[k] / n),
                    max_db: db(band_max[k]),
                })
                .collect(),
            playback_db_spl: self.playback.db_spl,
            version: VERSION,
        }
    }
}

/// The frames inside the reference's data boundary (Kabal 5.1): from the
/// first frame holding the first five samples whose magnitudes sum past
/// 200/32768 of full scale to the last frame holding the last such.
/// Every frame when there is no such run (silence measures as-is).
fn boundary(x: &[f64], full_scale: f64, ear: &Ear, count: usize) -> (usize, usize) {
    let threshold = 200.0 / 32_768.0 * full_scale;
    let loud = |i: usize| x[i..i + 5].iter().map(|v| v.abs()).sum::<f64>() > threshold;
    let n = x.len().saturating_sub(4);
    let Some(start) = (0..n).find(|&i| loud(i)) else {
        return (0, count - 1);
    };
    let end = (0..n).rev().find(|&i| loud(i)).unwrap_or(start) + 4;
    let hop = ear.hop();
    // The first frame containing `start`, the last containing `end`.
    let first = start.saturating_sub(ear.frame - 1).div_ceil(hop).min(count - 1);
    let last = (end / hop).min(count - 1);
    (first, last.max(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f64 = 48_000.0;
    const LEN: usize = 24_000; // half a second: 22 frames

    fn tone(hz: f64, amplitude: f64) -> Vec<f64> {
        (0..LEN).map(|i| amplitude * (TAU * hz * i as f64 / RATE).sin()).collect()
    }

    /// Deterministic white noise, RMS `rms`.
    fn noise(rms: f64, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..LEN)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s as f64 / u64::MAX as f64 - 0.5) * rms * 12f64.sqrt()
            })
            .collect()
    }

    /// Noise narrowed to one band by summing tones across it (a
    /// deterministic narrow-band probe): `count` tones from `lo` to `hi`.
    fn band_noise(lo: f64, hi: f64, rms: f64) -> Vec<f64> {
        let count = 16;
        let a = rms * (2.0 / count as f64).sqrt();
        let mut x = vec![0.0; LEN];
        for j in 0..count {
            let hz = lo + (hi - lo) * j as f64 / (count - 1) as f64;
            let phase = j as f64 * 2.4;
            for (i, v) in x.iter_mut().enumerate() {
                *v += a * (TAU * hz * i as f64 / RATE + phase).sin();
            }
        }
        x
    }

    fn nmr() -> Nmr {
        Nmr::new(Playback::default())
    }

    /// The noise enters linearly: 10 dB more noise, 10 dB more NMR, in
    /// total, in the worst band and per band.
    #[test]
    fn nmr_follows_the_noise_level_db_for_db() {
        let masker = tone(1_000.0, 0.5);
        let quiet = nmr().of_noise(&masker, &noise(1e-4, 7), RATE);
        let loud = nmr().of_noise(&masker, &noise(1e-4 * 10f64.sqrt(), 7), RATE);
        assert!((loud.total_db - quiet.total_db - 10.0).abs() < 0.01, "{} {}", quiet.total_db, loud.total_db);
        assert!((loud.max_db - quiet.max_db - 10.0).abs() < 0.01);
        assert_eq!(quiet.frames, ear_frames(LEN));
        assert_eq!(quiet.bands.len(), 109);
    }

    fn ear_frames(len: usize) -> usize {
        (len - 2048) / 1024 + 1
    }

    /// White noise under a full-scale-ish 1 kHz tone: masked near the
    /// tone (the NMR there is lowest), audible far from it, and the
    /// worst band is far from the masker.
    #[test]
    fn white_noise_is_masked_near_the_tone() {
        let masker = tone(1_000.0, 0.5);
        let r = nmr().of_noise(&masker, &noise(3e-4, 3), RATE);
        let at = |hz: f64| r.bands.iter().min_by(|a, b| (a.hz - hz).abs().total_cmp(&(b.hz - hz).abs())).unwrap().mean_db;
        assert!(at(1_000.0) < at(250.0) - 20.0, "{} vs {}", at(1_000.0), at(250.0));
        assert!(at(1_000.0) < at(8_000.0) - 20.0, "{} vs {}", at(1_000.0), at(8_000.0));
        assert!((r.max_hz - 1_000.0).abs() > 1_000.0, "{}", r.max_hz);
    }

    /// Upward spread of masking: a probe 2 Bark above a 1 kHz masker is
    /// masked more than one 2 Bark below it (27 dB/Bark below; about
    /// 24 + 230/f − 0.2·L above), and raising the masker by 20 dB lowers
    /// the upper probe's NMR by more than 20 dB (the upper slope flattens
    /// with level) but the lower one's by less.
    #[test]
    fn masking_spreads_upward_and_grows_with_level() {
        use super::super::ear::{bark, bark_hz};
        let z = bark(1_000.0);
        let (lo, hi) = (bark_hz(z - 2.0), bark_hz(z + 2.0));
        let narrow = |c: f64| band_noise(c * 0.97, c * 1.03, 1e-3);
        let probe_nmr = |amplitude: f64, c: f64| {
            let r = nmr().of_noise(&tone(1_000.0, amplitude), &narrow(c), RATE);
            r.bands.iter().min_by(|a, b| (a.hz - c).abs().total_cmp(&(b.hz - c).abs())).unwrap().mean_db
        };
        let (below, above) = (probe_nmr(0.1, lo), probe_nmr(0.1, hi));
        assert!(above < below - 3.0, "above {above:.1} dB vs below {below:.1} dB");
        let (below_loud, above_loud) = (probe_nmr(1.0, lo), probe_nmr(1.0, hi));
        assert!(above_loud < above - 22.0, "{above:.1} → {above_loud:.1}");
        // Below, the masker's own spread is unchanged in slope, and its
        // normalisation to unit area (now with a longer upper tail)
        // leaves a little less below: a 14–20 dB drop.
        let drop = below - below_loud;
        assert!((14.0..20.0).contains(&drop), "{below:.1} → {below_loud:.1}");
    }

    /// Identical signals: no noise, every band at the floor (10⁻¹² against
    /// the mask, BS.1387's band floor), no frame
    /// disturbed; a gain error of 1 dB on a tone is masked (a level
    /// difference inside the tone's own band) but 10 dB is not.
    #[test]
    fn identical_signals_sit_at_the_floor_and_gain_errors_scale() {
        let a = tone(1_000.0, 0.5);
        let same = nmr().between(&a, &a, RATE);
        assert!(same.total_db < -100.0 && same.max_db < -100.0, "{same:?}");
        assert_eq!(same.disturbed, 0.0);
        let g = |db: f64| a.iter().map(|x| x * 10f64.powf(db / 20.0)).collect::<Vec<_>>();
        let small = nmr().between(&a, &g(0.1), RATE);
        let big = nmr().between(&a, &g(10.0), RATE);
        assert!(small.disturbed == 0.0 && small.max_db < 0.0, "{}", small.max_db);
        assert!(big.disturbed == 1.0 && big.max_db > DISTURBED_DB, "{}", big.max_db);
    }

    /// Silence before and after the reference is outside the data
    /// boundary: padding changes nothing.
    #[test]
    fn silent_edges_are_skipped() {
        let a = tone(1_000.0, 0.5);
        let n = noise(1e-3, 5);
        let plain = nmr().of_noise(&a, &n, RATE);
        let mut pa = vec![0.0; 8 * 1024];
        pa.extend(&a);
        pa.extend(vec![0.0; 8 * 1024]);
        let mut pn = vec![0.0; 8 * 1024];
        pn.extend(&n);
        pn.extend(vec![0.0; 8 * 1024]);
        let padded = nmr().of_noise(&pa, &pn, RATE);
        assert!(padded.frames < ear_frames(pa.len()), "{}", padded.frames);
        assert!((padded.total_db - plain.total_db).abs() < 1.5, "{} vs {}", padded.total_db, plain.total_db);
    }

    /// A-weighting lowers the NMR of low-frequency noise far more than
    /// of noise near 2–4 kHz.
    #[test]
    fn a_weighting_discounts_low_noise() {
        let masker = tone(1_000.0, 0.5);
        let low = band_noise(60.0, 120.0, 1e-3);
        let mid = band_noise(2_500.0, 3_500.0, 1e-3);
        let drop = |n: &[f64]| {
            nmr().of_noise(&masker, n, RATE).total_db - Nmr::anmr(Playback::default(), &masker, n, RATE).total_db
        };
        assert!(drop(&low) > 15.0 && drop(&mid).abs() < 2.0, "{} {}", drop(&low), drop(&mid));
    }
}
