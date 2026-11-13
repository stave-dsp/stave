//! **Fir** — finite impulse response filter design: windowed sinc
//! (lowpass, highpass, bandpass), weighted least squares, and the
//! minimum-phase version of any design.
//!
//! A design is a set of taps in `f64`; run it through a
//! [`Convolver`](super::Convolver) via its
//! [`impulse_response`](Fir::impulse_response). Frequencies are fractions
//! of the sample rate (cycles per sample, `0` to `0.5`), since a design
//! is made for one rate: `hz / sample_rate`.

use super::cepstrum;
use super::fft::Complex;
use super::impulse_response::ImpulseResponse;
use super::window::Window;
use core::f64::consts::{PI, TAU};

/// What a windowed-sinc design passes. Edges are fractions of the
/// sample rate, in `(0, 0.5)`, at the −6 dB point of the windowed
/// response.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Passband {
    /// Below `cutoff`; unity gain at DC.
    Lowpass {
        /// The edge, a fraction of the sample rate.
        cutoff: f64,
    },
    /// Above `cutoff`; unity gain at Nyquist. Needs an odd length.
    Highpass {
        /// The edge, a fraction of the sample rate.
        cutoff: f64,
    },
    /// Between `low` and `high`; unity gain at their midpoint.
    Bandpass {
        /// The lower edge, a fraction of the sample rate.
        low: f64,
        /// The upper edge, a fraction of the sample rate.
        high: f64,
    },
}

/// One band of a [`Fir::least_squares`] specification: the gain wanted
/// from `start` to `end` (fractions of the sample rate) and how much its
/// squared error counts. Frequencies outside every band are free
/// (transition bands).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Band {
    /// The lower edge, a fraction of the sample rate.
    pub start: f64,
    /// The upper edge, a fraction of the sample rate.
    pub end: f64,
    /// The linear gain wanted across the band (1 passes, 0 stops).
    pub gain: f64,
    /// The weight of this band's squared error.
    pub weight: f64,
}

/// A designed FIR filter: its taps, in `f64`.
///
/// # Models
///
/// `y[t] = Σ h[j]·x[t − j]` with taps designed to a specification:
/// linear phase (symmetric taps, delay `(len − 1)/2` samples) from
/// [`windowed_sinc`](Self::windowed_sinc) and
/// [`least_squares`](Self::least_squares), minimum phase (the same
/// magnitude, the least delay) from [`minimum_phase`](Self::minimum_phase).
///
/// # Derivation
///
/// Windowed sinc: the ideal response truncated by a [`Window`]
/// (Oppenheim & Schafer, *Discrete-Time Signal Processing*, §7.5), with
/// Kaiser's formulas for β and the length ([`kaiser`](Self::kaiser)).
/// Least squares: the type-I design minimising the weighted integral
/// squared error over the bands, by its normal equations in closed form
/// (Parks & Burrus, *Digital Filter Design*, 1987, §3.2; MATLAB's
/// `firls`). Minimum phase: the real cepstrum (§13.8.1).
///
/// # Verified
///
/// `crates/stave-dsp/tests/fir_design.rs`: Kaiser lowpass, highpass and
/// bandpass designs meet their stopband attenuation within 1 dB of the
/// Kaiser estimate (worst 79.0 dB asked 80) and the matching passband
/// ripple, on a dense grid, at 40 to 100 dB; least squares beats the
/// Kaiser design's weighted error (1.45e-7 against 2.21e-7 at 61 taps)
/// and follows its weights; the minimum-phase version of an 80 dB
/// lowpass keeps the passband within 1.3e-6 dB (budget 0.01) and the
/// stopband at 80.0 dB (budget 77), and its partial energy never trails
/// the linear-phase design's (peak at tap 10 against 63). The
/// oversampler's half-band tables come out bit for bit
/// (`the_oversampler_half_bands_are_kaiser_designs`).
///
/// # Cost
///
/// Design runs once, off the audio thread (it allocates). Running a
/// design costs a [`Convolver`](super::Convolver) of its length:
/// `tests/datasheet.rs`, "Fir 127 taps B=64 direct", about 28 ns/sample
/// at zero latency on an Apple M-series core.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// Edges in `(0, 0.5)`; any length from 1 (odd for a highpass or least
/// squares). Least squares solves a `(len + 1)/2` square system in
/// `f64`: fine to a few thousand taps.
///
/// ```
/// use stave_dsp::primitives::spectral::{Fir, Passband};
///
/// // A lowpass at 1/8 of the rate, 80 dB down 0.03 above it.
/// let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.125 }, 80.0, 0.03);
/// assert!(fir.gain_db(0.0).abs() < 0.001);
/// assert!(fir.gain_db(0.125 + 0.03) < -79.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Fir {
    taps: Vec<f64>,
}

/// `sin(2π·f·k) / (π·k)` for `k ≠ 0`: the ideal lowpass at `f`, at offset
/// `k` from the centre; exactly zero where `2·f·k` is an integer (the
/// sinc's zeros, which `sin` would miss by an ulp).
fn sinc(f: f64, k: f64) -> f64 {
    let turns = 2.0 * f * k;
    if turns == turns.round() {
        0.0
    } else {
        (TAU * f * k).sin() / (PI * k)
    }
}

impl Fir {
    /// Taps as given.
    pub fn from_taps(taps: Vec<f64>) -> Self {
        Self { taps }
    }

    /// The ideal `passband` truncated to `len` taps by `window`, scaled
    /// to unity gain where the passband says. Linear phase: symmetric
    /// taps, `(len − 1)/2` samples of delay.
    ///
    /// # Panics
    ///
    /// If `len` is 0, an edge is outside `(0, 0.5)`, `low >= high`, or a
    /// highpass has an even length (its response is zero at Nyquist).
    pub fn windowed_sinc(passband: Passband, len: usize, window: Window) -> Self {
        assert!(len > 0, "Fir::windowed_sinc: no taps");
        let edge = |f: f64| assert!(f > 0.0 && f < 0.5, "Fir edge {f} outside (0, 0.5)");
        // The ideal response at offset k, and the frequency to normalise
        // at (the passband's reference point).
        let (centre, ideal, reference): (f64, Box<dyn Fn(f64) -> f64>, f64) = match passband {
            Passband::Lowpass { cutoff } => {
                edge(cutoff);
                (2.0 * cutoff, Box::new(move |k| sinc(cutoff, k)), 0.0)
            }
            Passband::Highpass { cutoff } => {
                edge(cutoff);
                assert!(len % 2 == 1, "Fir highpass needs an odd length, got {len}");
                (1.0 - 2.0 * cutoff, Box::new(move |k| -sinc(cutoff, k)), 0.5)
            }
            Passband::Bandpass { low, high } => {
                edge(low);
                edge(high);
                assert!(low < high, "Fir bandpass: low {low} ≥ high {high}");
                (
                    2.0 * (high - low),
                    Box::new(move |k| sinc(high, k) - sinc(low, k)),
                    0.5 * (low + high),
                )
            }
        };
        let m = (len - 1) as f64 / 2.0;
        let odd = len % 2 == 1;
        // side[j]: the taps at offsets ±k for k ascending (1, 2, … for an
        // odd length; ½, 3⁄2, … for an even one), windowed; the centre
        // weighs exactly 1.
        let side: Vec<f64> = (0..len / 2)
            .map(|j| {
                let k = if odd { (j + 1) as f64 } else { j as f64 + 0.5 };
                ideal(k) * window.at(k, m)
            })
            .collect();
        let centre = if odd { centre } else { 0.0 };
        let gain = if reference == 0.0 {
            centre + 2.0 * side.iter().sum::<f64>()
        } else {
            centre
                + 2.0
                    * side
                        .iter()
                        .enumerate()
                        .map(|(j, &s)| {
                            let k = if odd { (j + 1) as f64 } else { j as f64 + 0.5 };
                            s * (TAU * reference * k).cos()
                        })
                        .sum::<f64>()
        };
        let g = 1.0 / gain;
        let mut taps = vec![0.0; len];
        for (j, &s) in side.iter().enumerate() {
            let v = s * g;
            taps[len / 2 - 1 - j] = v;
            taps[len / 2 + j + usize::from(odd)] = v;
        }
        if odd {
            taps[len / 2] = centre * g;
        }
        Self { taps }
    }

    /// A Kaiser windowed-sinc design for `attenuation_db` of stopband
    /// beyond a `transition` (a fraction of the sample rate) from each
    /// edge: β by [`Window::kaiser_for`], the length by Kaiser's
    /// estimate `(A − 7.95)/(14.36·Δf) + 1`, rounded up to odd. The
    /// passband ripple is about the same fraction as the stopband:
    /// `±10^(−A/20)`. A bandpass is the difference of two lowpasses whose
    /// errors add, so it is designed 6 dB deeper.
    pub fn kaiser(passband: Passband, attenuation_db: f64, transition: f64) -> Self {
        assert!(transition > 0.0, "Fir::kaiser: the transition must be positive");
        let attenuation_db = match passband {
            Passband::Bandpass { .. } => attenuation_db + 20.0 * 2f64.log10(),
            _ => attenuation_db,
        };
        let len = ((attenuation_db - 7.95) / (14.36 * transition)).ceil().max(0.0) as usize + 1;
        let len = len | 1;
        // The windowed edge sits mid-transition.
        let shift = transition / 2.0;
        let passband = match passband {
            Passband::Lowpass { cutoff } => Passband::Lowpass {
                cutoff: cutoff + shift,
            },
            Passband::Highpass { cutoff } => Passband::Highpass {
                cutoff: cutoff - shift,
            },
            Passband::Bandpass { low, high } => Passband::Bandpass {
                low: low - shift,
                high: high + shift,
            },
        };
        Self::windowed_sinc(passband, len, Window::kaiser_for(attenuation_db))
    }

    /// The linear-phase (type I) design of odd length `len` minimising
    /// `Σ weight·∫(|H(f)| − gain)² df` over `bands`.
    ///
    /// # Panics
    ///
    /// If `len` is even or 0, a band is empty or outside `[0, 0.5]`, or
    /// the bands leave the system singular (no band at all).
    pub fn least_squares(len: usize, bands: &[Band]) -> Self {
        assert!(len % 2 == 1, "Fir::least_squares needs an odd length, got {len}");
        assert!(!bands.is_empty(), "Fir::least_squares: no bands");
        for b in bands {
            assert!(
                0.0 <= b.start && b.start < b.end && b.end <= 0.5,
                "Fir band [{}, {}] outside [0, 0.5]",
                b.start,
                b.end
            );
        }
        let m = (len - 1) / 2;
        // ∫ cos(2π·i·f) df over [a, b].
        let int = |i: usize, a: f64, b: f64| {
            if i == 0 {
                b - a
            } else {
                let w = TAU * i as f64;
                ((w * b).sin() - (w * a).sin()) / w
            }
        };
        let n = m + 1;
        let mut q = vec![0.0f64; n * n];
        let mut rhs = vec![0.0f64; n];
        for b in bands {
            for k in 0..n {
                for l in 0..n {
                    q[k * n + l] += b.weight
                        * 0.5
                        * (int(k.abs_diff(l), b.start, b.end) + int(k + l, b.start, b.end));
                }
                rhs[k] += b.weight * b.gain * int(k, b.start, b.end);
            }
        }
        let a = solve(&mut q, &mut rhs, n);
        let mut taps = vec![0.0; len];
        taps[m] = a[0];
        for k in 1..n {
            taps[m - k] = a[k] / 2.0;
            taps[m + k] = a[k] / 2.0;
        }
        Self { taps }
    }

    /// The minimum-phase filter with this magnitude response (the real
    /// cepstrum), the same length: the least delay any filter with this
    /// magnitude can have, for the loss of linear phase. The polarity
    /// at low frequencies is kept.
    pub fn minimum_phase(&self) -> Self {
        Self {
            taps: cepstrum::minimum_phase(&self.taps),
        }
    }

    /// The taps.
    pub fn taps(&self) -> &[f64] {
        &self.taps
    }

    /// The number of taps.
    pub fn len(&self) -> usize {
        self.taps.len()
    }

    /// `true` for a design with no taps.
    pub fn is_empty(&self) -> bool {
        self.taps.is_empty()
    }

    /// The frequency response at `frequency` (a fraction of the sample
    /// rate): `Σ h[j]·e^(−2πi·f·j)`, summed in `f64`.
    pub fn response(&self, frequency: f64) -> Complex<f64> {
        let w = -TAU * frequency;
        self.taps
            .iter()
            .enumerate()
            .fold(Complex::new(0.0, 0.0), |acc, (j, &h)| {
                let a = w * j as f64;
                acc + Complex::new(h * a.cos(), h * a.sin())
            })
    }

    /// The gain at `frequency` (a fraction of the sample rate), in dB.
    pub fn gain_db(&self, frequency: f64) -> f64 {
        20.0 * self.response(frequency).norm().max(1e-300).log10()
    }

    /// The taps as a mono [`ImpulseResponse`] for a
    /// [`Convolver`](super::Convolver).
    pub fn impulse_response(&self) -> ImpulseResponse {
        let taps: Vec<f32> = self.taps.iter().map(|&h| h as f32).collect();
        ImpulseResponse::mono(&taps)
    }
}

impl From<&Fir> for ImpulseResponse {
    fn from(fir: &Fir) -> Self {
        fir.impulse_response()
    }
}

/// Solves the `n × n` system `q·x = rhs` (row-major) by Gaussian
/// elimination with partial pivoting, in place.
fn solve(q: &mut [f64], rhs: &mut [f64], n: usize) -> Vec<f64> {
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&a, &b| q[a * n + col].abs().total_cmp(&q[b * n + col].abs()))
            .unwrap_or(col);
        assert!(
            q[pivot * n + col].abs() > 1e-300,
            "Fir::least_squares: the bands leave the system singular"
        );
        if pivot != col {
            for c in 0..n {
                q.swap(col * n + c, pivot * n + c);
            }
            rhs.swap(col, pivot);
        }
        let d = q[col * n + col];
        for row in col + 1..n {
            let f = q[row * n + col] / d;
            if f != 0.0 {
                for c in col..n {
                    q[row * n + c] -= f * q[col * n + c];
                }
                rhs[row] -= f * rhs[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|c| q[row * n + c] * x[c]).sum();
        x[row] = (rhs[row] - s) / q[row * n + row];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::resample::{halfband_coeffs_n, HB_BETA, HB_TAPS, HB_TAPS_INNER};

    /// The oversampler's half-band tables (kernel, which cannot depend on
    /// this layer) are this module's Kaiser lowpass at a quarter of the
    /// rate, bit for bit: the centre, the odd-offset taps, and exact
    /// zeros at the even offsets.
    #[test]
    fn the_oversampler_half_bands_are_kaiser_designs() {
        for taps in [HB_TAPS, HB_TAPS_INNER] {
            let fir = Fir::windowed_sinc(
                Passband::Lowpass { cutoff: 0.25 },
                taps,
                Window::Kaiser { beta: HB_BETA },
            );
            let (centre, side) = halfband_coeffs_n(taps);
            let m = (taps - 1) / 2;
            let h = fir.taps();
            assert_eq!(h[m].to_bits(), centre.to_bits(), "{taps}: centre");
            for k in 1..=m {
                let want = if k % 2 == 1 { side[(k - 1) / 2] } else { 0.0 };
                assert_eq!(h[m + k].to_bits(), want.to_bits(), "{taps}: offset {k}");
                assert_eq!(h[m - k].to_bits(), want.to_bits(), "{taps}: offset −{k}");
            }
        }
    }

    #[test]
    fn even_lengths_are_symmetric_with_no_centre() {
        let fir = Fir::windowed_sinc(Passband::Lowpass { cutoff: 0.2 }, 8, Window::Hann);
        let h = fir.taps();
        for n in 0..4 {
            assert_eq!(h[n], h[7 - n]);
        }
        assert!((fir.gain_db(0.0)).abs() < 1e-12);
    }
}
