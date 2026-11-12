//! A host's state save and load, the same for every format: parameters
//! from the shared table, the plugin's extra state through the processor
//! claim.

use super::PluginCell;
use crate::diagnostics::{CallError, Diagnostics};
use crate::params::Params;
use crate::plugin::Plugin;
use crate::state::{self, StateStore};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// How long a state call waits for the audio thread to finish its block.
pub const STATE_CLAIM_WAIT: Duration = Duration::from_millis(50);

/// A corrupt length field must not become a multi-gigabyte read.
pub const MAX_STATE_BYTES: usize = 32 * 1024 * 1024;

/// The chunk to hand the host. A busy audio thread costs the plugin's
/// extra state (logged), not the save.
pub fn save<P: Plugin>(
    plugin: &PluginCell<P>,
    table: &dyn Params,
    store: &Mutex<StateStore>,
    diagnostics: &Diagnostics,
) -> Vec<u8> {
    let extra = match plugin.claim_with_retry(STATE_CLAIM_WAIT) {
        Some(proc) => proc.plugin.save_extra(),
        None => {
            diagnostics.report("save state", &CallError::ExtraStateSkipped);
            Vec::new()
        }
    };
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    state::save(table, P::state_version(), &extra, &store)
}

/// Applies `bytes`; on a decode error nothing changes. Parameters are
/// live once decoded; extra state that cannot reach a busy plugin fails
/// the call.
pub fn load<P: Plugin>(
    bytes: &[u8],
    plugin: &PluginCell<P>,
    table: &dyn Params,
    store: &Mutex<StateStore>,
) -> Result<(), CallError> {
    let (report, extra) = state::load(
        bytes,
        table,
        &P::migrations(),
        &mut store.lock().unwrap_or_else(PoisonError::into_inner),
    )?;
    match plugin.claim_with_retry(STATE_CLAIM_WAIT) {
        Some(mut proc) => proc.plugin.load_extra(&extra, report.plugin_version),
        None if !extra.is_empty() => return Err(CallError::ProcessorBusy),
        None => {}
    }
    Ok(())
}
