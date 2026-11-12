//! What a host tool opens: a VST3 bundle or a catalog stage
//! (`stage:<name>`), behind one type so every tool accepts both.

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
    #[error("no stage '{name}' (catalog: {})", catalog.join(", "))]
    UnknownModule {
        name: String,
        catalog: Vec<&'static str>,
    },
}

pub enum Opened {
    Plugin(Box<LoadedPlugin>),
    Stage(CatalogInstance),
}

impl Opened {
    pub fn open(path: &Path, config: HostConfig) -> Result<Self, OpenError> {
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
            Self::Stage(m) => m,
        }
    }

    pub fn into_instance(self) -> Box<dyn Instance> {
        match self {
            Self::Plugin(p) => p,
            Self::Stage(m) => Box::new(m),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Plugin(p) => p.info().class.name.clone(),
            Self::Stage(m) => format!("{}{}", stages::PREFIX, m.name()),
        }
    }
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
