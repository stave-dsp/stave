//! Why a network description could not be loaded.

use std::fmt;

/// Why a network file or weight list could not become a network. Every
/// variant names what was wrong, so a plugin can show it to the user
/// who picked the file.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// The text is not JSON: the byte offset and the reason.
    Json { offset: usize, reason: &'static str },
    /// The file's format version is outside the versions this reader
    /// implements.
    Version(String),
    /// An architecture this reader does not implement (`ConvNet`,
    /// `Linear` …), at the top level or as a container's submodel (then
    /// prefixed with the submodel's path, `SlimmableContainer
    /// config.submodels[i].model: …`).
    Architecture(String),
    /// A field that is missing or malformed.
    Config(String),
    /// A feature of a known architecture that this reader does not
    /// implement (FiLM, grouped convolutions, a post-head …), named.
    Unsupported(String),
    /// The weight list's length does not match the architecture.
    Weights { expected: usize, found: usize },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Json { offset, reason } => write!(f, "not JSON: {reason} at byte {offset}"),
            LoadError::Version(v) => write!(f, "unsupported file version {v} (reads 0.5 to 0.7)"),
            LoadError::Architecture(a) => write!(f, "unsupported architecture \"{a}\""),
            LoadError::Config(what) => write!(f, "malformed config: {what}"),
            LoadError::Unsupported(what) => write!(f, "unsupported feature: {what}"),
            LoadError::Weights { expected, found } => {
                write!(
                    f,
                    "the architecture needs {expected} weights, the file has {found}"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// A cursor over a flat weight list, read in the order the exporter
/// wrote it. Running past the end is counted, not panicked on, so the
/// error can report the full expected length.
pub(crate) struct Weights<'a> {
    data: &'a [f32],
    at: usize,
}

impl<'a> Weights<'a> {
    pub(crate) fn new(data: &'a [f32]) -> Self {
        Self { data, at: 0 }
    }

    /// The next `n` weights; past the end, zeros (and [`finish`](Self::finish)
    /// reports the shortfall).
    pub(crate) fn take(&mut self, n: usize) -> impl Iterator<Item = f32> + '_ {
        let start = self.at;
        self.at += n;
        (start..start + n).map(|i| self.data.get(i).copied().unwrap_or(0.0))
    }

    /// One weight.
    pub(crate) fn one(&mut self) -> f32 {
        self.take(1).next().unwrap_or(0.0)
    }

    /// Every weight consumed exactly.
    pub(crate) fn finish(self) -> Result<(), LoadError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(LoadError::Weights {
                expected: self.at,
                found: self.data.len(),
            })
        }
    }
}
