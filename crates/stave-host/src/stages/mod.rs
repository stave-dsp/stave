//! stave-dsp stages by name, so every host tool loads a stage the way
//! it loads a plugin: `stage:<name>`, its settings stave-core params.
//! A param change is applied in place at the next block: no
//! allocation, state kept, safe from any thread.
//!
//! ```text
//! mod.rs       the stage model: StageSpec, StageParams
//! catalog.rs   the static catalog: every stage, its params and builder
//! classics.rs  REFERENCES: the classic netlists and the ladders, offline
//! pedals.rs    REFERENCES: the classic pedals with their knobs
//! effects.rs   REFERENCES: plate, rotary, tape, opto, converters, mic, instruments
//! volts.rs     a netlist in volts, its knobs and in-stage sweeps
//! instance.rs  CatalogInstance / CatalogHandle: a catalog stage running
//! ```

/// A choice over a static list of option names.
macro_rules! options_list {
    ($(#[$doc:meta])* $name:ident = $names:expr) => {
        $(#[$doc])*
        #[derive(Copy, Clone)]
        pub struct $name(u32);

        impl stave_core::params::ParamEnum for $name {
            const COUNT: u32 = $names.len() as u32;
            fn from_index(i: u32) -> Self {
                Self(i.min(Self::COUNT - 1))
            }
            fn to_index(self) -> u32 {
                self.0
            }
            fn name(i: u32) -> &'static str {
                $names[(i as usize).min($names.len() - 1)]
            }
        }
    };
}

mod catalog;
mod classics;
mod effects;
mod instance;
mod pedals;
mod volts;
#[cfg(test)]
mod tests;

pub use catalog::{CATALOG, REFERENCES};
pub use instance::{CatalogHandle, CatalogInstance};

use stave_core::params::{AnyParam, Params};
use stave_dsp::{NewtonStats, Stage};

pub const PREFIX: &str = "stage:";

/// A stage whose params can change while it runs.
pub trait Configurable: Stage + Send {
    fn configure(&mut self, params: &StageParams);

    /// What its nonlinear solver has done since `prepare`, if it has one.
    fn solver(&self) -> Option<NewtonStats> {
        None
    }
}

pub type BoxedStage = Box<dyn Configurable>;

/// A catalog entry: a stage's name, its params, and how it builds.
pub struct StageSpec {
    pub name: &'static str,
    /// A fresh table of the stage's params at their defaults.
    pub params: fn() -> StageParams,
    pub build: fn(&StageParams) -> BoxedStage,
}

/// A stage's params: stave-core params in declaration order (ids are
/// the positions), read by name when the stage builds or reconfigures.
pub struct StageParams(Vec<Box<dyn AnyParam>>);

impl StageParams {
    pub fn new(params: Vec<Box<dyn AnyParam>>) -> Self {
        Self(params)
    }

    /// The value of the param named `name`.
    ///
    /// # Panics
    /// If the stage declares no such param (a catalog bug).
    pub fn get(&self, name: &str) -> f64 {
        self.by_name(name)
            .unwrap_or_else(|| panic!("stage reads undeclared param '{name}'"))
            .value()
    }

    pub(crate) fn f32(&self, name: &str) -> f32 {
        self.get(name) as f32
    }

    /// A choice param's option index.
    pub fn choice(&self, name: &str) -> usize {
        self.get(name) as usize
    }
}

impl Params for StageParams {
    fn count(&self) -> usize {
        self.0.len()
    }

    fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
        self.0.get(index).map(|p| &**p)
    }
}

/// The catalog stage, or reference model, named `name`.
pub fn find(name: &str) -> Option<&'static StageSpec> {
    CATALOG.iter().chain(REFERENCES).find(|m| m.name == name)
}
