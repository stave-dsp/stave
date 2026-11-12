//! The editor's parameters: the plugin's typed table, read directly, and
//! the host's gesture path for every edit, so the DAW records and undoes
//! GUI changes. A control index is the param's table index.

extern crate alloc;
use alloc::boxed::Box;
use alloc::sync::Arc;

use stave_core::params::{AnyParam, ParamId, Params};

/// The wrapper-side gesture sink. VST3 backs this with the captured
/// IComponentHandler; CLAP (when its editor lands) with GestureQueue.
pub trait EditorHost {
    fn begin_edit(&self, id: u32);
    fn perform_edit(&self, id: u32, plain: f64);
    fn end_edit(&self, id: u32);
}

pub(crate) struct EditorBinding {
    pub(crate) table: Arc<dyn Params>,
    pub(crate) host: Box<dyn EditorHost>,
}

impl EditorBinding {
    /// The param at a control index (indices come from the table).
    pub(crate) fn param(&self, ci: usize) -> &dyn AnyParam {
        self.table
            .by_index(ci)
            .expect("control indices come from the table")
    }

    pub(crate) fn plain(&self, ci: usize) -> f64 {
        self.param(ci).plain()
    }
}

impl super::view::PluginEditorView {
    pub(crate) fn param_count(&self) -> usize {
        self.binding.table.count()
    }

    pub(crate) fn param(&self, ci: usize) -> &dyn AnyParam {
        self.binding.param(ci)
    }

    pub(crate) fn plain(&self, ci: usize) -> f64 {
        self.binding.plain(ci)
    }

    /// Plain values at the ends of the param's range.
    pub(crate) fn param_range(&self, ci: usize) -> (f64, f64) {
        let p = self.param(ci);
        (p.normalized_to_plain(0.0), p.normalized_to_plain(1.0))
    }

    pub(crate) fn param_index_of(&self, id: ParamId) -> Option<usize> {
        (0..self.param_count()).find(|&i| self.param(i).meta().id == id)
    }

    /// Editor-originated value change, through the host's gesture path.
    /// `bracket` wraps it in begin/end for one-shot controls (toggles,
    /// dropdowns, preset applies); drags bracket at mouseDown/mouseUp
    /// via begin_control/end_control instead.
    pub fn write_control_plain(&self, ci: usize, plain: f64, bracket: bool) {
        let id = self.param(ci).meta().id.0;
        let host = &self.binding.host;
        if bracket {
            host.begin_edit(id);
        }
        host.perform_edit(id, plain);
        if bracket {
            host.end_edit(id);
        }
    }

    pub fn begin_control(&self, ci: usize) {
        self.binding.host.begin_edit(self.param(ci).meta().id.0);
    }

    pub fn end_control(&self, ci: usize) {
        self.binding.host.end_edit(self.param(ci).meta().id.0);
    }
}
