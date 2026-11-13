//! Program material for a comparison: a built-in item, any WAV file, or
//! a file from the sample library (`STAVE_SAMPLE_LIBRARY`), brought to
//! the session's rate and named by checksum.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use stave_host::wav;

use super::builtin;
use super::sha256;

/// Where material came from: enough to find it again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Source {
    Builtin { name: String },
    File { path: PathBuf, file_sha256: String },
}

/// Material at the session's rate, ready to run through candidates.
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub source: Source,
    pub sample_rate: f64,
    /// The file's own rate (the built-ins are made at the session rate).
    pub original_rate: f64,
    /// One or two channels, equal length.
    pub channels: Vec<Vec<f32>>,
    /// SHA-256 of the samples as played (after any rate conversion).
    pub pcm_sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MaterialError {
    #[error("no built-in item '{0}'")]
    UnknownBuiltin(String),
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: wav::WavError,
    },
    #[error("could not read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} is empty", .0.display())]
    Empty(PathBuf),
    #[error("{name} is {seconds:.2} s; loudness needs at least 0.4 s")]
    TooShort { name: String, seconds: f64 },
}

impl Material {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate
    }

    pub fn builtin(name: &str, sample_rate: f64) -> Result<Self, MaterialError> {
        let item = builtin::find(name).ok_or_else(|| MaterialError::UnknownBuiltin(name.into()))?;
        let channels = item.make(sample_rate);
        Ok(Self {
            name: item.name.to_string(),
            source: Source::Builtin {
                name: item.name.to_string(),
            },
            sample_rate,
            original_rate: sample_rate,
            pcm_sha256: sha256::of_audio(&channels),
            channels,
        })
    }

    /// A WAV file at `sample_rate` (converted if the file's rate
    /// differs). More than two channels keep the first two.
    pub fn file(path: &Path, sample_rate: f64) -> Result<Self, MaterialError> {
        let bytes = std::fs::read(path).map_err(|source| MaterialError::Io {
            path: path.into(),
            source,
        })?;
        let audio = wav::read(path).map_err(|source| MaterialError::Read {
            path: path.into(),
            source,
        })?;
        let mut channels: Vec<Vec<f32>> = audio.channels.into_iter().take(2).collect();
        if channels.first().is_none_or(Vec::is_empty) {
            return Err(MaterialError::Empty(path.into()));
        }
        let original_rate = f64::from(audio.sample_rate);
        if (original_rate - sample_rate).abs() > 0.5 {
            channels = channels
                .iter()
                .map(|c| convert_rate(c, original_rate, sample_rate))
                .collect();
        }
        let name = path
            .file_stem()
            .map_or_else(|| "?".into(), |s| s.to_string_lossy().into_owned());
        let m = Self {
            name: name.clone(),
            source: Source::File {
                path: path.into(),
                file_sha256: sha256::hex(&bytes),
            },
            sample_rate,
            original_rate,
            pcm_sha256: sha256::of_audio(&channels),
            channels,
        };
        if m.seconds() < 0.4 {
            return Err(MaterialError::TooShort {
                name,
                seconds: m.seconds(),
            });
        }
        Ok(m)
    }

    /// Bring the material back from a log or bless record.
    pub fn from_source(source: &Source, sample_rate: f64) -> Result<Self, MaterialError> {
        match source {
            Source::Builtin { name } => Self::builtin(name, sample_rate),
            Source::File { path, .. } => Self::file(path, sample_rate),
        }
    }
}

/// Windowed-sinc rate conversion (Blackman window, 32 zero crossings
/// each side, cut-off at 0.95 of the lower Nyquist). Offline, for
/// material only.
pub fn convert_rate(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    const HALF: i64 = 32;
    let ratio = to / from;
    let cutoff = 0.95 * ratio.min(1.0);
    let out_len = (x.len() as f64 * ratio).round() as usize;
    let len = x.len() as i64;
    (0..out_len)
        .map(|n| {
            let t = n as f64 / ratio;
            let centre = t.floor() as i64;
            let reach = (HALF as f64 / cutoff).ceil() as i64;
            let mut acc = 0.0;
            for i in (centre - reach + 1).max(0)..=(centre + reach).min(len - 1) {
                let d = t - i as f64;
                let u = d * cutoff;
                if u.abs() >= HALF as f64 {
                    continue;
                }
                let sinc = if u == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
                };
                let w = u / HALF as f64; // −1 … 1
                let win = 0.42 + 0.5 * (std::f64::consts::PI * w).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                acc += f64::from(x[i as usize]) * sinc * win * cutoff;
            }
            acc as f32
        })
        .collect()
}

/// The sample library's root: `STAVE_SAMPLE_LIBRARY` from the
/// environment, else from the local configuration files the scripts
/// read (`.stave/local.env` in the checkout, `~/.config/stave/env`).
pub fn library_root() -> Option<PathBuf> {
    const KEY: &str = "STAVE_SAMPLE_LIBRARY";
    if let Some(v) = std::env::var_os(KEY) {
        return Some(PathBuf::from(v));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let files = [
        Some(PathBuf::from(".stave/local.env")),
        home.map(|h| h.join(".config/stave/env")),
    ];
    files
        .into_iter()
        .flatten()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .find_map(|text| config_value(&text, KEY))
        .map(PathBuf::from)
}

/// `KEY=value` or `KEY="value"` from a shell-style config file.
fn config_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line
            .strip_prefix("export ")
            .unwrap_or(line)
            .strip_prefix(key)?
            .strip_prefix('=')?;
        let v = rest.trim().trim_matches('"').trim_matches('\'');
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// The library's program material: every WAV under its `Test Corpus/`
/// (the curated set), relative path and full path, sorted. Empty when
/// the library is absent (unset, or the drive not mounted).
pub fn library_files(root: &Path) -> Vec<(String, PathBuf)> {
    let corpus = root.join("Test Corpus");
    let mut out = Vec::new();
    let mut stack = vec![corpus.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
                let rel = p
                    .strip_prefix(&corpus)
                    .map_or_else(|_| p.display().to_string(), |r| r.display().to_string());
                out.push((rel, p));
            }
        }
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_conversion_keeps_a_tone() {
        let (from, to) = (44_100.0, 48_000.0);
        let x: Vec<f32> = (0..44_100)
            .map(|i| (std::f64::consts::TAU * 1_000.0 * f64::from(i) / from).sin() as f32 * 0.5)
            .collect();
        let y = convert_rate(&x, from, to);
        assert_eq!(y.len(), 48_000);
        // Compare against the ideal tone at the new rate, away from the edges.
        let err = (2_000..46_000)
            .map(|i| {
                let ideal = (std::f64::consts::TAU * 1_000.0 * i as f64 / to).sin() * 0.5;
                (f64::from(y[i]) - ideal).abs()
            })
            .fold(0.0, f64::max);
        assert!(err < 1e-3, "{err}");
    }

    #[test]
    fn config_values_parse_like_the_scripts() {
        let text = "# c\nSTAVE_LINUX_HOST=x\nSTAVE_SAMPLE_LIBRARY=\"/a b/lib\"\n";
        assert_eq!(config_value(text, "STAVE_SAMPLE_LIBRARY").as_deref(), Some("/a b/lib"));
        assert_eq!(config_value("export K='v'", "K").as_deref(), Some("v"));
        assert_eq!(config_value("KEY2=1", "KEY"), None);
    }

    #[test]
    fn files_are_named_by_content() {
        let dir = std::env::temp_dir().join(format!("stave-voicing-mat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let audio = wav::Audio {
            sample_rate: 48_000,
            channels: vec![(0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.3).collect()],
        };
        wav::write(&path, &audio).unwrap();
        let a = Material::file(&path, 48_000.0).unwrap();
        let b = Material::file(&path, 48_000.0).unwrap();
        assert_eq!(a.pcm_sha256, b.pcm_sha256);
        assert_eq!(a.channels.len(), 1);
        let Source::File { file_sha256, .. } = &a.source else { panic!() };
        assert_eq!(file_sha256.len(), 64);
        let short = dir.join("short.wav");
        wav::write(&short, &wav::Audio { sample_rate: 48_000, channels: vec![vec![0.1; 100]] }).unwrap();
        assert!(matches!(Material::file(&short, 48_000.0), Err(MaterialError::TooShort { .. })));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
