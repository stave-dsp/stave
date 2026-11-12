//! The `clap_plugin` vtable: lifecycle, activation and extension lookup.

use super::*;

pub(super) unsafe extern "C" fn plugin_init<P: ClapExport>(plugin: *const sys::ClapPlugin) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    ffi_entry::<P, _>(
        || false,
        || {
            if let Some(log) = inst.host.and_then(Host::log) {
                inst.diagnostics.log.add(Box::new(log));
            }
            true
        },
    )
}

pub(super) unsafe extern "C" fn plugin_destroy<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // A panic before the free leaks the instance instead of unwinding
    // into the host.
    // SAFETY: host contract (see `host`): `destroy` is the last call.
    ffi_entry::<P, _>(|| (), || unsafe { destroy::<P>(plugin) });
}

/// # Safety
/// `plugin` is a live pointer `create_plugin` returned; nothing uses it
/// after this call.
unsafe fn destroy<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guard.report(&inst.diagnostics);
    inst.diagnostics.report_faults();
    // SAFETY: `create_plugin` made this pointer with `Box::into_raw` of
    // the whole `Instance`; `destroy` is the host's last call on it.
    let instance = unsafe { Box::from_raw(plugin as *mut Instance<P>) };
    // A destructor that panics (logged by the hook) must not unwind into
    // the host.
    let _ = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| drop(instance)));
}

pub(super) unsafe extern "C" fn plugin_activate<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guarded(
        "activate",
        || false,
        || {
            let Some(mut proc) = inst.plugin.claim() else {
                return inst.answer("activate", Err(CallError::ProcessorBusy));
            };
            inst.shared.set_sample_rate(sample_rate as f32);
            inst.shared.set_max_frames(max_frames);
            let env = stave_core::Environment::new(sample_rate as f32, max_frames as usize)
                .with_layout(crate::ports::selected(inst));
            let latency = proc.prepare(&env, max_in_channels::<P>());
            inst.diagnostics.event(stave_core::telemetry::Event::Prepare {
                sample_rate: sample_rate as f32,
                max_block: max_frames,
            });
            inst.shared.set_latency(latency);
            inst.tail.store(
                stave_core::runtime::tail_wire(proc.plugin.tail()),
                core::sync::atomic::Ordering::Release,
            );
            inst.shared.set_activated(true);
            true
        },
    )
}

pub(super) unsafe extern "C" fn latency_get<P: ClapExport>(plugin: *const sys::ClapPlugin) -> u32 {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.shared.latency()
}

pub(super) unsafe extern "C" fn tail_get<P: ClapExport>(plugin: *const sys::ClapPlugin) -> u32 {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.tail.load(core::sync::atomic::Ordering::Acquire)
}

pub(super) unsafe extern "C" fn plugin_deactivate<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    ffi_entry::<P, _>(|| (), || inst.diagnostics.report_faults());
    inst.guarded(
        "deactivate",
        || (),
        || {
            let Some(mut proc) = inst.plugin.claim() else {
                return inst.report("deactivate", &CallError::ProcessorBusy);
            };
            proc.release();
            inst.diagnostics.event(stave_core::telemetry::Event::Release);
            inst.shared.set_activated(false);
        },
    );
}

pub(super) unsafe extern "C" fn plugin_start_processing(_plugin: *const sys::ClapPlugin) -> bool {
    true
}

pub(super) unsafe extern "C" fn plugin_stop_processing<P: ClapExport>(
    _plugin: *const sys::ClapPlugin,
) {
}

pub(super) unsafe extern "C" fn plugin_reset<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guarded(
        "reset",
        || (),
        || match inst.plugin.claim() {
            Some(mut proc) => proc.plugin.reset(),
            None => inst.report("reset", &CallError::ProcessorBusy),
        },
    );
}

pub(super) unsafe extern "C" fn plugin_on_main_thread<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
) {
    // SAFETY: host contract (see `host`).
    ffi_entry::<P, _>(|| (), || unsafe { on_main_thread::<P>(plugin) });
}

/// # Safety
/// `plugin` is a live pointer `create_plugin` returned.
unsafe fn on_main_thread<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guard.report(&inst.diagnostics);
    inst.diagnostics.report_faults();
}

pub(super) unsafe extern "C" fn plugin_get_extension<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    id: *const c_char,
) -> *const c_void {
    // SAFETY: host contract (see `host`).
    let (inst, requested) = unsafe { (Instance::<P>::from_plugin_ptr(plugin), in_text(id, "id")) };
    let Ok(requested) = requested else {
        return ptr::null();
    };
    ffi_entry::<P, _>(ptr::null, || extension(inst, requested))
}

fn extension<P: ClapExport>(inst: &Instance<P>, requested: &CStr) -> *const c_void {
    if requested == sys::CLAP_EXT_AUDIO_PORTS {
        return &inst.audio_ports_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_AUDIO_PORTS_CONFIG {
        return &inst.audio_ports_config_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_PARAMS {
        return &inst.params_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_STATE {
        return &inst.state_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_TAIL {
        return &inst.tail_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_LATENCY {
        return &inst.latency_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_GUI && inst.face.is_some() {
        return &inst.gui_vtable as *const _ as *const c_void;
    }
    // An X11 face pumps from the host's run loop.
    #[cfg(all(unix, not(target_os = "macos")))]
    if requested == sys::CLAP_EXT_TIMER_SUPPORT && inst.face.is_some() {
        return &inst.timer_vtable as *const _ as *const c_void;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if requested == sys::CLAP_EXT_POSIX_FD_SUPPORT && inst.face.is_some() {
        return &inst.fd_vtable as *const _ as *const c_void;
    }
    if requested == sys::CLAP_EXT_NOTE_PORTS && ports::takes_notes::<P>() {
        return &inst.note_ports_vtable as *const _ as *const c_void;
    }
    ptr::null()
}
