//! Transfer function H(f) from an input/output pair (a log sweep is
//! the usual stimulus): the H1 estimator, Σ Y·X* / Σ |X|², over
//! Hann-windowed half-overlapping segments. Averaging cross-spectra
//! (not per-segment ratios) keeps it unbiased where the input is weak;
//! bins whose input energy is below the floor are reported invalid
//! rather than as noise.

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

use crate::level::db;
use crate::window::Window;

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct Bin {
    pub hz: f64,
    pub gain_db: f64,
    /// Wrapped to (−π, π].
    pub phase_rad: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TransferFunction {
    pub fft_size: usize,
    pub segments: usize,
    /// Only bins with enough input energy, ascending in frequency.
    pub bins: Vec<Bin>,
}

impl TransferFunction {
    /// Gain at the bin nearest `hz`.
    pub fn gain_at(&self, hz: f64) -> Option<f64> {
        self.nearest(hz).map(|b| b.gain_db)
    }

    pub fn nearest(&self, hz: f64) -> Option<&Bin> {
        self.bins
            .iter()
            .min_by(|a, b| (a.hz - hz).abs().total_cmp(&(b.hz - hz).abs()))
    }
}

/// Input power below this fraction of the strongest bin's is ignored.
const INPUT_FLOOR: f64 = 1e-8;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TransferConfig {
    pub sample_rate: f64,
    /// Segment length: a power of two, at least 2.
    pub fft_size: usize,
    pub window: Window,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            fft_size: 8192,
            window: Window::Hann,
        }
    }
}

/// Estimate H from `input`/`output` (same length, aligned).
pub fn estimate(input: &[f32], output: &[f32], config: &TransferConfig) -> TransferFunction {
    let TransferConfig {
        sample_rate: rate,
        fft_size,
        ..
    } = *config;
    let n = fft_size;
    let hop = n / 2;
    let len = input.len().min(output.len());
    let window = config.window.coefficients(n);
    let fft = Fft::<f64>::new(n);
    let half = fft.real_bins();
    let (mut sxy, mut sxx) = (vec![Complex::default(); half], vec![0.0f64; half]);
    let mut segments = 0;
    let (mut xt, mut yt) = (vec![0.0f64; n], vec![0.0f64; n]);
    let (mut xb, mut yb) = (
        vec![Complex::default(); half],
        vec![Complex::default(); half],
    );
    let mut start = 0;
    while start + n <= len {
        for i in 0..n {
            xt[i] = f64::from(input[start + i]) * window[i];
            yt[i] = f64::from(output[start + i]) * window[i];
        }
        fft.forward_real(&xt, &mut xb);
        fft.forward_real(&yt, &mut yb);
        for k in 0..half {
            sxy[k] = sxy[k] + yb[k] * xb[k].conj();
            sxx[k] += xb[k].norm_sqr();
        }
        segments += 1;
        start += hop;
    }
    let floor = sxx.iter().copied().fold(0.0, f64::max) * INPUT_FLOOR;
    let bins = (1..half)
        .filter(|&k| sxx[k] > floor && sxx[k] > 0.0)
        .map(|k| {
            let h = sxy[k].scale(1.0 / sxx[k]);
            Bin {
                hz: k as f64 * rate / n as f64,
                gain_db: db(h.norm()),
                phase_rad: h.arg(),
            }
        })
        .collect();
    TransferFunction {
        fft_size: n,
        segments,
        bins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{LogSweep, Signal};

    const RATE: f64 = 48_000.0;

    fn sweep() -> Vec<f32> {
        let s = LogSweep {
            start_hz: 20.0,
            end_hz: 20_000.0,
            seconds: 1.0,
            amplitude: 0.5,
            sample_rate: RATE,
        };
        (0..s.frames()).map(|f| s.at(0, f)).collect()
    }

    #[test]
    fn flat_gain_reads_flat() {
        let x = sweep();
        let y: Vec<f32> = x.iter().map(|v| v * 2.0).collect();
        let h = estimate(
            &x,
            &y,
            &TransferConfig {
                sample_rate: RATE,
                fft_size: 4096,
                ..Default::default()
            },
        );
        assert!(h.segments > 10);
        for hz in [50.0, 1_000.0, 15_000.0] {
            assert!((h.gain_at(hz).unwrap() - 6.0206).abs() < 0.01, "{hz} Hz");
        }
    }

    #[test]
    fn one_pole_lowpass_matches_its_analytic_response() {
        // y[n] = a·x[n] + (1 − a)·y[n−1]
        let a = 0.1f64;
        let x = sweep();
        let mut prev = 0.0f64;
        let y: Vec<f32> = x
            .iter()
            .map(|&v| {
                prev = a * f64::from(v) + (1.0 - a) * prev;
                prev as f32
            })
            .collect();
        let h = estimate(
            &x,
            &y,
            &TransferConfig {
                sample_rate: RATE,
                fft_size: 8192,
                ..Default::default()
            },
        );
        for hz in [100.0, 1_000.0, 5_000.0] {
            let w = std::f64::consts::TAU * hz / RATE;
            let den = Complex::<f64>::new(1.0 - (1.0 - a) * w.cos(), (1.0 - a) * w.sin());
            let want = db(a / den.norm());
            let got = h.gain_at(hz).unwrap();
            assert!(
                (got - want).abs() < 0.1,
                "{hz} Hz: got {got:.3}, want {want:.3}"
            );
        }
    }

    #[test]
    fn delay_keeps_magnitude_and_rotates_phase() {
        let x = sweep();
        let d = 10;
        let y: Vec<f32> = (0..x.len())
            .map(|i| if i >= d { x[i - d] } else { 0.0 })
            .collect();
        let h = estimate(
            &x,
            &y,
            &TransferConfig {
                sample_rate: RATE,
                fft_size: 4096,
                ..Default::default()
            },
        );
        let b = *h.nearest(1_200.0).unwrap();
        assert!(b.gain_db.abs() < 0.1, "{b:?}");
        let want = -std::f64::consts::TAU * b.hz * d as f64 / RATE;
        let wrapped =
            (want + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        assert!((b.phase_rad - wrapped).abs() < 0.05, "{b:?} vs {wrapped}");
    }

    #[test]
    fn short_or_silent_input_yields_no_bins() {
        assert!(estimate(
            &[0.0; 100],
            &[0.0; 100],
            &TransferConfig {
                sample_rate: RATE,
                fft_size: 4096,
                ..TransferConfig::default()
            }
        )
        .bins
        .is_empty());
        assert!(estimate(
            &[0.0; 8192],
            &[0.0; 8192],
            &TransferConfig {
                sample_rate: RATE,
                fft_size: 4096,
                ..TransferConfig::default()
            }
        )
        .bins
        .is_empty());
    }
}
