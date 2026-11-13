//! Perceptual metrics (DSP.md R19): residuals and distortion judged by
//! what a listener can hear, not by waveform RMS (the measurement
//! design's principle 6).
//!
//! ```text
//! ear       ITU-R BS.1387's FFT ear model (basic version), as Kabal 2002 reads it
//! nmr       noise-to-mask ratio: total, worst band, disturbed frames; ANMR (Kahles 2019)
//! erb       ERB-band spectral error (Glasberg & Moore's ERB_N scale)
//! rnonlin   Tan & Moore's Rnonlin: gammatone-band cross-correlation, original vs distorted
//! gedlee    Geddes & Lee's Gm on a static transfer curve
//! null      the null test by audibility: the aligned residual's NMR under the reference (R16)
//! ```
//!
//! Each metric names its source and its implementation version
//! ([`VERSIONS`]): a fingerprint (DSP.md R18) records the versions it was
//! computed with, so a change to a metric shows as a stale fingerprint,
//! never as a model that moved.
//!
//! Absolute level matters to an ear model (internal noise, the
//! level-dependent spread of masking), so a metric that has one states
//! its playback assumption: a full-scale sine plays at
//! [`PLAYBACK_DB_SPL`] (92 dB SPL, BS.1387's default), and the caller
//! says what full scale is ([`Playback`]).
//!
//! ```
//! use stave_measure::perceptual::{nmr::Nmr, Playback};
//!
//! // A 1 kHz tone, and the same tone 0.1 dB louder: the difference hides
//! // under the tone. White noise 40 dB under it does not, far from 1 kHz.
//! let rate = 48_000.0;
//! let tone: Vec<f64> = (0..24_000).map(|i| 0.5 * (std::f64::consts::TAU * 1_000.0 * i as f64 / rate).sin()).collect();
//! let louder: Vec<f64> = tone.iter().map(|x| x * 10f64.powf(0.1 / 20.0)).collect();
//! let nmr = Nmr::new(Playback::default());
//! let r = nmr.between(&tone, &louder, rate);
//! assert!(r.max_db < 0.0 && r.disturbed == 0.0, "{r:?}");
//! assert_eq!(r.playback_db_spl, 92.0);
//! let mut seed = 1u32;
//! let hiss: Vec<f64> = tone.iter().map(|_| {
//!     seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
//!     5e-3 * (f64::from(seed) / f64::from(u32::MAX) - 0.5)
//! }).collect();
//! assert!(nmr.of_noise(&tone, &hiss, rate).max_db > 0.0);
//! ```

pub mod ear;
pub mod erb;
pub mod gedlee;
pub mod nmr;
pub mod null;
pub mod rnonlin;

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

/// BS.1387's playback assumption: a full-scale sine is 92 dB SPL.
pub const PLAYBACK_DB_SPL: f64 = 92.0;

/// Each metric's implementation, `(name, version)`. A version moves
/// whenever the metric's numbers would (a constant, a band edge, a
/// floor); fingerprints record them.
pub const VERSIONS: [(&str, u32); 6] = [
    ("nmr-bs1387-basic", nmr::VERSION),
    ("anmr-kahles2019", nmr::ANMR_VERSION),
    ("erb-band-error", erb::VERSION),
    ("rnonlin-tan2004", rnonlin::VERSION),
    ("gedlee-2003", gedlee::VERSION),
    ("null-nmr", null::VERSION),
];

/// [`VERSIONS`] as one line: `name/version name/version …`.
pub fn versions() -> String {
    VERSIONS.iter().map(|(n, v)| format!("{n}/{v}")).collect::<Vec<_>>().join(" ")
}

/// How loud a signal plays: a sine of peak `full_scale` (in the
/// signal's own units: 1.0 for audio, volts for a circuit) plays at
/// `db_spl`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Playback {
    pub full_scale: f64,
    pub db_spl: f64,
}

impl Default for Playback {
    /// Full scale 1.0 at 92 dB SPL.
    fn default() -> Self {
        Self {
            full_scale: 1.0,
            db_spl: PLAYBACK_DB_SPL,
        }
    }
}

impl Playback {
    /// Full scale `full_scale` at 92 dB SPL.
    pub fn full_scale(full_scale: f64) -> Self {
        Self {
            full_scale,
            ..Self::default()
        }
    }
}

/// The A-weighting (IEC 61672-1) as an amplitude gain, 1 at 1 kHz.
pub fn a_weighting(hz: f64) -> f64 {
    let f2 = hz * hz;
    let (c1, c2, c3, c4) = (20.598_997f64.powi(2), 107.652_65f64.powi(2), 737.862_23f64.powi(2), 12_194.217f64.powi(2));
    let ra = |f2: f64| c4 * f2 * f2 / ((f2 + c1) * ((f2 + c2) * (f2 + c3)).sqrt() * (f2 + c4));
    ra(f2) / ra(1e6)
}

/// The equivalent rectangular bandwidth of the auditory filter at `hz`
/// (Glasberg & Moore 1990): `24.7·(4.37·f/1000 + 1)` Hz.
pub fn erb_hz(hz: f64) -> f64 {
    24.7 * (4.37 * hz / 1000.0 + 1.0)
}

/// The ERB-number (Cams) of `hz`: `21.4·log10(4.37·f/1000 + 1)`.
pub fn erb_number(hz: f64) -> f64 {
    21.4 * (4.37 * hz / 1000.0 + 1.0).log10()
}

/// The frequency at ERB-number `e`.
pub fn erb_number_hz(e: f64) -> f64 {
    (10f64.powf(e / 21.4) - 1.0) * 1000.0 / 4.37
}

/// `x` repeated to at least `len` samples (a periodic signal measured
/// over whole periods: every frame sees steady state).
pub fn tiled(x: &[f64], len: usize) -> Vec<f64> {
    assert!(!x.is_empty(), "nothing to tile");
    x.iter().copied().cycle().take(len.max(x.len()).div_ceil(x.len()) * x.len()).collect()
}

/// The real spectrum of `x` zero-padded to `n` (a power of two).
pub(crate) fn real_spectrum(x: &[f64], n: usize) -> Vec<Complex<f64>> {
    let mut buf = x.to_vec();
    buf.resize(n, 0.0);
    let fft = Fft::new(n);
    let mut out = vec![Complex::new(0.0, 0.0); n / 2 + 1];
    fft.forward_real(&buf, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weighting_matches_iec_61672_table() {
        // IEC 61672-1 Table 3 (nominal, rounded to 0.1 dB): 31.5 Hz −39.4, 100 Hz −19.1,
        // 1 kHz 0, 4 kHz +1.0, 10 kHz −2.5.
        for (hz, db) in [(31.5, -39.4), (100.0, -19.1), (1_000.0, 0.0), (4_000.0, 1.0), (10_000.0, -2.5)] {
            let got = 20.0 * a_weighting(hz).log10();
            assert!((got - db).abs() < 0.15, "{hz} Hz: {got:.2} vs {db}");
        }
    }

    #[test]
    fn erb_scale_round_trips_and_matches_glasberg_moore() {
        assert!((erb_hz(1_000.0) - 132.639).abs() < 1e-3);
        for hz in [50.0, 440.0, 19_739.0] {
            assert!((erb_number_hz(erb_number(hz)) - hz).abs() < 1e-9 * hz);
        }
    }

    #[test]
    fn versions_name_every_metric() {
        let v = versions();
        assert_eq!(v.split(' ').count(), 6, "{v}");
    }
}
