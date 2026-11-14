//! `kAudioUnitProperty_ClassInfo`: the session's state as the property
//! list Apple's `AUBase` writes — `version`, `type`, `subtype`,
//! `manufacturer`, `name`, and `data`, whose bytes are the framework's
//! own state (`stave_core::runtime::state`, the bytes the CLAP and VST3
//! wrappers save), so unknown params, migrations and the plugin's extra
//! state behave as in every format.

use stave_core::runtime::state as wrapper_state;
use stave_core::runtime::state::MAX_STATE_BYTES;

use super::*;
use crate::sys::fourcc;

/// The state format's version (`AUBase`'s current one).
const VERSION: i32 = 0;

/// The present preset's name before any is chosen.
pub(crate) const UNTITLED: &str = "Untitled";

/// The state as a CFDictionary the caller releases.
pub(crate) fn save<P: AuExport>(inst: &Instance<P>) -> cf::Cf {
    let bytes = wrapper_state::save(&inst.plugin, inst.params.as_ref(), &inst.state_store, &inst.diagnostics);
    let dict = cf::dictionary();
    let id = inst.identity;
    cf::insert(&dict, "version", &cf::number(VERSION));
    cf::insert(&dict, "type", &cf::number(fourcc(&id.kind) as i32));
    cf::insert(&dict, "subtype", &cf::number(fourcc(&id.subtype) as i32));
    cf::insert(&dict, "manufacturer", &cf::number(fourcc(&id.manufacturer) as i32));
    cf::insert(&dict, "name", &cf::string(&lock(&inst.present).1));
    cf::insert(&dict, "data", &cf::data(&bytes));
    dict
}

/// Restore the state in `list` (a ClassInfo dictionary this component
/// wrote).
///
/// # Safety
/// `list` is null or a live CoreFoundation object.
pub(crate) unsafe fn load<P: AuExport>(inst: &Instance<P>, list: sys::CFPropertyListRef) -> OSStatus {
    let id = inst.identity;
    // SAFETY: the caller's contract; every read checks its type.
    let (version, kind, subtype, manufacturer, data, name) = unsafe {
        (
            cf::read_number(cf::lookup(list, "version")),
            cf::read_number(cf::lookup(list, "type")),
            cf::read_number(cf::lookup(list, "subtype")),
            cf::read_number(cf::lookup(list, "manufacturer")),
            cf::read_data(cf::lookup(list, "data")),
            cf::read_string(cf::lookup(list, "name")),
        )
    };
    let ours = |code: Option<i32>, want: &[u8; 4]| code == Some(fourcc(want) as i32);
    if version != Some(VERSION) || !ours(kind, &id.kind) || !ours(subtype, &id.subtype) || !ours(manufacturer, &id.manufacturer) {
        return sys::kAudioUnitErr_InvalidPropertyValue;
    }
    let Some(bytes) = data.filter(|d| d.len() <= MAX_STATE_BYTES) else {
        return sys::kAudioUnitErr_InvalidPropertyValue;
    };
    let result = wrapper_state::load(&bytes, &inst.plugin, inst.params.as_ref(), &inst.state_store);
    inst.diagnostics.event(stave_core::telemetry::Event::Restore(result.is_ok()));
    match result {
        Ok(()) => {
            *lock(&inst.present) = (-1, name.unwrap_or_else(|| UNTITLED.to_string()));
            params::all_changed(inst);
            sys::noErr
        }
        Err(error) => {
            inst.report("ClassInfo", &error);
            sys::kAudioUnitErr_InvalidPropertyValue
        }
    }
}
