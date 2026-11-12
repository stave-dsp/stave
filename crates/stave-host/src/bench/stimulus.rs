//! What the bench feeds the plugin: a parsed, validated spec, built
//! into a [`Signal`] once the sample rate is known.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use stave_measure::signal::{db_to_amp, LogSweep, Noise, Recording, Signal, Silence, Sine};
use crate::wav::WavError;
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

#[derive(Debug, thiserror::Error)]
pub enum StimulusError {
    #[error("unknown signal '{spec}' (sine, impulse, silence, noise, sweep, wav)")]
    UnknownKind { spec: String },
    #[error("'{field}' is not a number in '{spec}'")]
    NotANumber { field: String, spec: String },
    #[error("too many fields in '{spec}'")]
    TooManyFields { spec: String },
    #[error("'{kind}' takes no fields")]
    TakesNoFields { kind: String },
    #[error("sweep needs both F0 and F1 in '{spec}'")]
    SweepNeedsBothEnds { spec: String },
    #[error("wav needs a path: wav:PATH")]
    WavNeedsPath,
    #[error(transparent)]
    Wav(#[from] WavError),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stimulus {
    Sine {
        freq: f64,
        db: f64,
    },
    /// Silence through the timed run (latency still uses its impulse).
    Impulse,
    Silence,
    Noise {
        db: f64,
    },
    Sweep {
        start_hz: f64,
        end_hz: f64,
        db: f64,
    },
    Wav {
        path: PathBuf,
    },
}

impl Default for Stimulus {
    fn default() -> Self {
        Self::Sine {
            freq: Self::SINE_HZ,
            db: Self::LEVEL_DB,
        }
    }
}

impl Stimulus {
    /// What `parse` fills in for omitted fields.
    pub const SINE_HZ: f64 = 1_000.0;
    pub const LEVEL_DB: f64 = -12.0;
    /// Lower than tones: noise's crest factor is higher.
    pub const NOISE_DB: f64 = -18.0;

    /// `sine[:FREQ[:DBFS]] | impulse | silence | noise[:DBFS] |
    /// sweep[:F0:F1[:DBFS]] | wav:PATH`
    pub fn parse(s: &str) -> Result<Self, StimulusError> {
        let (kind, rest) = s.split_once(':').unwrap_or((s, ""));
        let nums = || -> Result<Vec<f64>, StimulusError> {
            rest.split(':')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    p.parse().map_err(|_| StimulusError::NotANumber {
                        field: p.to_string(),
                        spec: s.to_string(),
                    })
                })
                .collect()
        };
        let arity = |n: &[f64], max: usize| {
            if n.len() > max {
                Err(StimulusError::TooManyFields {
                    spec: s.to_string(),
                })
            } else {
                Ok(())
            }
        };
        match kind {
            "sine" => {
                let n = nums()?;
                arity(&n, 2)?;
                Ok(Self::Sine {
                    freq: n.first().copied().unwrap_or(Self::SINE_HZ),
                    db: n.get(1).copied().unwrap_or(Self::LEVEL_DB),
                })
            }
            "impulse" | "silence" if !rest.is_empty() => Err(StimulusError::TakesNoFields {
                kind: kind.to_string(),
            }),
            "impulse" => Ok(Self::Impulse),
            "silence" => Ok(Self::Silence),
            "noise" => {
                let n = nums()?;
                arity(&n, 1)?;
                Ok(Self::Noise {
                    db: n.first().copied().unwrap_or(Self::NOISE_DB),
                })
            }
            "sweep" => {
                let n = nums()?;
                arity(&n, 3)?;
                if n.len() == 1 {
                    return Err(StimulusError::SweepNeedsBothEnds {
                        spec: s.to_string(),
                    });
                }
                Ok(Self::Sweep {
                    start_hz: n.first().copied().unwrap_or(AUDIBLE_MIN_HZ),
                    end_hz: n.get(1).copied().unwrap_or(AUDIBLE_MAX_HZ),
                    db: n.get(2).copied().unwrap_or(Self::LEVEL_DB),
                })
            }
            "wav" if !rest.is_empty() => Ok(Self::Wav {
                path: PathBuf::from(rest),
            }),
            "wav" => Err(StimulusError::WavNeedsPath),
            _ => Err(StimulusError::UnknownKind {
                spec: s.to_string(),
            }),
        }
    }

    /// The signal at `rate`. Sines are bin-snapped to `analysis_n`
    /// (coherent THD); sweeps span `seconds`.
    pub fn build(
        &self,
        rate: f64,
        analysis_n: usize,
        seconds: f64,
    ) -> Result<Built, StimulusError> {
        Ok(match self {
            Self::Sine { freq, db } => {
                let s = Sine {
                    frequency_hz: *freq,
                    amplitude: db_to_amp(*db),
                    sample_rate: rate,
                }
                .bin_snapped(analysis_n);
                Built {
                    signal: Box::new(s),
                    tone: Some(s),
                    warning: None,
                }
            }
            Self::Impulse | Self::Silence => Built::plain(Silence),
            Self::Noise { db } => Built::plain(Noise {
                seed: 0x5EED,
                amplitude: db_to_amp(*db) as f32,
            }),
            Self::Sweep {
                start_hz,
                end_hz,
                db,
            } => Built::plain(LogSweep {
                start_hz: *start_hz,
                end_hz: *end_hz,
                seconds,
                amplitude: db_to_amp(*db),
                sample_rate: rate,
            }),
            Self::Wav { path } => {
                let audio = crate::wav::read(path)?;
                let warning = (f64::from(audio.sample_rate) != rate).then(|| {
                    format!(
                        "wav is {} Hz, host runs {rate} Hz — played without resampling",
                        audio.sample_rate
                    )
                });
                let signal = Recording {
                    channels: Arc::new(audio.channels),
                };
                Built {
                    signal: Box::new(signal),
                    tone: None,
                    warning,
                }
            }
        })
    }
}

impl fmt::Display for Stimulus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sine { freq, db } => write!(f, "sine {freq} Hz @ {db} dBFS"),
            Self::Impulse => f.write_str("impulse"),
            Self::Silence => f.write_str("silence"),
            Self::Noise { db } => write!(f, "noise @ {db} dBFS"),
            Self::Sweep {
                start_hz,
                end_hz,
                db,
            } => write!(f, "log sweep {start_hz}–{end_hz} Hz @ {db} dBFS"),
            Self::Wav { path } => write!(f, "wav {}", path.display()),
        }
    }
}

/// A stimulus ready to render.
pub struct Built {
    pub signal: Box<dyn Signal>,
    /// The exact tone fed in, when the stimulus is a sine.
    pub tone: Option<Sine>,
    pub warning: Option<String>,
}

impl Built {
    fn plain(s: impl Signal + 'static) -> Self {
        Self {
            signal: Box::new(s),
            tone: None,
            warning: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_form_with_defaults() {
        assert_eq!(Stimulus::parse("sine").unwrap(), Stimulus::default());
        assert_eq!(
            Stimulus::parse("sine:440").unwrap(),
            Stimulus::Sine {
                freq: 440.0,
                db: -12.0
            }
        );
        assert_eq!(
            Stimulus::parse("sine:440:-3").unwrap(),
            Stimulus::Sine {
                freq: 440.0,
                db: -3.0
            }
        );
        assert_eq!(Stimulus::parse("impulse").unwrap(), Stimulus::Impulse);
        assert_eq!(
            Stimulus::parse("noise:-6").unwrap(),
            Stimulus::Noise { db: -6.0 }
        );
        assert_eq!(
            Stimulus::parse("sweep:50:5000").unwrap(),
            Stimulus::Sweep {
                start_hz: 50.0,
                end_hz: 5_000.0,
                db: -12.0
            }
        );
        assert_eq!(
            Stimulus::parse("wav:/a:b.wav").unwrap(),
            Stimulus::Wav {
                path: "/a:b.wav".into()
            }
        );
    }

    #[test]
    fn rejects_what_it_cannot_honor() {
        for bad in [
            "sinewave",
            "sine:abc",
            "sine:1:2:3",
            "impulse:3",
            "sweep:100",
            "wav",
            "pink",
        ] {
            assert!(Stimulus::parse(bad).is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn sine_builds_bin_snapped_with_its_tone() {
        let b = Stimulus::default().build(48_000.0, 32_768, 1.0).unwrap();
        let t = b.tone.unwrap();
        assert_eq!((t.frequency_hz * 32_768.0 / 48_000.0).fract(), 0.0);
        assert_eq!(b.signal.at(0, 5), t.at(0, 5));
    }

    #[test]
    fn missing_wav_is_an_error() {
        let s = Stimulus::Wav {
            path: "/nope.wav".into(),
        };
        assert!(s.build(48_000.0, 1024, 1.0).is_err());
    }
}
