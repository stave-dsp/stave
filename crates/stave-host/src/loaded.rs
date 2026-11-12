//! What a host tool opens: a VST3 bundle or a catalog module
//! (`module:<name>`), behind one type so every tool accepts both.

use std::path::{Path, PathBuf};

use crate::config::HostConfig;
use crate::instance::Instance;
use crate::modules::{self, ModuleInstance};
use crate::plugin::{HostError, LoadedPlugin};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not open {}", path.display())]
    Plugin {
        path: PathBuf,
        #[source]
        source: HostError,
    },
    #[error("could not enter the linked plugin")]
    Linked(#[source] HostError),
    #[error("no module '{name}' (catalog: {})", catalog.join(", "))]
    UnknownModule {
        name: String,
        catalog: Vec<&'static str>,
    },
}

pub enum Loaded {
    Plugin(Box<LoadedPlugin>),
    Module(ModuleInstance),
}

impl Loaded {
    pub fn open(path: &Path, config: HostConfig) -> Result<Self, LoadError> {
        let Some(name) = module_name(path) else {
            return LoadedPlugin::open(path, config)
                .map(|p| Self::Plugin(Box::new(p)))
                .map_err(|source| LoadError::Plugin {
                    path: path.to_path_buf(),
                    source,
                });
        };
        let module = modules::find(name).ok_or_else(|| LoadError::UnknownModule {
            name: name.to_string(),
            catalog: modules::CATALOG.iter().map(|m| m.name).collect(),
        })?;
        Ok(Self::Module(ModuleInstance::new(module, &config)))
    }

    pub fn instance(&mut self) -> &mut dyn Instance {
        match self {
            Self::Plugin(p) => p.as_mut(),
            Self::Module(m) => m,
        }
    }

    pub fn into_instance(self) -> Box<dyn Instance> {
        match self {
            Self::Plugin(p) => p,
            Self::Module(m) => Box::new(m),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Plugin(p) => p.info().class.name.clone(),
            Self::Module(m) => format!("{}{}", modules::PREFIX, m.name()),
        }
    }
}

/// `filter/peak` for `module:filter/peak`; `None` for a bundle path.
pub fn module_name(path: &Path) -> Option<&str> {
    path.to_str()?.strip_prefix(modules::PREFIX)
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
        let mut t = Loaded::open(Path::new("module:coupler"), config).unwrap();
        assert_eq!(t.name(), "module:coupler");
        assert_eq!(t.instance().layout().outputs, 2);
        let Err(e) = Loaded::open(Path::new("module:nope"), config) else {
            panic!("opened a missing module")
        };
        assert!(
            matches!(&e, LoadError::UnknownModule { catalog, .. } if catalog.contains(&"filter/peak"))
        );
    }
}
