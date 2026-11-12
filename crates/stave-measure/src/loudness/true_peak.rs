//! True peak (BS.1770-4 annex 2): the peak of the signal reconstructed
//! between samples, by oversampling with a windowed-sinc interpolator.

use std::collections::VecDeque;

use super::{LevelMeasure, LevelMeter, Outside, StreamSpec};
use crate::level::db;
use crate::window::Window;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TruePeak {
    pub oversampling: usize,
    /// Interpolator taps on each side of the reconstructed point.
    pub half_taps: usize,
}

impl Default for TruePeak {
    fn default() -> Self {
        Self {
            oversampling: 4,
            half_taps: 16,
        }
    }
}

impl TruePeak {
    /// Interpolation kernel for the point `fraction` of the way to the
    /// next sample, normalized to unity DC gain.
    fn kernel(&self, fraction: f64) -> Vec<f64> {
        let half = self.half_taps as f64;
        let taps: Vec<f64> = (0..2 * self.half_taps)
            .map(|k| {
                let t = k as f64 - half + 1.0 - fraction;
                let sinc = if t == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
                };
                sinc * Window::Blackman.at((t + half) / (2.0 * half))
            })
            .collect();
        let gain: f64 = taps.iter().sum();
        taps.into_iter().map(|h| h / gain).collect()
    }
}

impl LevelMeasure for TruePeak {
    fn name(&self) -> &'static str {
        "true-peak"
    }

    fn meter(&self, stream: &StreamSpec) -> Box<dyn LevelMeter> {
        let channels = stream.channels;
        Box::new(TruePeakMeter {
            kernels: (1..self.oversampling)
                .map(|p| self.kernel(p as f64 / self.oversampling as f64))
                .collect(),
            half_taps: self.half_taps,
            history: vec![VecDeque::from(vec![0.0; 2 * self.half_taps]); channels],
            outside: stream.outside,
            frames: 0,
            peak: None,
        })
    }
}

/// Each channel's last `2 · half_taps` samples; after sample `n` arrives,
/// the points between `n − half_taps` and the next sample are known.
#[derive(Clone)]
struct TruePeakMeter {
    kernels: Vec<Vec<f64>>,
    half_taps: usize,
    history: Vec<VecDeque<f64>>,
    outside: Outside,
    frames: u64,
    peak: Option<f64>,
}

impl TruePeakMeter {
    fn advance(&mut self, channel: usize, frame: u64, x: f64) {
        let history = &mut self.history[channel];
        history.pop_front();
        history.push_back(x);
        let mut loudest = self.peak.unwrap_or(0.0).max(x.abs());
        let first_point = match self.outside {
            Outside::Silence => self.half_taps,
            Outside::Unknown => 2 * self.half_taps - 1,
        };
        if frame >= first_point as u64 {
            for kernel in &self.kernels {
                let y: f64 = kernel.iter().zip(history.iter()).map(|(h, v)| h * v).sum();
                loudest = loudest.max(y.abs());
            }
        }
        self.peak = Some(loudest);
    }

    /// The points still waiting on future samples, read against silence.
    fn flushed(&self) -> Self {
        let mut done = self.clone();
        for channel in 0..done.history.len() {
            for i in 0..self.half_taps as u64 {
                let peak_before = done.peak;
                done.advance(channel, self.frames + i, 0.0);
                if self.frames + i < self.half_taps as u64 {
                    done.peak = peak_before;
                }
            }
        }
        done
    }
}

impl LevelMeter for TruePeakMeter {
    fn push(&mut self, block: &[&[f32]]) {
        let frames = block.iter().map(|c| c.len()).min().unwrap_or(0);
        for (channel, samples) in block.iter().enumerate().take(self.history.len()) {
            for (i, &x) in samples[..frames].iter().enumerate() {
                let x = if x.is_finite() { f64::from(x) } else { 0.0 };
                self.advance(channel, self.frames + i as u64, x);
            }
        }
        self.frames += frames as u64;
    }

    fn level(&self) -> Option<f64> {
        if self.frames == 0 {
            return None;
        }
        match self.outside {
            Outside::Silence => self.flushed().peak.map(db),
            Outside::Unknown => self.peak.map(db),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_peak_between_samples() {
        // A quarter-rate sine at 45°: every sample sits at ±0.707 of a
        // full-scale waveform. Faded in and out, so its edges add no
        // overshoot of their own.
        let n = 9_600;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let edge = (i.min(n - 1 - i) as f64 / 2_400.0).min(1.0);
                let phase = std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4;
                (edge * phase.sin()) as f32
            })
            .collect();
        let sample_peak = crate::loudness::Peak.measure(&[&x], 48_000.0).unwrap();
        let true_peak = TruePeak::default().measure(&[&x], 48_000.0).unwrap();
        assert!((sample_peak + 3.01).abs() < 0.01, "{sample_peak}");
        assert!(true_peak.abs() < 0.05, "{true_peak}");
    }

    #[test]
    fn a_window_cut_from_a_stream_has_no_edges_of_its_own() {
        // Cut at the crests of a steady 1 kHz sine: with silence outside
        // the cuts are steps that overshoot; with the stream unknown
        // outside they are not there.
        let x: Vec<f32> = (0..4_800)
            .map(|i| (0.5 * (std::f64::consts::TAU * 1_000.0 * i as f64 / 48_000.0).cos()) as f32)
            .collect();
        let read = |outside| {
            let mut meter = TruePeak::default().meter(&StreamSpec {
                channels: 1,
                sample_rate: 48_000.0,
                outside,
            });
            meter.push(&[&x]);
            meter.level().unwrap()
        };
        let steady = db(0.5);
        assert!((read(Outside::Unknown) - steady).abs() < 0.01);
        assert!(read(Outside::Silence) > steady + 0.1);
    }

    #[test]
    fn never_below_the_sample_peak() {
        let x = [0.0f32, 0.9, -0.3, 0.0];
        let tp = TruePeak::default().measure(&[&x], 48_000.0).unwrap();
        assert!(tp >= db(f64::from(0.9f32)));
    }
}
