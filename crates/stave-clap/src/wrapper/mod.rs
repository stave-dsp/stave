//! Generic adapter from `stave_core::Plugin` to the CLAP C ABI.
//!
//! Statics that need to be per-plugin-type live in the user's crate (emitted
//! by `stave_clap_export!`). They reach back into this module via the
//! `ClapExport` trait so that generic functions here can locate them.

use core::ffi::{c_char, c_void, CStr};
use core::ptr;

extern crate alloc;
use alloc::sync::Arc;

use stave_core::diagnostics::{CallError, Diagnostics, ProcessFault};
use stave_core::event::NoteKind;
use stave_core::params::Params;
use stave_core::state::StateStore;
use stave_core::wrapper::state::{self as wrapper_state, MAX_STATE_BYTES};
use stave_core::wrapper::{PanicGuard, WrapperTaps};
use stave_core::{
    AudioBuffer, Event, EventKind, GestureEvent, GestureQueue, GestureSink, NoteEvent, ParamId,
    Plugin, PluginCell, PluginController, PluginFeature, Preset, ProcessCtx, SharedState,
    Telemetry, Transport,
};

use crate::ports;
use crate::sys;

/// The panic boundary for `extern "C"` entries: `call()`, or `failed()`
/// if it panics — an unwind never reaches the host. Entries that run
/// plugin code on an instance go through [`Instance::guarded`] (the
/// same boundary plus the instance's disable-on-panic latch); this is
/// for the rest (entry, factory, instance construction, wrapper-only
/// bookkeeping). The panic itself is logged by the `log_panics` hook
/// installed here.
///
/// Entries whose whole body is a constant or a single atomic load
/// cannot panic and are not wrapped.
pub(crate) fn ffi_entry<P: Plugin, R>(failed: impl FnOnce() -> R, call: impl FnOnce() -> R) -> R {
    stave_core::diagnostics::log_panics(const { &P::DESCRIPTOR });
    // AssertUnwindSafe: on a panic, whatever `call` built is dropped
    // during the unwind and `failed()` touches none of it.
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(call)).unwrap_or_else(|_| failed())
}

/// Scratch must fit the WIDEST declared member — hosts can
/// renegotiate configs between activations.
fn max_in_channels<P: Plugin>() -> usize {
    P::CONFIGS
        .iter()
        .map(|c| c.in_channels() as usize)
        .max()
        .unwrap_or(0)
}

/// Per-plugin-type glue. Implemented by the `stave_clap_export!` macro for
/// the user's plugin so generic CLAP functions can reach the type's statics.
pub trait ClapExport: Plugin + stave_face::HasFace {
    fn clap_descriptor() -> &'static sys::ClapPluginDescriptor;
    fn clap_factory() -> &'static sys::ClapPluginFactory;
}

/// Macro-implemented on the project's ENGINE type (local — orphan
/// rule); the shell picks it up via the blanket impl below.
pub trait ClapEntry {
    fn clap_descriptor() -> &'static sys::ClapPluginDescriptor;
    fn clap_factory() -> &'static sys::ClapPluginFactory;
}

impl<E> ClapExport for stave_core::StavePlugin<E>
where
    E: stave_face::FaceSpec + ClapEntry,
{
    fn clap_descriptor() -> &'static sys::ClapPluginDescriptor {
        E::clap_descriptor()
    }
    fn clap_factory() -> &'static sys::ClapPluginFactory {
        E::clap_factory()
    }
}

/// NULL-terminated `[*const c_char; N]` array, wrapped so it can live in a
/// `static` (raw pointers aren't `Sync` by default — but our pointers all
/// point into `'static` C-strings, which is fine to share).
#[repr(transparent)]
pub struct FeaturesArray<const N: usize>([*const c_char; N]);

// SAFETY: contents are 'static C-string pointers; the array is read-only
// after const initialization. No data race possible.
unsafe impl<const N: usize> Sync for FeaturesArray<N> {}

impl<const N: usize> FeaturesArray<N> {
    /// Build the array at const-eval time. Asserts there's room for at least
    /// `features.len()` entries plus the trailing NULL.
    pub const fn new(features: &[PluginFeature]) -> Self {
        let mut out = [ptr::null::<c_char>(); N];
        assert!(features.len() < N, "feature count exceeds slot capacity");
        let mut i = 0;
        while i < features.len() {
            out[i] = features[i].as_clap_cstr().as_ptr();
            i += 1;
        }
        Self(out)
    }

    pub const fn as_ptr(&self) -> *const *const c_char {
        self.0.as_ptr()
    }
}

mod entry;
mod gesture;
#[cfg(target_os = "macos")]
mod gui;
mod host;
mod params;
mod plugin;
mod process;
mod state_ext;

pub use entry::*;
pub(crate) use gesture::*;
pub use gesture::controller;
#[cfg(target_os = "macos")]
use gui::*;
pub(crate) use host::{in_text, out_param};
use host::{
    out_text, CoreEvent, Host, InputEvents, InputStream, OutputEvents, OutputStream, Process,
};
use params::*;
use plugin::*;
use process::*;
use state_ext::*;

// ===========================================================================
// Plugin instance
// ===========================================================================

/// Heap-allocated per-instance state. Placed first so the host's
/// `*const ClapPlugin` pointer also addresses the start of our `Instance`.
///
/// Thread contract mirrors stave-vst3: entry points hold only
/// `&Instance`; cross-thread fields are atomics in `shared`; the plugin
/// is claimed exclusively per call through `PluginCell`.
#[repr(C)]
pub(crate) struct Instance<P: ClapExport> {
    /// MUST be the first field — see `from_plugin_ptr`.
    clap_plugin: sys::ClapPlugin,
    audio_ports_vtable: sys::ClapPluginAudioPorts,
    audio_ports_config_vtable: sys::ClapPluginAudioPortsConfig,
    /// Index into CONFIGS of the host-selected port configuration
    /// audio-ports-config select() picks a declared member;
    /// port queries and the process aux path serve THAT member.
    pub(crate) config: core::sync::atomic::AtomicUsize,
    params_vtable: sys::ClapPluginParams,
    state_vtable: sys::ClapPluginState,
    shared: SharedState,
    plugin: PluginCell<P>,
    /// Typed parameter table; host params served from here
    /// with stable ids. CLAP wire values are PLAIN, matching the store.
    param_table: Arc<dyn Params>,
    /// Unknown-content retention across state cycles.
    state_store: std::sync::Mutex<StateStore>,
    /// UI→host gesture events, drained into `out_events`
    /// during process/flush so the DAW records and undoes GUI edits;
    /// shared with every face's gesture sink.
    gestures: Arc<GestureQueue>,
    #[cfg(target_os = "macos")]
    gui_vtable: sys::ClapPluginGui,
    note_ports_vtable: sys::ClapPluginNotePorts,
    /// The plugin's face; None = no `clap.gui`.
    face: Option<stave_face::Face>,
    /// Factory presets, for the face's preset readout.
    presets: Vec<Preset>,
    /// The open face, between `gui.create` and `gui.destroy`. Only the
    /// [main-thread] gui calls touch it (clap gui.h).
    #[cfg(target_os = "macos")]
    editor: core::cell::RefCell<Option<stave_ui_cocoa::FaceView>>,
    /// The instance's telemetry: the plugin's taps, and the wrapper's
    /// I/O peaks, CPU load and capture for its face.
    telemetry: Arc<Telemetry>,
    taps: WrapperTaps,
    tail_vtable: sys::ClapPluginTail,
    latency_vtable: sys::ClapPluginLatency,
    /// Id of the param flagged `bypass`, if any.
    bypass_id: Option<u32>,
    tail: core::sync::atomic::AtomicU32,
    /// The creating host, for `request_callback` and `clap.log`.
    host: Option<Host>,
    diagnostics: Diagnostics,
    guard: PanicGuard,
}

impl<P: ClapExport> Instance<P> {
    /// Recover `&Instance` from the opaque plugin pointer the host hands
    /// back to us. Never `&mut` — exclusive access goes through
    /// `self.plugin.claim()`.
    ///
    /// # Safety
    /// `plugin` is a pointer `create_plugin` returned for this `P`, not
    /// yet destroyed.
    pub(crate) unsafe fn from_plugin_ptr<'a>(plugin: *const sys::ClapPlugin) -> &'a Instance<P> {
        debug_assert!(!plugin.is_null());
        // SAFETY: `create_plugin` returns the whole `Instance` allocation
        // cast to its first field (`repr(C)`), and it lives until
        // `destroy`.
        unsafe { &*(plugin as *const Instance<P>) }
    }

    /// `false` for a refused call, after logging why.
    /// `call()` inside the instance's panic guard; `failed()` once the
    /// instance is disabled.
    pub(crate) fn guarded<R>(
        &self,
        name: &str,
        failed: impl FnOnce() -> R,
        call: impl FnOnce() -> R,
    ) -> R {
        self.guard.run(&self.diagnostics, name, failed, call)
    }

    /// A host call answered `true`/`false`: guarded, refusals logged.
    pub(crate) fn call(&self, name: &str, call: impl FnOnce() -> Result<(), CallError>) -> bool {
        self.guarded(name, || false, || self.answer(name, call()))
    }

    pub(crate) fn answer(&self, call: &str, result: Result<(), CallError>) -> bool {
        result.inspect_err(|error| self.report(call, error)).is_ok()
    }

    fn report(&self, call: &str, error: &CallError) {
        self.diagnostics.report(call, error);
    }

    /// Realtime-safe. The first fault since the last report asks the
    /// host for `on_main_thread`, which reports it.
    fn fault(&self, fault: ProcessFault, count: u32) {
        if let (true, Some(host)) = (self.diagnostics.faults.record(fault, count), self.host) {
            host.request_callback();
        }
    }
}
