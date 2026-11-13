//! System identification for block-oriented models (DSP.md M3, M4): a
//! device's input and output in, a [`stave_dsp::learned::block`] model
//! out.
//!
//! ```text
//! capture             the stimulus to play (multisines, a synchronized sweep, bursts)
//!                     and the measurements read back from a recording or a system
//! periodic            random-phase multisines, steady-state periods, exact
//!                     band-limited rate changes of a period
//! rational            rational fits of a response (Sanathanan–Koerner), poles and zeros
//! wiener_hammerstein  the best split of the BLA, the curve by least squares
//! hammerstein         Novak's polynomial Hammerstein kernels from one sweep
//! solve               least squares, normal equations, polynomial roots
//! ```
//!
//! Every fit says how well it fits ([`wiener_hammerstein::Fit`],
//! [`hammerstein::Fit`]), and [`Validation`] holds a fitted model to
//! held-out audio by simulation error (Schoukens & Ljung 2019: never the
//! one-step prediction error), with the perceptual metrics of DSP.md
//! R19 beside it.
//!
//! ```
//! use stave_measure::identify::periodic::Records;
//! use stave_measure::identify::wiener_hammerstein::{fit, Options, Structure};
//!
//! // A Wiener system: a one-pole low-pass into tanh, measured at three
//! // levels with two realisations each (steady state: filtered in the
//! // frequency domain, exactly periodic).
//! let rate = 48_000.0;
//! let records = Records::measure(rate, 2048, (20.0, 20_000.0), &[0.05, 0.3, 1.0], 2, |x| {
//!     let mut y = 0.0;
//!     let mut out = Vec::new();
//!     for _ in 0..4 {
//!         out = x.iter().map(|&v| { y = 0.8 * y + 0.2 * v; (2.0 * y).tanh() }).collect();
//!     }
//!     out
//! });
//! let options = Options { structure: Structure::Wiener, max_order: 2, ..Options::default() };
//! let fit = fit(&records, &options).unwrap();
//! assert!(fit.error_db.iter().all(|&(_, e)| e < -50.0), "{:?}", fit.error_db);
//! ```

pub mod capture;
pub mod hammerstein;
pub mod periodic;
pub mod rational;
pub mod solve;
pub mod wiener_hammerstein;

use crate::perceptual::erb::ErbError;
use crate::perceptual::nmr::{Nmr, NmrReport};
use crate::perceptual::Playback;

/// A model held to a reference output on held-out audio.
#[derive(Clone, Debug)]
pub struct Validation {
    /// The residual's power re the reference's, dB (each without its
    /// mean): the simulation error.
    pub error_db: f64,
    /// ERB-band spectral error of the model's output against the
    /// reference's.
    pub erb: ErbError,
    /// The residual's noise-to-mask ratio under the reference (BS.1387,
    /// the reference's peak played at 92 dB SPL).
    pub nmr: NmrReport,
}

impl Validation {
    /// `model`'s output against `reference`'s, both the response to the
    /// same input at `rate`.
    pub fn of(reference: &[f64], model: &[f64], rate: f64) -> Self {
        let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len().max(1) as f64;
        let (mr, mm) = (mean(reference), mean(model));
        let r: Vec<f64> = reference.iter().map(|v| v - mr).collect();
        let m: Vec<f64> = model.iter().map(|v| v - mm).collect();
        let e: f64 = r.iter().zip(&m).map(|(a, b)| (a - b) * (a - b)).sum();
        let p: f64 = r.iter().map(|a| a * a).sum();
        let peak = r.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-300);
        Self {
            error_db: 10.0 * (e / p.max(1e-300)).log10(),
            erb: ErbError::between(&r, &m, rate),
            nmr: Nmr::new(Playback::full_scale(peak)).between(&r, &m, rate),
        }
    }
}
