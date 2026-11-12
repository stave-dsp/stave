//! The sample pads: one-shots that play through an effect's inputs (or
//! over an instrument's output). Built-in test signals for reverb,
//! delay and impulse-response work, plus every `.wav` in the folder
//! named by `STAVE_PLAY_SAMPLES`.

use std::path::Path;
use std::sync::Arc;

use stave_measure::signal::{LogSweep, Noise, Signal};
use stave_measure::wav;
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

/// The environment variable naming a folder of `.wav` one-shots.
pub const SAMPLES_DIR_ENV: &str = "STAVE_PLAY_SAMPLES";

#[derive(Clone)]
pub struct Sample {
    pub name: String,
    /// Interleaved by channel.
    pub data: Arc<Vec<f32>>,
    pub channels: u16,
    pub sample_rate: u32,
}

/// What the audio thread receives when a pad is hit.
#[derive(Clone, Debug)]
pub struct SampleTrigger {
    pub data: Arc<Vec<f32>>,
    pub channels: u16,
}

impl Sample {
    pub fn trigger(&self) -> SampleTrigger {
        SampleTrigger {
            data: self.data.clone(),
            channels: self.channels,
        }
    }

    pub fn duration_ms(&self) -> u32 {
        let frames_per_second = u64::from(self.sample_rate) * u64::from(self.channels);
        if frames_per_second == 0 {
            return 0;
        }
        (self.data.len() as u64 * 1000 / frames_per_second) as u32
    }

    /// A mono sample of `seconds`, sample by sample from `at(frame)`.
    fn mono(name: &str, rate: u32, seconds: f64, at: impl Fn(u64) -> f32) -> Self {
        let frames = (seconds * f64::from(rate)) as u64;
        Self {
            name: name.into(),
            data: Arc::new((0..frames).map(at).collect()),
            channels: 1,
            sample_rate: rate,
        }
    }

    fn from_wav(path: &Path) -> Option<Self> {
        let audio = wav::read(path).ok()?;
        let frames = audio.channels.first()?.len();
        let data = (0..frames)
            .flat_map(|f| audio.channels.iter().map(move |c| c[f]))
            .collect();
        Some(Self {
            name: path.file_stem()?.to_string_lossy().into_owned(),
            data: Arc::new(data),
            channels: audio.channels.len() as u16,
            sample_rate: audio.sample_rate,
        })
    }
}

/// The user's samples (if any), then the built-in test signals.
pub fn builtin_bank(rate: u32) -> Vec<Sample> {
    let mut bank = user_samples();
    bank.extend(test_signals(rate));
    bank
}

fn user_samples() -> Vec<Sample> {
    let Some(dir) = std::env::var_os(SAMPLES_DIR_ENV) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
        .collect();
    paths.sort();
    paths.iter().filter_map(|p| Sample::from_wav(p)).collect()
}

fn test_signals(rate: u32) -> Vec<Sample> {
    let sr = f64::from(rate);
    let seconds = |frame: u64| frame as f64 / sr;
    let decay = |frame: u64, tau: f64| (-seconds(frame) / tau).exp() as f32;
    let sine = |frame: u64, hz: f64| (std::f64::consts::TAU * hz * seconds(frame)).sin() as f32;
    let noise = Noise {
        seed: 0xDEAD_BEEF,
        amplitude: 0.6,
    };
    let sweep = LogSweep {
        start_hz: AUDIBLE_MIN_HZ,
        end_hz: AUDIBLE_MAX_HZ,
        seconds: 2.0,
        amplitude: 0.4,
        sample_rate: sr,
    };
    let burst_len = 0.08;
    vec![
        Sample::mono(
            "impulse",
            rate,
            64.0 / sr,
            |f| if f == 0 { 1.0 } else { 0.0 },
        ),
        Sample::mono("noise burst", rate, burst_len, |f| {
            noise.at(0, f) * (1.0 - (seconds(f) / burst_len) as f32)
        }),
        Sample::mono("pluck 220 Hz", rate, 0.6, |f| {
            0.4 * sine(f, 220.0) * decay(f, 0.21)
        }),
        Sample::mono("pluck 440 Hz", rate, 0.6, |f| {
            0.4 * sine(f, 440.0) * decay(f, 0.21)
        }),
        Sample::mono("chord (C major)", rate, 1.2, |f| {
            let triad = sine(f, 261.63) + sine(f, 329.63) + sine(f, 392.0);
            0.35 / 3.0 * triad * decay(f, 0.48)
        }),
        Sample::mono("log sweep 20–20k", rate, sweep.seconds, |f| {
            sweep.at(0, f)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signals_have_their_lengths_and_levels() {
        let bank = test_signals(48_000);
        let by_name = |n: &str| bank.iter().find(|s| s.name == n).unwrap();
        assert_eq!(by_name("impulse").data[0], 1.0);
        assert_eq!(by_name("log sweep 20–20k").duration_ms(), 2000);
        let pluck = by_name("pluck 440 Hz");
        let peak = pluck.data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 0.4 && peak > 0.35, "{peak}");
        assert!(pluck.data.last().unwrap().abs() < 0.03, "decayed");
    }

    #[test]
    fn wav_files_interleave_their_channels() {
        let dir = std::env::temp_dir().join(format!("stave-play-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        let audio = wav::Audio {
            sample_rate: 44_100,
            channels: vec![vec![1.0, 2.0], vec![-1.0, -2.0]],
        };
        wav::write(&path, &audio).unwrap();
        let s = Sample::from_wav(&path).unwrap();
        assert_eq!(
            (s.name.as_str(), s.channels, &s.data[..]),
            ("stereo", 2, &[1.0, -1.0, 2.0, -2.0][..])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
