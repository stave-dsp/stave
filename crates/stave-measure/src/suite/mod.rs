//! The measurement design's stimulus suite and its analysers (DSP.md,
//! "Measurement design"): the same stimuli for every model, circuit and
//! plugin, so results compare across models, commits and references.
//!
//! ```text
//! period      coherent periods valid at every shipping rate; collision-free bins
//! excitation  tones on bins, gated (bursts) and faded in: one stimulus type
//! multisine   odd-odd log-spaced multisines with Schroeder phases
//! sweep       the exponential sine sweep (Farina) and its inverse
//! levels      calibrated anchors (dBu, dBV, instrument) and the knee finder
//! analysis    bins of a period, BLA, harmonics, compression, THD+N, IMD,
//!             bursts, aliasing, periodic steady state
//! ```
//!
//! Every stimulus is a function of continuous time, so one description
//! renders at any sample rate and as an ngspice source. A run is
//! measured over whole [`Period`](period::Period)s after settling: every
//! tone sits exactly on a bin, so one rectangular window gives exact
//! magnitudes and phases with no leakage.
//!
//! ```
//! use stave_measure::suite::analysis::{bins, Harmonics};
//! use stave_measure::suite::excitation::Excitation;
//! use stave_measure::suite::period::{tone_bin, Period, SHIPPING_RATES};
//!
//! // A tone near 1 kHz whose first 15 harmonics land on distinct bins at
//! // 44.1, 48 and 96 kHz, folded or not, through a soft clipper.
//! let period = Period::units(6); // 20 ms
//! let bin = tone_bin(period, 1_000.0, 15, &SHIPPING_RATES).unwrap();
//! let tone = Excitation::sine(period, bin, 0.5);
//! let rate = 48_000.0;
//! let n = period.samples(rate);
//! let out: Vec<f64> = (0..n).map(|i| (tone.at(i as f64 / rate) * 2.0).tanh()).collect();
//! let wanted: Vec<u32> = (0..=15).map(|h| h * bin).collect();
//! let spectrum = bins(&out, period, rate, &wanted);
//! let h = Harmonics::of(&spectrum, bin, 15);
//! assert!(h.dbc(3).unwrap() > -40.0 && h.dbc(2).unwrap() < -200.0); // odd only
//! ```

pub mod analysis;
pub mod excitation;
pub mod levels;
pub mod multisine;
pub mod period;
pub mod sweep;

pub use stave_dsp::primitives::spectral::Complex;

/// `a / b` for the suite's complex amplitudes.
pub fn divide(a: Complex<f64>, b: Complex<f64>) -> Complex<f64> {
    let d = b.norm_sqr();
    Complex::new(
        (a.re * b.re + a.im * b.im) / d,
        (a.im * b.re - a.re * b.im) / d,
    )
}

/// `20·log10(x)`, with silence as −∞.
pub fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// An angle wrapped into (−π, π].
pub fn wrap(radians: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let r = radians.rem_euclid(tau);
    if r > std::f64::consts::PI {
        r - tau
    } else {
        r
    }
}
