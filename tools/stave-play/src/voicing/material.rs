//! Program material for a comparison: a built-in item, any WAV file, a
//! file from the sample library (`STAVE_SAMPLE_LIBRARY`), or an item of a
//! reference-corpus role (`role:NAME`, DSP.md R22), brought to the
//! session's rate and named by checksum.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use stave_host::wav;
use stave_test::corpus::Corpus;

use super::builtin;
use super::sha256;

/// Where material came from: enough to find it again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Source {
    Builtin { name: String },
    File { path: PathBuf, file_sha256: String },
    /// Item `index` of a corpus role (`crates/stave-test/corpus.spec`),
    /// its excerpt, every file named with its locked SHA-256 (`None`:
    /// the lock does not pin the role).
    Corpus {
        role: String,
        index: usize,
        item: String,
        files: Vec<(String, Option<String>)>,
    },
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
    #[error("{0}")]
    Corpus(String),
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

    /// Item `index` of the corpus role `role` at `sample_rate`: through
    /// the local lock when it pins the role (drift is an error), else
    /// resolved against the library as it is.
    pub fn corpus(role: &str, index: usize, sample_rate: f64) -> Result<Self, MaterialError> {
        let err = MaterialError::Corpus;
        let corpus = Corpus::open().map_err(|s| err(s.0))?;
        if corpus.spec.role(role).is_none() {
            let names: Vec<&str> = corpus.spec.roles.iter().map(|r| r.name.as_str()).collect();
            return Err(err(format!("no corpus role {role} (roles: {})", names.join(", "))));
        }
        let items = corpus.role(role).map_err(|s| err(s.0))?;
        let item = items
            .get(index)
            .ok_or_else(|| err(format!("role {role} has {} items; #{index} is past them", items.len())))?;
        let m = corpus.load(item).map_err(err)?;
        let original_rate = f64::from(m.rate);
        let channels = m.at_rate(sample_rate);
        let name = format!("{role}#{index} {}", m.name);
        let out = Self {
            name: name.clone(),
            source: Source::Corpus {
                role: role.into(),
                index,
                item: m.name.clone(),
                files: m.files.clone(),
            },
            sample_rate,
            original_rate,
            pcm_sha256: sha256::of_audio(&channels),
            channels,
        };
        if out.seconds() < 0.4 {
            return Err(MaterialError::TooShort { name, seconds: out.seconds() });
        }
        Ok(out)
    }

    /// Bring the material back from a log or bless record.
    pub fn from_source(source: &Source, sample_rate: f64) -> Result<Self, MaterialError> {
        match source {
            Source::Builtin { name } => Self::builtin(name, sample_rate),
            Source::File { path, .. } => Self::file(path, sample_rate),
            Source::Corpus { role, index, .. } => Self::corpus(role, *index, sample_rate),
        }
    }
}

pub use stave_test::corpus::rate::convert_rate;

/// The sample library's root (`stave_test::corpus::library_root`).
pub fn library_root() -> Option<PathBuf> {
    stave_test::corpus::library_root()
}

/// `role:NAME` or `role:NAME#N` (item N, from 0): the role and index.
pub fn parse_role(spec: &str) -> Option<(String, usize)> {
    let rest = spec.strip_prefix("role:")?;
    match rest.split_once('#') {
        Some((role, n)) => Some((role.to_string(), n.parse().ok()?)),
        None => Some((rest.to_string(), 0)),
    }
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
    fn roles_parse() {
        assert_eq!(parse_role("role:mix_bus"), Some(("mix_bus".into(), 0)));
        assert_eq!(parse_role("role:guitar_di#3"), Some(("guitar_di".into(), 3)));
        assert_eq!(parse_role("role:x#y"), None);
        assert_eq!(parse_role("drum loop"), None);
    }

    #[test]
    fn an_absent_role_says_so() {
        // With or without a library: never a panic, always a reason.
        let e = Material::corpus("no_such_role", 0, 48_000.0).unwrap_err().to_string();
        assert!(e.contains("library absent") || e.contains("no corpus role no_such_role"), "{e}");
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
