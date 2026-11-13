//! What a host tool opens: a VST3 bundle, an Audio Unit (`au:<name>` or
//! a `.component` path, macOS) or a catalog stage (`stage:<name>`),
//! behind one type so every tool accepts all three.

use std::path::{Path, PathBuf};

use crate::config::HostConfig;
use crate::instance::Instance;
use crate::stages::{self, CatalogInstance};
use crate::plugin::{HostError, LoadedPlugin};

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("could not open {}", path.display())]
    Plugin {
        path: PathBuf,
        #[source]
        source: HostError,
    },
    #[error("could not enter the linked plugin")]
    Linked(#[source] HostError),
    #[error("could not open the Audio Unit '{query}'")]
    AudioUnit {
        query: String,
        #[source]
        source: AuOpenError,
    },
    #[error("no stage '{name}' (catalog: {})", catalog.join(", "))]
    UnknownModule {
        name: String,
        catalog: Vec<&'static str>,
    },
}

#[cfg(target_os = "macos")]
pub use crate::au::AuError as AuOpenError;

/// Audio Units are a macOS format.
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("Audio Units are hosted on macOS only")]
pub struct AuOpenError;

pub enum Opened {
    Plugin(Box<LoadedPlugin>),
    #[cfg(target_os = "macos")]
    AudioUnit(Box<crate::au::AuInstance>),
    Stage(CatalogInstance),
}

impl Opened {
    pub fn open(path: &Path, config: HostConfig) -> Result<Self, OpenError> {
        if let Some(query) = au_query(path) {
            return open_au(&query, config);
        }
        let Some(name) = stage_name(path) else {
            return LoadedPlugin::open(path, config)
                .map(|p| Self::Plugin(Box::new(p)))
                .map_err(|source| OpenError::Plugin {
                    path: path.to_path_buf(),
                    source,
                });
        };
        let stage = stages::find(name).ok_or_else(|| OpenError::UnknownModule {
            name: name.to_string(),
            catalog: stages::CATALOG.iter().map(|m| m.name).collect(),
        })?;
        Ok(Self::Stage(CatalogInstance::new(stage, &config)))
    }

    pub fn instance(&mut self) -> &mut dyn Instance {
        match self {
            Self::Plugin(p) => p.as_mut(),
            #[cfg(target_os = "macos")]
            Self::AudioUnit(a) => a.as_mut(),
            Self::Stage(m) => m,
        }
    }

    pub fn into_instance(self) -> Box<dyn Instance> {
        self.into_send()
    }

    /// The instance, to move to another thread (one at a time).
    pub fn into_send(self) -> Box<dyn Instance + Send> {
        match self {
            Self::Plugin(p) => p,
            #[cfg(target_os = "macos")]
            Self::AudioUnit(a) => a,
            Self::Stage(m) => Box::new(m),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Plugin(p) => p.info().class.name.clone(),
            #[cfg(target_os = "macos")]
            Self::AudioUnit(a) => format!("{}{}", crate::au::PREFIX, a.component().full_name()),
            Self::Stage(m) => format!("{}{}", stages::PREFIX, m.name()),
        }
    }
}

/// The Audio Unit a path names (`au:<name>`, a `.component` bundle), or
/// `None` for anything else.
pub fn au_query(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    if let Some(q) = s.strip_prefix("au:") {
        return Some(q.trim().to_string());
    }
    (path.extension()? == "component").then(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?
}

#[cfg(target_os = "macos")]
fn open_au(query: &str, config: HostConfig) -> Result<Opened, OpenError> {
    crate::au::AuInstance::open(query, config)
        .map(|a| Opened::AudioUnit(Box::new(a)))
        .map_err(|source| OpenError::AudioUnit {
            query: query.to_string(),
            source,
        })
}

#[cfg(not(target_os = "macos"))]
fn open_au(query: &str, _config: HostConfig) -> Result<Opened, OpenError> {
    Err(OpenError::AudioUnit {
        query: query.to_string(),
        source: AuOpenError,
    })
}

/// `filter/peak` for `stage:filter/peak`; `None` for a bundle path.
pub fn stage_name(path: &Path) -> Option<&str> {
    path.to_str()?.strip_prefix(stages::PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_modules_by_name_and_lists_the_catalog_on_a_miss() {
        let config = HostConfig {
            max_block: std::num::NonZeroUsize::new(64).unwrap(),
            ..HostConfig::default()
        };
        let mut t = Opened::open(Path::new("stage:coupler"), config).unwrap();
        assert_eq!(t.name(), "stage:coupler");
        assert_eq!(t.instance().layout().outputs, 2);
        let Err(e) = Opened::open(Path::new("stage:nope"), config) else {
            panic!("opened a missing stage")
        };
        assert!(
            matches!(&e, OpenError::UnknownModule { catalog, .. } if catalog.contains(&"filter/peak"))
        );
    }
}
