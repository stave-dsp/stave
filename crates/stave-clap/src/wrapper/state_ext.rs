//! The `clap.state` extension, backed by the param table.

use super::*;

pub(super) unsafe extern "C" fn state_save<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    stream: *const sys::ClapOstream,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, stream) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            OutputStream::new(stream),
        )
    };
    inst.call("state.save", || {
        let result = stream.and_then(|stream| {
            stream.write_all(&wrapper_state::save(
                &inst.plugin,
                inst.param_table.as_ref(),
                &inst.state_store,
                &inst.diagnostics,
            ))
        });
        result
    })
}

pub(super) unsafe extern "C" fn state_load<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    stream: *const sys::ClapIstream,
) -> bool {
    // SAFETY: host contract (see `host`).
    let (inst, stream) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            InputStream::new(stream),
        )
    };
    inst.call("state.load", || {
        let result = stream.and_then(|stream| {
            wrapper_state::load(
                &stream.read_to_end(MAX_STATE_BYTES)?,
                &inst.plugin,
                inst.param_table.as_ref(),
                &inst.state_store,
            )
        });
        // The values just changed under the host: it must re-read them
        // (clap ext/state.h, ext/params.h).
        if result.is_ok() {
            if let Some(host) = inst.host {
                host.rescan_param_values();
            }
        }
        result
    })
}
