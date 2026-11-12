//! Test signals as pure functions of (channel, frame): a render is
//! reproducible by construction, and a check can regenerate exactly
//! what was fed in (the bypass check compares against it bit for bit).

use std::f64::consts::TAU;
use std::sync::Arc;

pub trait Signal: Send + Sync {
    fn at(&self, channel: usize, frame: u64) -> f32;

    /// One channel's first `frames` frames, as a buffer.
    fn render(&self, channel: usize, frames: usize) -> Vec<f32> {
        (0..frames as u64).map(|f| self.at(channel, f)).collect()
    }

    /// `frames` frames from `start` into each of `out`'s channels.
    fn fill(&self, start: u64, out: &mut [Vec<f32>], frames: usize) {
        for (c, buf) in out.iter_mut().enumerate() {
            for (i, s) in buf[..frames].iter_mut().enumerate() {
                *s = self.at(c, start + i as u64);
            }
        }
    }
}

/// Channel 0 of `S` on every channel.
pub struct Mono<S>(pub S);

impl<S: Signal> Signal for Mono<S> {
    fn at(&self, _: usize, frame: u64) -> f32 {
        self.0.at(0, frame)
    }
}

/// Linear gain for `db` dBFS.
pub fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub struct Silence;

impl Signal for Silence {
    fn at(&self, _: usize, _: u64) -> f32 {
        0.0
    }
}

/// A sine; every channel in phase.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Sine {
    pub frequency_hz: f64,
    pub amplitude: f64,
    pub sample_rate: f64,
}

impl Sine {
    /// The same tone moved to the nearest exact bin of a `window`-point
    /// analysis, so Goertzel/FFT measurement is coherent (no leakage, no
    /// window function needed).
    pub fn bin_snapped(self, window: usize) -> Self {
        let bin = (self.frequency_hz * window as f64 / self.sample_rate)
            .round()
            .max(1.0);
        Self {
            frequency_hz: bin * self.sample_rate / window as f64,
            ..self
        }
    }
}

impl Signal for Sine {
    fn at(&self, _: usize, frame: u64) -> f32 {
        let cycles = self.frequency_hz * frame as f64 / self.sample_rate;
        (self.amplitude * (TAU * cycles).sin()) as f32
    }
}

/// A unit impulse at one frame, every channel.
pub struct Impulse {
    pub frame: u64,
}

impl Signal for Impulse {
    fn at(&self, _: usize, frame: u64) -> f32 {
        if frame == self.frame {
            1.0
        } else {
            0.0
        }
    }
}

/// Silence until `frame`, then `level` on every channel.
pub struct Step {
    pub frame: u64,
    pub level: f32,
}

impl Signal for Step {
    fn at(&self, _: usize, frame: u64) -> f32 {
        if frame >= self.frame {
            self.level
        } else {
            0.0
        }
    }
}

/// Deterministic white noise, uniform in ±`amplitude`, decorrelated per
/// channel (a counter-based hash: no state, any frame on demand).
#[derive(Copy, Clone, Debug)]
pub struct Noise {
    pub seed: u64,
    pub amplitude: f32,
}

impl Signal for Noise {
    fn at(&self, channel: usize, frame: u64) -> f32 {
        let mut z =
            self.seed ^ frame.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((channel as u64) << 56);
        // splitmix64 finalizer
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let unit = (z >> 40) as f32 / (1u64 << 24) as f32; // [0, 1)
        (unit * 2.0 - 1.0) * self.amplitude
    }
}

/// An exponential (log) sine sweep from `start_hz` to `end_hz` over `seconds`,
/// with short raised-cosine fades at both ends; silent outside its span.
#[derive(Copy, Clone, Debug)]
pub struct LogSweep {
    pub start_hz: f64,
    pub end_hz: f64,
    pub seconds: f64,
    pub amplitude: f64,
    pub sample_rate: f64,
}

impl LogSweep {
    const FADE_S: f64 = 0.03;

    pub fn frames(&self) -> u64 {
        (self.seconds * self.sample_rate) as u64
    }

    /// The instantaneous frequency at `frame`.
    pub fn frequency_at(&self, frame: u64) -> f64 {
        let t = frame as f64 / self.sample_rate;
        self.start_hz * (self.end_hz / self.start_hz).powf(t / self.seconds)
    }
}

impl Signal for LogSweep {
    fn at(&self, _: usize, frame: u64) -> f32 {
        let n = self.frames();
        if frame >= n {
            return 0.0;
        }
        let t = frame as f64 / self.sample_rate;
        let l = self.seconds / (self.end_hz / self.start_hz).ln();
        let phase = TAU * self.start_hz * l * ((t / l).exp() - 1.0);
        let fade = (Self::FADE_S * self.sample_rate).max(1.0);
        let edge = (frame as f64).min((n - frame) as f64);
        let env = if edge < fade {
            0.5 - 0.5 * (std::f64::consts::PI * edge / fade).cos()
        } else {
            1.0
        };
        (self.amplitude * env * phase.sin()) as f32
    }
}

/// Recorded audio (e.g. a WAV), looped; a channel beyond the
/// recording's reads its last channel.
#[derive(Clone)]
pub struct Recording {
    pub channels: Arc<Vec<Vec<f32>>>,
}

impl Signal for Recording {
    fn at(&self, channel: usize, frame: u64) -> f32 {
        match self.channels.get(channel).or(self.channels.last()) {
            Some(ch) if !ch.is_empty() => ch[(frame % ch.len() as u64) as usize],
            _ => 0.0,
        }
    }
}

/// `inner` until `until`, silence after (a burst).
pub struct Gate<S> {
    pub inner: S,
    pub until: u64,
}

impl<S: Signal> Signal for Gate<S> {
    fn at(&self, c: usize, f: u64) -> f32 {
        if f < self.until {
            self.inner.at(c, f)
        } else {
            0.0
        }
    }
}

impl<S: Signal + ?Sized> Signal for Box<S> {
    fn at(&self, c: usize, f: u64) -> f32 {
        (**self).at(c, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_amplitude_and_period() {
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(-6.0),
            sample_rate: 48_000.0,
        };
        assert!((s.amplitude - 0.501_187).abs() < 1e-6);
        assert_eq!(s.at(0, 0), 0.0);
        assert!(
            (s.at(0, 12) - s.amplitude as f32).abs() < 1e-6,
            "quarter period at 12 frames"
        );
        assert_eq!(s.at(0, 7), s.at(1, 7), "channels in phase");
    }

    #[test]
    fn bin_snapping_lands_on_an_integer_bin() {
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(0.0),
            sample_rate: 48_000.0,
        }
        .bin_snapped(32_768);
        let bin = s.frequency_hz * 32_768.0 / 48_000.0;
        assert_eq!(bin, bin.round());
        assert!((s.frequency_hz - 1_000.0).abs() < 48_000.0 / 32_768.0);
    }

    #[test]
    fn impulse_is_one_frame() {
        let mut out = vec![vec![9.0; 8]; 2];
        Impulse { frame: 3 }.fill(0, &mut out, 8);
        assert_eq!(out[1], [0., 0., 0., 1., 0., 0., 0., 0.]);
    }

    #[test]
    fn noise_is_deterministic_bounded_and_decorrelated() {
        let n = Noise {
            seed: 7,
            amplitude: 0.4,
        };
        assert_eq!(n.at(0, 123), n.at(0, 123));
        assert_ne!(n.at(0, 123), n.at(1, 123));
        let xs: Vec<f32> = (0..20_000).map(|f| n.at(0, f)).collect();
        assert!(xs.iter().all(|x| x.abs() <= 0.4));
        let mean = xs.iter().map(|&x| f64::from(x)).sum::<f64>() / xs.len() as f64;
        assert!(mean.abs() < 0.01, "zero-mean: {mean}");
    }

    #[test]
    fn sweep_spans_its_band_and_fades() {
        let s = LogSweep {
            start_hz: 20.0,
            end_hz: 20_000.0,
            seconds: 1.0,
            amplitude: 0.5,
            sample_rate: 48_000.0,
        };
        assert!((s.frequency_at(0) - 20.0).abs() < 1e-9);
        assert!((s.frequency_at(48_000) - 20_000.0).abs() < 1e-6);
        assert_eq!(s.at(0, 0), 0.0, "faded in");
        assert_eq!(s.at(0, 48_000), 0.0, "silent past the end");
        let peak = (0..48_000).map(|f| s.at(0, f).abs()).fold(0.0, f32::max);
        assert!((peak - 0.5).abs() < 1e-3);
    }

    #[test]
    fn recording_loops_and_reuses_last_channel() {
        let r = Recording {
            channels: Arc::new(vec![vec![1.0, 2.0]]),
        };
        assert_eq!((r.at(0, 3), r.at(5, 0)), (2.0, 1.0));
        let empty = Recording {
            channels: Arc::new(vec![]),
        };
        assert_eq!(empty.at(0, 0), 0.0);
    }

    #[test]
    fn gate_cuts_at_its_frame() {
        let s = Gate {
            inner: Sine {
                frequency_hz: 100.0,
                amplitude: db_to_amp(0.0),
                sample_rate: 400.0,
            },
            until: 2,
        };
        assert_ne!(s.at(0, 1), 0.0);
        assert_eq!(s.at(0, 2), 0.0);
    }
}
