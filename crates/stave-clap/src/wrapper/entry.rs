//! `clap_entry` and the plugin factory.

use super::*;

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn entry_init(_plugin_path: *const c_char) -> bool {
    true
}

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn entry_deinit() {}

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn entry_get_factory<P: ClapExport>(
    factory_id: *const c_char,
) -> *const c_void {
    // SAFETY: clap.h: null or a nul-terminated id.
    let requested = unsafe { in_text(factory_id, "factory_id") };
    ffi_entry::<P, _>(ptr::null, || {
        if requested.is_ok_and(|id| id == sys::CLAP_PLUGIN_FACTORY_ID) {
            P::clap_factory() as *const _ as *const c_void
        } else {
            ptr::null()
        }
    })
}

// ===========================================================================
// Factory vtable
// ===========================================================================

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn factory_get_plugin_count<P: ClapExport>(
    _factory: *const sys::ClapPluginFactory,
) -> u32 {
    1
}

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn factory_get_plugin_descriptor<P: ClapExport>(
    _factory: *const sys::ClapPluginFactory,
    index: u32,
) -> *const sys::ClapPluginDescriptor {
    ffi_entry::<P, _>(ptr::null, || {
        if index == 0 {
            P::clap_descriptor() as *const _
        } else {
            ptr::null()
        }
    })
}

/// # Safety
/// Called by the CLAP host under the clap.h entry/factory contract.
pub unsafe extern "C" fn factory_create_plugin<P: ClapExport>(
    _factory: *const sys::ClapPluginFactory,
    host: *const sys::ClapHost,
    plugin_id: *const c_char,
) -> *const sys::ClapPlugin {
    // SAFETY: clap.h: null or a nul-terminated id.
    let requested = unsafe { in_text(plugin_id, "plugin_id") };
    if requested.ok() != Some(P::DESCRIPTOR.id) {
        return ptr::null();
    }
    // `P::new()` and the author hooks below are plugin code. On a panic
    // the partly built values drop during the unwind and the host gets
    // null.
    // SAFETY: clap.h: `host` is null or outlives the instance.
    ffi_entry::<P, _>(ptr::null, || unsafe { create_plugin::<P>(host) })
}

/// # Safety
/// `host` is null or a `clap_host` that outlives the instance.
unsafe fn create_plugin<P: ClapExport>(host: *const sys::ClapHost) -> *const sys::ClapPlugin {
    // Telemetry exists before the plugin so it can be attached right
    // after construction; the wrapper's I/O taps read the floor until
    // audio runs, so a face's meter rails draw from the first frame.
    let telemetry = Telemetry::new();
    let taps = WrapperTaps::new(&telemetry);
    let floor = [WrapperTaps::FLOOR_DB; 3];
    taps.publish_peaks(floor, floor);

    let mut plugin_obj = P::new();
    plugin_obj.attach_telemetry(telemetry.clone());
    let param_table = plugin_obj.param_table();
    if let Err(e) = param_table.validate() {
        Diagnostics::new(&P::DESCRIPTOR).log.write(
            stave_core::diagnostics::Severity::PluginMisbehaving,
            &format_args!("param table invalid: {e}"),
        );
        debug_assert!(false, "param table invalid: {e}");
        return ptr::null();
    }

    let (face, presets) = (plugin_obj.face(), plugin_obj.presets());
    let instance = Box::new(Instance::<P> {
        clap_plugin: sys::ClapPlugin {
            desc: P::clap_descriptor(),
            plugin_data: ptr::null_mut(),
            init: Some(plugin_init::<P>),
            destroy: Some(plugin_destroy::<P>),
            activate: Some(plugin_activate::<P>),
            deactivate: Some(plugin_deactivate::<P>),
            start_processing: Some(plugin_start_processing),
            stop_processing: Some(plugin_stop_processing::<P>),
            reset: Some(plugin_reset::<P>),
            process: Some(plugin_process::<P>),
            get_extension: Some(plugin_get_extension::<P>),
            on_main_thread: Some(plugin_on_main_thread::<P>),
        },
        audio_ports_vtable: sys::ClapPluginAudioPorts {
            count: Some(ports::audio_ports_count::<P>),
            get: Some(ports::audio_ports_get::<P>),
        },
        audio_ports_config_vtable: sys::ClapPluginAudioPortsConfig {
            count: Some(ports::config_count::<P>),
            get: Some(ports::config_get::<P>),
            select: Some(ports::config_select::<P>),
        },
        config: core::sync::atomic::AtomicUsize::new(0),
        params_vtable: sys::ClapPluginParams {
            count: Some(params_count::<P>),
            get_info: Some(params_get_info::<P>),
            get_value: Some(params_get_value::<P>),
            value_to_text: Some(params_value_to_text::<P>),
            text_to_value: Some(params_text_to_value::<P>),
            flush: Some(params_flush::<P>),
        },
        state_vtable: sys::ClapPluginState {
            save: Some(state_save::<P>),
            load: Some(state_load::<P>),
        },
        tail_vtable: sys::ClapPluginTail {
            get: Some(tail_get::<P>),
        },
        latency_vtable: sys::ClapPluginLatency {
            get: Some(latency_get::<P>),
        },
        bypass_id: (0..param_table.count())
            .filter_map(|i| param_table.by_index(i))
            .find(|p| p.meta().flags.bypass)
            .map(|p| p.meta().id.0),
        tail: core::sync::atomic::AtomicU32::new(0),
        shared: SharedState::new(44_100.0),
        plugin: PluginCell::new(plugin_obj),
        param_table,
        state_store: std::sync::Mutex::new(StateStore::default()),
        gestures: Arc::new(GestureQueue::new()),
        #[cfg(target_os = "macos")]
        gui_vtable: gui_vtable::<P>(),
        note_ports_vtable: sys::ClapPluginNotePorts {
            count: Some(ports::note_ports_count::<P>),
            get: Some(ports::note_ports_get::<P>),
        },
        face,
        presets,
        #[cfg(target_os = "macos")]
        editor: core::cell::RefCell::new(None),
        telemetry,
        taps,
        // SAFETY: clap.h: the host outlives the instances it creates.
        host: unsafe { Host::new(host) },
        diagnostics: Diagnostics::new(&P::DESCRIPTOR),
        guard: PanicGuard::default(),
    });

    // plugin_data points back at the whole Instance so the vtable funcs can
    // recover &Instance — though we actually use the `clap_plugin` ptr
    // identity since Instance is repr(C) with clap_plugin first.
    let raw = Box::into_raw(instance);
    // SAFETY: `raw` is the live, unshared allocation just made.
    unsafe {
        (*raw).clap_plugin.plugin_data = raw as *mut c_void;
    }
    // Cast the WHOLE-allocation pointer (same address — clap_plugin is
    // the first repr(C) field). Returning `&(*raw).clap_plugin as *const _`
    // would narrow provenance to the first field and make every later
    // `&Instance` reconstruction UB (caught by Miri on the ABI test).
    raw.cast::<sys::ClapPlugin>()
}
