//! Fitting a parallel (generalised polynomial) Hammerstein model from one
//! synchronized exponential sweep: Novak, Simon, Kadlec & Lotton,
//! "Nonlinear system identification using exponential swept-sine
//! signal", IEEE TIM 59(8), 2010, with the synchronized sweep of Novak,
//! Lotton & Simon, JAES 63(10), 2015 (and the audio-effects case, Novak,
//! Simon & Lotton, DAFx-09).
//!
//! The sweep `x = a·sin φ(t)`, `φ = 2π f₁ L e^{t/L}` with `f₁L` whole,
//! has `n·φ(t) = φ(t + L ln n)`: the n-th harmonic a branch makes is the
//! sweep itself, `L ln n` early. Deconvolving the output by the sweep
//! (dividing spectra over the sweep's band) therefore leaves one impulse
//! response per harmonic, the n-th at `−L ln n`. Expanding `sinᵐ` gives
//! the harmonic responses in terms of the branch filters (positive
//! frequencies; `m ≥ n`, `m ≡ n mod 2`):
//!
//! ```text
//! Hₙ(f) = Σₘ Gₘ(f) · a^{m−1} · (2j)^{1−m} · C(m, (m+n)/2) · (−1)^{(m−n)/2}
//! ```
//!
//! — upper triangular, solved from the highest order down. (Checked
//! against `sin³ = (3 sin − sin 3·)/4`: `A₁₃ = 3a²/4`, `A₃₃ = −a²/4`.)
//! The derivation is from the expansion and the synchronized sweep's
//! phase property; the round trip in `block_models.rs` holds it. The
//! separation and the conversion are [`kernels`](super::kernels)'s,
//! shared with the kernels as an analysis (DSP.md M6).

use std::f64::consts::TAU;

use stave_dsp::learned::block::{Linear, ParallelHammerstein};
use stave_dsp::Stage;

use super::kernels::HarmonicResponses;
use super::periodic::waveform;
use super::rational::{Point, Rational};
use crate::suite::sweep::Sweep;
use crate::suite::Complex;

/// How each branch filter is stored.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Branches {
    /// A rational fit of each kernel up to `max_order` (as many zeros as
    /// poles), the lowest within `tolerance_db` of the best.
    Rational { max_order: usize, tolerance_db: f64 },
    /// The kernel's causal taps, at most `taps` (and at most the linear
    /// response's window).
    Fir { taps: usize },
}

/// The fit's settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// The highest power (Novak's M).
    pub order: usize,
    pub branches: Branches,
    /// A branch whose peak contribution at the sweep's level is this far
    /// below the linear branch's is dropped, dB.
    pub floor_db: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            order: 5,
            branches: Branches::Rational {
                max_order: 4,
                tolerance_db: 0.1,
            },
            floor_db: -100.0,
        }
    }
}

/// The sweep a fit reads: `sweep` sampled at `rate`, faded out over its
/// last `fade` seconds (an abrupt end rings across the band), with
/// silence after it to a power-of-two length at least `tail` seconds
/// longer (the response's decay).
pub fn stimulus(sweep: &Sweep, rate: f64, fade: f64, tail: f64) -> Vec<f64> {
    let mut x = sweep.samples(rate);
    let n = x.len();
    let f = ((fade * rate) as usize).min(n);
    for i in 0..f {
        let g = 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / f as f64).cos();
        x[n - 1 - i] *= g;
    }
    let total = (n + (tail * rate).ceil() as usize).next_power_of_two();
    x.resize(total, 0.0);
    x
}

/// `n choose k`.
fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// `A[n][m]` of the module docs at amplitude `a`, 1-based.
pub fn coefficient(n: usize, m: usize, a: f64) -> Complex<f64> {
    if m < n || (m - n) % 2 == 1 {
        return Complex::new(0.0, 0.0);
    }
    // (2j)^{1−m} = 2^{1−m} · j^{1−m}; j^k cycles 1, j, −1, −j.
    let k = (4 - (m - 1) % 4) % 4;
    let jk = [Complex::new(1.0, 0.0), Complex::new(0.0, 1.0), Complex::new(-1.0, 0.0), Complex::new(0.0, -1.0)][k];
    let sign = if ((m - n) / 2) % 2 == 0 { 1.0 } else { -1.0 };
    let mag = a.powi(m as i32 - 1) * 2f64.powi(1 - m as i32) * choose(m, (m + n) / 2) * sign;
    jk.scale(mag)
}

/// A fitted parallel Hammerstein model, and its kernels.
#[derive(Clone, Debug)]
pub struct Fit {
    pub model: ParallelHammerstein,
    /// Each branch's measured response `Gₘ` on `(Hz, value)` points over
    /// its valid band (`m·2f₁` to `0.9·f₂`).
    pub kernels: Vec<Vec<(f64, Complex<f64>)>>,
    /// Branches dropped below the floor (1-based powers).
    pub dropped: Vec<usize>,
    /// Kernel length, samples (what separated between harmonics).
    pub kernel_len: usize,
    /// Each rational branch's worst fit error against its kernel, dB
    /// (absolute, re the kernel's peak).
    pub branch_error_db: Vec<f64>,
}

/// Fit from `output`, the response at `rate` to [`stimulus`]`(sweep, …)`
/// = `input`.
pub fn fit(sweep: &Sweep, rate: f64, input: &[f64], output: &[f64], options: &Options) -> Option<Fit> {
    let n = input.len();
    assert!(n.is_power_of_two() && output.len() == n, "pad the sweep to a power of two (stimulus())");
    let a = sweep.amplitude;
    let order = options.order.max(1);
    let separated = HarmonicResponses::separate_with(sweep, rate, output, order, false)?;
    let nfft = 2 * (separated.responses[0].len() - 1);
    // FIR branches keep the causal part, as long as the linear window.
    let len = match options.branches {
        Branches::Fir { taps } => taps.min(separated.linear_window),
        Branches::Rational { .. } => separated.linear_window,
    };
    let fade = len / 4;
    let kdf = separated.bin_hz;
    let valid = |m: usize, k: usize| separated.valid(m, k);
    let g = separated.hammerstein();
    let kernels: Vec<Vec<(f64, Complex<f64>)>> = (1..=order)
        .map(|m| {
            (0..=nfft / 2)
                .filter(|&k| k > 0 && valid(m, k))
                .map(|k| (k as f64 * kdf, g[m - 1][k]))
                .collect()
        })
        .collect();
    // Each branch's peak contribution at the sweep's level.
    let reach: Vec<f64> = kernels
        .iter()
        .enumerate()
        .map(|(i, kern)| kern.iter().fold(0.0f64, |p, (_, v)| p.max(v.norm())) * a.powi(i as i32 + 1))
        .collect();
    let floor = reach[0] * 10f64.powf(options.floor_db / 20.0);
    let mut dropped = Vec::new();
    let mut branch_error_db = Vec::new();
    let mut branches = Vec::with_capacity(order);
    for (i, kern) in kernels.iter().enumerate() {
        if reach[i] < floor || kern.len() < 8 {
            dropped.push(i + 1);
            branches.push(Linear::gain(0.0));
            branch_error_db.push(f64::NEG_INFINITY);
            continue;
        }
        match options.branches {
            Branches::Rational { max_order, tolerance_db } => {
                let peak = kern.iter().fold(0.0f64, |p, (_, v)| p.max(v.norm()));
                // Absolute error re the peak: weight by |h|²/peak², per
                // octave by 1/f.
                let points: Vec<Point> = kern
                    .iter()
                    .map(|&(f, h)| Point {
                        w: TAU * f / rate,
                        h,
                        weight: h.norm_sqr() / (peak * peak) / f,
                    })
                    .collect();
                let (r, _) = Rational::fit_order(&points, max_order, tolerance_db)?;
                let e = kern
                    .iter()
                    .map(|&(f, h)| (r.response(TAU * f / rate) - h).norm() / peak)
                    .fold(0.0, f64::max);
                branch_error_db.push(20.0 * (1.0 + e).log10());
                branches.push(Linear::Rational(r.sections()));
            }
            Branches::Fir { .. } => {
                let mut full = vec![Complex::new(0.0, 0.0); nfft];
                for k in 0..=nfft / 2 {
                    full[k] = g[i][k];
                    if k > 0 && k < nfft / 2 {
                        full[nfft - k] = g[i][k].conj();
                    }
                }
                let mut taps = waveform(&full);
                taps.truncate(len);
                for (j, t) in taps.iter_mut().enumerate().skip(len - fade) {
                    let q = j - (len - fade);
                    *t *= 0.5 + 0.5 * (std::f64::consts::PI * q as f64 / fade as f64).cos();
                }
                branch_error_db.push(0.0);
                branches.push(Linear::Fir(taps));
            }
        }
    }
    let mut model = ParallelHammerstein::new(rate as f32, branches).with_limit(a);
    // The offset: what the output holds on average over the sweep that
    // the branches do not.
    model.prepare(rate as f32);
    let swept = sweep.samples(rate).len();
    let mean: f64 = input[..swept]
        .iter()
        .zip(&output[..swept])
        .map(|(&u, &v)| v - model.tick_f64(u))
        .sum::<f64>()
        / swept as f64;
    model.reset();
    let model = model.with_offset(mean);
    Some(Fit {
        model,
        kernels,
        dropped,
        kernel_len: len,
        branch_error_db,
    })
}
