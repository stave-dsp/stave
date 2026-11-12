//! stave-dsp modules by name, so every host tool loads a module the way
//! it loads a plugin: `module:<name>` with its settings as controls.
//! A setting change is applied in place at the next block: no
//! allocation, state kept, safe from any thread.
//!
//! ```text
//! mod.rs       the module model: Module, Setting, Range, SettingValues
//! catalog.rs   the static catalog: every module, its settings and builder
//! instance.rs  ModuleInstance / ModuleHandle: a catalog module running
//! ```

mod catalog;
mod instance;
#[cfg(test)]
mod tests;

pub use catalog::CATALOG;
pub use instance::{ModuleHandle, ModuleInstance};

use stave_dsp::{NewtonStats, Stage};

pub const PREFIX: &str = "module:";

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Setting {
    pub name: &'static str,
    pub default: f64,
    pub range: Range,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Range {
    Continuous {
        min: f64,
        max: f64,
        unit: &'static str,
    },
    /// One of the named options, held as its index.
    Choice(&'static [&'static str]),
}

impl Range {
    pub fn min(&self) -> f64 {
        match self {
            Self::Continuous { min, .. } => *min,
            Self::Choice(_) => 0.0,
        }
    }

    pub fn max(&self) -> f64 {
        match self {
            Self::Continuous { max, .. } => *max,
            Self::Choice(options) => options.len().saturating_sub(1) as f64,
        }
    }

    pub fn admits(&self, value: f64) -> bool {
        let within = (self.min()..=self.max()).contains(&value);
        match self {
            Self::Continuous { .. } => within,
            Self::Choice(_) => within && value.fract() == 0.0,
        }
    }

    /// Position 0..1 across the range (linear; a lone option is 0).
    pub fn normalize(&self, value: f64) -> f64 {
        let span = self.max() - self.min();
        if span > 0.0 {
            (value - self.min()) / span
        } else {
            0.0
        }
    }

    /// The value at position 0..1; a choice lands on its nearest option.
    pub fn denormalize(&self, normalized: f64) -> f64 {
        let value = self.min() + normalized.clamp(0.0, 1.0) * (self.max() - self.min());
        match self {
            Self::Continuous { .. } => value,
            Self::Choice(_) => value.round(),
        }
    }

    fn unit(&self) -> &'static str {
        match self {
            Self::Continuous { unit, .. } => unit,
            Self::Choice(_) => "",
        }
    }

    fn options(&self) -> &'static [&'static str] {
        match self {
            Self::Continuous { .. } => &[],
            Self::Choice(options) => options,
        }
    }
}

/// A stage whose settings can change while it runs.
pub trait Configurable: Stage + Send {
    fn configure(&mut self, values: &SettingValues);

    /// What its nonlinear solver has done since `prepare`, if it has one.
    fn solver(&self) -> Option<NewtonStats> {
        None
    }
}

pub type BoxedModule = Box<dyn Configurable>;

pub struct Module {
    pub name: &'static str,
    pub settings: &'static [Setting],
    pub build: fn(&SettingValues) -> BoxedModule,
}

/// A module's current setting values, read by name.
pub struct SettingValues<'a> {
    pub(crate) settings: &'a [Setting],
    pub(crate) values: &'a [f64],
}

impl SettingValues<'_> {
    pub fn get(&self, name: &str) -> f64 {
        let i = self
            .settings
            .iter()
            .position(|s| s.name == name)
            .unwrap_or_else(|| panic!("module reads undeclared setting '{name}'"));
        self.values[i]
    }

    pub(crate) fn f32(&self, name: &str) -> f32 {
        self.get(name) as f32
    }

    /// A choice setting's option index.
    pub fn choice(&self, name: &str) -> usize {
        self.get(name) as usize
    }
}

pub fn find(name: &str) -> Option<&'static Module> {
    CATALOG.iter().find(|m| m.name == name)
}
