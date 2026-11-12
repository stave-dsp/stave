//! Factory presets as a VST3 program-change parameter
//! ([`ParamId::PROGRAM`]): a stepped list whose positions are the
//! presets, so every host's generic UI and parameter list carry them.
//! Selecting one writes its values into the table, then asks the host to
//! re-read every parameter.

use core::sync::atomic::{AtomicU32, Ordering};

use stave_core::params::Params;
use stave_core::{ParamId, Preset};

use crate::host::ComponentHandler;
use crate::sys::{ParameterInfo, ProgramListId, RESTART_PARAM_VALUES_CHANGED};
use crate::wire_strings::write_utf16_name;

const IS_LIST: i32 = 1 << 3;
const PROGRAM_LIST_ID: ProgramListId = 1;
const IS_PROGRAM_CHANGE: i32 = 1 << 15;

pub(crate) struct Programs {
    presets: Vec<Preset>,
    current: AtomicU32,
}

impl Programs {
    pub fn new(presets: Vec<Preset>) -> Self {
        Self {
            presets,
            current: AtomicU32::new(0),
        }
    }

    /// The parameter exists only when there is something to select.
    pub fn exposed(&self) -> bool {
        !self.presets.is_empty()
    }

    /// The program list's id, when there is one (the root unit's list).
    pub fn list_id(&self) -> Option<ProgramListId> {
        self.exposed().then_some(PROGRAM_LIST_ID)
    }

    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    pub fn count(&self) -> usize {
        self.presets.len()
    }

    pub fn name_at(&self, index: usize) -> Option<&str> {
        self.presets.get(index).map(|p| p.name)
    }

    fn last(&self) -> f64 {
        (self.presets.len().max(2) - 1) as f64
    }

    pub fn index_of(&self, normalized: f64) -> usize {
        ((normalized.clamp(0.0, 1.0) * self.last()).round() as usize).min(self.presets.len() - 1)
    }

    pub fn normalized_of(&self, index: f64) -> f64 {
        (index / self.last()).clamp(0.0, 1.0)
    }

    pub fn info(&self, dest: &mut ParameterInfo) {
        dest.id = ParamId::PROGRAM.0;
        write_utf16_name(&mut dest.title, "Program");
        write_utf16_name(&mut dest.short_title, "Program");
        dest.step_count = self.presets.len() as i32 - 1;
        dest.default_normalized_value = 0.0;
        dest.flags = IS_LIST | IS_PROGRAM_CHANGE;
    }

    pub fn name(&self, normalized: f64) -> &str {
        self.presets[self.index_of(normalized)].name
    }

    pub fn find(&self, name: &str) -> Option<f64> {
        let i = self.presets.iter().position(|p| p.name == name)?;
        Some(self.normalized_of(i as f64))
    }

    pub fn normalized(&self) -> f64 {
        self.normalized_of(f64::from(self.current.load(Ordering::Relaxed)))
    }

    /// Apply the preset at `normalized` to `table` and tell the host its
    /// parameters changed.
    pub fn select(&self, normalized: f64, table: &dyn Params, handler: &ComponentHandler) {
        let index = self.index_of(normalized);
        self.current.store(index as u32, Ordering::Relaxed);
        for (id, plain) in &self.presets[index].values {
            if let Some(p) = table.by_id(*id) {
                p.set_plain(*plain);
            }
        }
        handler.restart_component(RESTART_PARAM_VALUES_CHANGED);
    }
}
