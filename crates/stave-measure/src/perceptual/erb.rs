//! ERB-band spectral error: two signals' power in bands one ERB wide
//! (Glasberg & Moore's ERB_N scale), compared band by band in dB.
//!
//! # Models
//!
//! The auditory filter bank's resolution: bands one ERB-number (Cam)
//! wide from 50 Hz up to 20 kHz (or 0.45 of the rate), rectangular on
//! the ERB-number scale `21.4·log10(4.37·f/1000 + 1)`. Each band's level
//! is its power in dB; the error is the test's level minus the
//! reference's, over the bands where either stands within
//! [`RANGE_DB`] of the reference's loudest band (both held at that
//! floor, so two inaudible bands agree). The report holds the worst
//! band (|dB| and where), the RMS over the bands and the mean (a level
//! offset reads as a mean).
//!
//! # Derivation
//!
//! The ERB_N scale of Glasberg & Moore 1990 (as Slaney's Auditory
//! Toolbox and ISO 532-2 use it); rectangular bands rather than gammatone
//! weights, so a band's power is exact for a line spectrum (a period's
//! bins).
//!
//! # Verified
//!
//! `crates/stave-measure/src/perceptual/erb.rs::tests`: a gain reads as
//! that gain in every band, a delay as nothing, a treble shelf only in
//! the bands above its corner, and a line spectrum's band powers equal
//! the signal's.
//!
//! # Cost
//!
//! `"ErbError (ERB bands, 48 kHz)"` per analysed sample: one FFT of the
//! signal (Welch, 4096 points) per side.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Any rate; signals of any length (Welch frames of 4096 at most).

use super::{erb_number, erb_number_hz, real_spectrum};

pub const VERSION: u32 = 1;
/// The lowest band's lower edge.
pub const LOW_HZ: f64 = 50.0;
/// The highest band's upper edge (or 0.45 of the rate).
pub const HIGH_HZ: f64 = 20_000.0;
/// Bands quieter than this below the reference's loudest are held at it.
pub const RANGE_DB: f64 = 60.0;

/// Bands one ERB wide.
#[derive(Clone, Debug, PartialEq)]
pub struct ErbBands {
    /// Each band's edges in Hz, and its centre.
    pub edges: Vec<(f64, f64)>,
    pub centres: Vec<f64>,
}

impl ErbBands {
    /// From 50 Hz to 20 kHz (or 0.45·`rate`), one Cam each.
    pub fn new(rate: f64) -> Self {
        let (lo, hi) = (erb_number(LOW_HZ), erb_number(HIGH_HZ.min(0.45 * rate)));
        let count = (hi - lo).floor() as usize;
        let edges: Vec<(f64, f64)> = (0..count)
            .map(|j| (erb_number_hz(lo + j as f64), erb_number_hz(lo + j as f64 + 1.0)))
            .collect();
        let centres = (0..count).map(|j| erb_number_hz(lo + j as f64 + 0.5)).collect();
        Self { edges, centres }
    }

    pub fn len(&self) -> usize {
        self.centres.len()
    }

    pub fn is_empty(&self) -> bool {
        self.centres.is_empty()
    }

    /// The power in each band of a line spectrum `(hz, power)`.
    pub fn powers(&self, lines: impl IntoIterator<Item = (f64, f64)>) -> Vec<f64> {
        let mut out = vec![0.0; self.len()];
        for (hz, p) in lines {
            if let Some(j) = self.edges.iter().position(|&(a, b)| hz >= a && hz < b) {
                out[j] += p;
            }
        }
        out
    }

    /// The power in each band of `x` at `rate`: Hann-windowed Welch
    /// frames of up to 4096 samples, half overlapped, the window's power
    /// gain divided out.
    pub fn powers_of(&self, x: &[f64], rate: f64) -> Vec<f64> {
        let n = x.len().next_power_of_two().clamp(64, 4096);
        let hop = n / 2;
        let w: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos()).collect();
        let gain: f64 = w.iter().map(|v| v * v).sum();
        let mut acc = vec![0.0; n / 2 + 1];
        let mut frames = 0;
        let mut start = 0;
        loop {
            let frame: Vec<f64> = (0..n).map(|i| x.get(start + i).copied().unwrap_or(0.0) * w[i]).collect();
            for (a, c) in acc.iter_mut().zip(real_spectrum(&frame, n)) {
                *a += c.norm_sqr();
            }
            frames += 1;
            start += hop;
            if start + n > x.len() {
                break;
            }
        }
        // Mean-square power, one-sided: a sine of amplitude a sums to
        // |X|² = N·Σw²·a²/4 over its bins, and its power is a²/2.
        let scale = 2.0 / (gain * n as f64 * frames as f64);
        let bin_hz = rate / n as f64;
        self.powers(acc.iter().enumerate().skip(1).map(|(k, p)| (k as f64 * bin_hz, p * scale)))
    }
}

/// Two band-power patterns compared.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ErbError {
    /// The largest |test − reference| over the bands, dB, and its centre.
    pub max_db: f64,
    pub at_hz: f64,
    /// The RMS of the band errors, dB.
    pub rms_db: f64,
    /// Their mean (signed), dB.
    pub mean_db: f64,
    /// The bands compared.
    pub bands: usize,
    pub version: u32,
}

impl ErbError {
    /// `test`'s band powers against `reference`'s (from [`ErbBands`]).
    pub fn of(bands: &ErbBands, reference: &[f64], test: &[f64]) -> Self {
        let loudest = reference.iter().copied().fold(0.0, f64::max);
        let floor = loudest * 10f64.powf(-RANGE_DB / 10.0);
        let db = |p: f64| 10.0 * p.max(floor).max(1e-300).log10();
        let (mut max, mut at, mut sq, mut sum, mut count) = (0.0f64, 0.0, 0.0, 0.0, 0usize);
        for j in 0..bands.len() {
            let (r, t) = (reference[j], test[j]);
            if r < floor && t < floor {
                continue;
            }
            let e = db(t) - db(r);
            if e.abs() > max {
                max = e.abs();
                at = bands.centres[j];
            }
            sq += e * e;
            sum += e;
            count += 1;
        }
        let n = count.max(1) as f64;
        Self {
            max_db: max,
            at_hz: at,
            rms_db: (sq / n).sqrt(),
            mean_db: sum / n,
            bands: count,
            version: VERSION,
        }
    }

    /// `test` against `reference`, two signals at `rate`.
    pub fn between(reference: &[f64], test: &[f64], rate: f64) -> Self {
        let bands = ErbBands::new(rate);
        Self::of(&bands, &bands.powers_of(reference, rate), &bands.powers_of(test, rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f64 = 48_000.0;

    fn noise(len: usize) -> Vec<f64> {
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s as f64 / u64::MAX as f64 - 0.5
            })
            .collect()
    }

    #[test]
    fn bands_are_one_erb_wide_from_50_hz() {
        let b = ErbBands::new(RATE);
        assert!((b.edges[0].0 - 50.0).abs() < 1e-9);
        assert!(b.len() >= 38 && b.len() <= 41, "{}", b.len());
        for (j, &(lo, hi)) in b.edges.iter().enumerate() {
            let w = super::super::erb_hz(b.centres[j]);
            assert!(((hi - lo) / w - 1.0).abs() < 0.02, "band {j}: {} vs ERB {w}", hi - lo);
        }
    }

    #[test]
    fn a_gain_reads_as_that_gain_and_a_delay_as_nothing() {
        let x = noise(48_000);
        let louder: Vec<f64> = x.iter().map(|v| v * 10f64.powf(1.0 / 20.0)).collect();
        let e = ErbError::between(&x, &louder, RATE);
        assert!((e.max_db - 1.0).abs() < 1e-6 && (e.rms_db - 1.0).abs() < 1e-6 && (e.mean_db - 1.0).abs() < 1e-6, "{e:?}");
        let mut late = vec![0.0; 48];
        late.extend(&x[..x.len() - 48]);
        let d = ErbError::between(&x, &late, RATE);
        assert!(d.max_db < 0.2, "{d:?}");
    }

    /// A treble shelf (+6 dB above ~4 kHz, a first-order difference)
    /// shows in the upper bands only.
    #[test]
    fn a_shelf_shows_above_its_corner() {
        let x = noise(48_000);
        let a = (-TAU * 4_000.0 / RATE).exp();
        let mut lp = 0.0;
        let y: Vec<f64> = x
            .iter()
            .map(|&v| {
                lp = a * lp + (1.0 - a) * v;
                v + (v - lp) // the high part doubled
            })
            .collect();
        let bands = ErbBands::new(RATE);
        let (pr, pt) = (bands.powers_of(&x, RATE), bands.powers_of(&y, RATE));
        let e = ErbError::of(&bands, &pr, &pt);
        assert!(e.at_hz > 8_000.0 && e.max_db > 4.0, "{e:?}");
        let low = bands.centres.iter().position(|&c| c > 300.0).unwrap();
        assert!((10.0 * (pt[low] / pr[low]).log10()).abs() < 0.3);
    }

    /// A line spectrum (a period's bins) and the same signal measured by
    /// Welch agree in the bands that hold the lines (Welch's window
    /// leaks a little into the neighbours).
    #[test]
    fn lines_and_signals_agree() {
        let bands = ErbBands::new(RATE);
        let picks = [8usize, 18, 30];
        let tones: Vec<(f64, f64)> = picks.iter().zip([0.5, 0.25, 0.1]).map(|(&j, a)| (bands.centres[j], a)).collect();
        let x: Vec<f64> = (0..48_000)
            .map(|i| tones.iter().map(|(f, a)| a * (TAU * f * i as f64 / RATE).sin()).sum())
            .collect();
        let lines = bands.powers(tones.iter().map(|&(f, a)| (f, a * a / 2.0)));
        let welch = bands.powers_of(&x, RATE);
        for j in picks {
            let e = 10.0 * (welch[j] / lines[j]).log10();
            assert!(e.abs() < 0.05, "band {j}: {e} dB");
        }
    }
}
