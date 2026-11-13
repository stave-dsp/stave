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
//! phase property; the round trip in `block_models.rs` holds it.

use std::f64::consts::TAU;

use stave_dsp::learned::block::{Linear, ParallelHammerstein};
use stave_dsp::Stage;

use super::periodic::{spectrum, waveform};
use super::rational::{Point, Rational};
use crate::suite::sweep::Sweep;
use crate::suite::{divide, Complex};

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
    let (f1, f2) = (sweep.low_hz, sweep.high_hz.min(0.5 * rate));
    let l = sweep.rate_constant();
    if (f1 * l - (f1 * l).round()).abs() > 1e-6 {
        // Not synchronized: the harmonics would not line up in phase.
        return None;
    }
    let a = sweep.amplitude;
    let y = spectrum(output);
    let df = rate / n as f64;
    // Deconvolve by the sweep's analytic spectrum (Novak et al. 2015, by
    // stationary phase): X̃(f) = (a/2)·√(L/f)·e^{j(2πfL(1 − ln(f/f₁)) − π/4)},
    // times `rate` for the DFT. Dividing by it, not by the sampled sweep's
    // spectrum, needs no band edges: 1/X̃ falls as √f towards DC, so the
    // even branches' DC and whatever lies below f₁ fade instead of
    // ringing through every window (a taper's ringing, cut by a window,
    // leaves an error falling only as f₁/f).
    let mut d = vec![Complex::new(0.0, 0.0); n];
    for k in 1..n / 2 {
        let f = k as f64 * df;
        let phase = TAU * f * l * (1.0 - (f / f1).ln()) - std::f64::consts::FRAC_PI_4;
        let mag = 0.5 * a * (l / f).sqrt() * rate;
        let xt = Complex::new(mag * phase.cos(), mag * phase.sin());
        d[k] = divide(y[k], xt);
        d[n - k] = d[k].conj();
    }
    let ir = waveform(&d);
    let order = options.order.max(1);
    // Each harmonic's window reaches halfway to its neighbours (harmonic
    // n arrives at −L·ln n): back towards n + 1 and on towards n − 1
    // (the linear response, with no neighbour ahead, as far on as
    // harmonic 2 is behind it). A band-limited response rings both ways
    // of its arrival; a window cut short before it leaves an error
    // falling only as 1/f, so the reach back matters as much as on.
    let gap = |a: usize, b: usize| l * (b as f64 / a as f64).ln() * rate;
    let back: Vec<usize> = (1..=order).map(|h| (0.5 * gap(h, h + 1)).floor() as usize).collect();
    let on: Vec<usize> = (1..=order)
        .map(|h| if h == 1 { gap(1, 2).floor() as usize } else { (0.5 * gap(h - 1, h)).floor() as usize })
        .collect();
    if back.iter().chain(&on).any(|&w| w < 16) {
        return None;
    }
    let widest = (0..order).map(|i| back[i] + on[i]).max()?;
    let nfft = (2 * widest).next_power_of_two();
    let mut harmonics: Vec<Vec<Complex<f64>>> = Vec::with_capacity(order);
    for h in 1..=order {
        let (pre, post) = (back[h - 1], on[h - 1]);
        let p = -(l * (h as f64).ln() * rate);
        let start = p.floor() as i64 - pre as i64;
        let offset = p - start as f64;
        let mut seg = vec![0.0; nfft];
        let (fade_in, fade_out) = (pre / 2, post / 4);
        for (i, s) in seg.iter_mut().enumerate().take(pre + post) {
            let at = (start + i as i64).rem_euclid(n as i64) as usize;
            let w = if i < fade_in {
                0.5 - 0.5 * (std::f64::consts::PI * i as f64 / fade_in as f64).cos()
            } else if i >= pre + post - fade_out {
                let j = i - (pre + post - fade_out);
                0.5 + 0.5 * (std::f64::consts::PI * j as f64 / fade_out as f64).cos()
            } else {
                1.0
            };
            *s = ir[at] * w;
        }
        let s = spectrum(&seg);
        harmonics.push(
            (0..=nfft / 2)
                .map(|k| {
                    let w = TAU * k as f64 / nfft as f64;
                    let shift = Complex::new((w * offset).cos(), (w * offset).sin());
                    s[k] * shift
                })
                .collect(),
        );
    }
    // FIR branches keep the causal part, as long as the linear window.
    let len = match options.branches {
        Branches::Fir { taps } => taps.min(on[0]),
        Branches::Rational { .. } => on[0],
    };
    let fade = len / 4;
    // Branches from the highest power down, on each bin of the kernel grid.
    let kdf = rate / nfft as f64;
    let valid = |m: usize, k: usize| {
        let f = k as f64 * kdf;
        f >= 2.0 * f1 * m as f64 && f <= 0.9 * f2
    };
    // Below its band (output frequencies under 2·m·f₁, from inputs the
    // sweep never played) a branch is unmeasured, yet the lower
    // harmonics there still hold its contribution (sin³ has a
    // fundamental): it is held at its lowest measured value, so the
    // lower branches are not corrupted by its absence.
    let mut g = vec![vec![Complex::new(0.0, 0.0); nfft / 2 + 1]; order];
    for m in (1..=order).rev() {
        for k in 0..=nfft / 2 {
            if !valid(m, k) {
                continue;
            }
            let mut rhs = harmonics[m - 1][k];
            for q in m + 1..=order {
                rhs = rhs - coefficient(m, q, a) * g[q - 1][k];
            }
            g[m - 1][k] = divide(rhs, coefficient(m, m, a));
        }
        if let Some(first) = (0..=nfft / 2).find(|&k| valid(m, k)) {
            let held = g[m - 1][first];
            for v in &mut g[m - 1][..first] {
                *v = held;
            }
        }
    }
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
