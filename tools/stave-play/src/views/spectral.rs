//! The windowed FFT every frequency view shares: plan once, reuse every
//! buffer (nothing allocates per frame), any window, amplitude scaled so
//! a full-scale sine reads 0 dBFS.

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

use stave_measure::window::Window;

use crate::analysis::AnalysisRing;

/// Floor for dB conversions in the views.
pub const FLOOR_DB: f32 = -120.0;

/// What an analyzer computes: its FFT size and window.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AnalyzerConfig {
    pub size: usize,
    pub window: Window,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            size: 2048,
            window: Window::Hann,
        }
    }
}

pub struct Analyzer {
    config: AnalyzerConfig,
    fft: Fft<f32>,
    window: Vec<f32>,
    time: Vec<f32>,
    snapshot: Vec<f32>,
    freq: Vec<Complex<f32>>,
}

impl Analyzer {
    pub fn new(config: AnalyzerConfig) -> Self {
        let fft = Fft::new(config.size);
        let mut analyzer = Self {
            config,
            fft,
            window: vec![],
            time: vec![],
            snapshot: vec![],
            freq: vec![],
        };
        analyzer.allocate();
        analyzer
    }

    pub fn config(&self) -> AnalyzerConfig {
        self.config
    }

    pub fn size(&self) -> usize {
        self.config.size
    }

    /// Adopt `config`, re-planning only what changed.
    pub fn configure(&mut self, config: AnalyzerConfig) {
        if config == self.config {
            return;
        }
        if config.size != self.config.size {
            self.fft = Fft::new(config.size);
        }
        self.config = config;
        self.allocate();
    }

    fn allocate(&mut self) {
        let n = self.config.size;
        self.window = self
            .config
            .window
            .coefficients(n)
            .into_iter()
            .map(|w| w as f32)
            .collect();
        self.time = vec![0.0; n];
        self.snapshot = vec![0.0; n];
        self.freq = vec![Complex::default(); n / 2 + 1];
    }

    /// The first n/2 bins of the windowed FFT of `x` (`x.len() == n`).
    pub fn spectrum(&mut self, x: &[f32]) -> &[Complex<f32>] {
        let mut windowed = std::mem::take(&mut self.time);
        for ((t, &s), &w) in windowed.iter_mut().zip(x).zip(&self.window) {
            *t = s * w;
        }
        self.fft.forward_real(&windowed, &mut self.freq);
        self.time = windowed;
        &self.freq[..self.size() / 2]
    }

    /// The spectrum of the newest n frames of `ring`'s channel `ch`.
    pub fn ring_spectrum(&mut self, ring: &AnalysisRing, ch: usize) -> &[Complex<f32>] {
        let mut snapshot = std::mem::take(&mut self.snapshot);
        ring.snapshot(ch, &mut snapshot);
        self.spectrum(&snapshot);
        self.snapshot = snapshot;
        &self.freq[..self.size() / 2]
    }

    /// Sine amplitude in dBFS for a bin, corrected for the window's gain.
    pub fn amp_db(&self, bin: Complex<f32>) -> f32 {
        let gain = self.config.window.coherent_gain() as f32;
        let amp = bin.norm() * 2.0 / (self.size() as f32 * gain);
        if amp <= 1e-6 {
            FLOOR_DB
        } else {
            20.0 * amp.log10()
        }
    }

    pub fn bin_hz(&self, bin: usize, rate: f32) -> f32 {
        bin as f32 * rate / self.size() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, bin: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * (bin * i) as f32 / n as f32).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_reads_zero_dbfs_at_its_bin() {
        let mut a = Analyzer::new(AnalyzerConfig {
            size: 1024,
            ..AnalyzerConfig::default()
        });
        let full = a.spectrum(&tone(1024, 64, 1.0)).to_vec();
        let half = a.spectrum(&tone(1024, 64, 0.5)).to_vec();
        assert!(a.amp_db(full[64]).abs() < 0.01, "{}", a.amp_db(full[64]));
        assert!((a.amp_db(half[64]) + 6.02).abs() < 0.01);
        assert_eq!(a.amp_db(full[300]), FLOOR_DB);
    }

    #[test]
    fn resize_replans_and_maps_bins_to_hz() {
        let mut a = Analyzer::new(AnalyzerConfig {
            size: 1024,
            ..AnalyzerConfig::default()
        });
        a.configure(AnalyzerConfig {
            size: 2048,
            ..a.config()
        });
        assert_eq!(a.size(), 2048);
        assert_eq!(a.spectrum(&vec![0.0; 2048]).len(), 1024);
        assert_eq!(a.bin_hz(1024, 48_000.0), 24_000.0);
    }
}
