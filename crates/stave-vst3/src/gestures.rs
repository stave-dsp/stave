//! The face's gesture sink: every editor edit goes to the host's
//! `IComponentHandler` (beginEdit / performEdit / endEdit), so the DAW
//! records and undoes it. The `PluginController` has already written the
//! table.

use std::sync::Arc;

use stave_core::params::{ParamId, Params};
use stave_core::GestureSink;

use crate::host::ComponentHandler;

/// [`GestureSink`] over the instance's handler (whichever the host has
/// set, now or later) and its table (for the wire's normalized value).
pub(crate) struct Vst3Gestures {
    pub(crate) handler: Arc<ComponentHandler>,
    pub(crate) table: Arc<dyn Params>,
}

impl GestureSink for Vst3Gestures {
    fn begin(&self, id: ParamId) {
        self.handler.begin_edit(id.0);
    }
    fn perform(&self, id: ParamId, plain: f64) {
        if let Some(p) = self.table.by_id(id) {
            self.handler.perform_edit(id.0, p.normalize(plain));
        }
    }
    fn end(&self, id: ParamId) {
        self.handler.end_edit(id.0);
    }
}
