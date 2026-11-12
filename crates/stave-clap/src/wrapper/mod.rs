//! Generic adapter from a plugin (`stave_core::runtime::Erased`) to the
//! CLAP C ABI. Everything per plugin type — descriptor, factory, entry —
//! is a promoted constant of a generic function, so the export macro
//! emits only the `clap_entry` symbol.

use core::ffi::{c_char, c_void, CStr};
use core::ptr;

use std::sync::Arc;

use stave_core::diagnostics::{CallError, Diagnostics, ProcessFault};
use stave_core::event::NoteKind;
use stave_core::params::Params;
use stave_core::runtime::state::{self as wrapper_state, MAX_STATE_BYTES};
use stave_core::runtime::{
    Erased, GestureEvent, GestureQueue, PanicGuard, PluginCell, SharedState, WrapperTaps,
};
use stave_core::state::StateStore;
use stave_core::{
    Block, Event, EventKind, Feature, GestureSink, InputBuffer, NoteEvent, OutputBuffer, ParamId,
    PluginController, Preset, Telemetry, Transport,
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
pub(crate) fn ffi_entry<P: Erased, R>(failed: impl FnOnce() -> R, call: impl FnOnce() -> R) -> R {
    stave_core::diagnostics::log_panics(const { &P::INFO });
    // AssertUnwindSafe: on a panic, whatever `call` built is dropped
    // during the unwind and `failed()` touches none of it.
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(call)).unwrap_or_else(|_| failed())
}

/// Scratch must fit the WIDEST declared member — hosts can
/// renegotiate configs between activations.
fn max_in_channels<P: Erased>() -> usize {
    P::LAYOUTS
        .iter()
        .map(|c| c.input_channels())
        .max()
        .unwrap_or(0)
}

/// What the CLAP wrapper needs beyond [`Erased`]: the version string and
/// the face. `stave::export!` implements it.
pub trait ClapExport: Erased {
    /// The version hosts show: `INFO.version`, or the exporting crate's.
    const VERSION: &'static str = Self::INFO.version;

    /// The plugin's face, if it has one.
    fn face(&self) -> Option<stave_face::Face> {
        None
    }
}

/// `text` as a nul-terminated C string in an `N`-byte buffer, at compile
/// time: a string that does not fit, or holds a nul, fails the build.
#[repr(transparent)]
pub struct CText<const N: usize>([u8; N]);

impl<const N: usize> CText<N> {
    pub const fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        assert!(bytes.len() < N, "Info text too long for the CLAP descriptor");
        let mut out = [0u8; N];
        let mut i = 0;
        while i < bytes.len() {
            assert!(bytes[i] != 0, "Info text holds a nul byte");
            out[i] = bytes[i];
            i += 1;
        }
        Self(out)
    }

    pub const fn as_ptr(&self) -> *const c_char {
        self.0.as_ptr().cast()
    }
}

/// `P`'s CLAP descriptor: a promoted constant, one per type, its strings
/// and features compiled in.
pub fn descriptor<P: ClapExport>() -> &'static sys::ClapPluginDescriptor {
    const {
        &sys::ClapPluginDescriptor {
            clap_version: sys::CLAP_VERSION,
            id: const { &CText::<256>::new(P::INFO.id) }.as_ptr(),
            name: const { &CText::<256>::new(P::INFO.name) }.as_ptr(),
            vendor: const { &CText::<256>::new(P::INFO.vendor) }.as_ptr(),
            url: const { &CText::<512>::new(P::INFO.url) }.as_ptr(),
            manual_url: ptr::null(),
            support_url: ptr::null(),
            version: const { &CText::<64>::new(P::VERSION) }.as_ptr(),
            description: const { &CText::<1024>::new(P::INFO.description) }.as_ptr(),
            features: const { &FeaturesArray::<17>::new(P::INFO.features) }.as_ptr(),
        }
    }
}

/// `P`'s plugin factory, a promoted constant.
pub fn factory<P: ClapExport>() -> &'static sys::ClapPluginFactory {
    const {
        &sys::ClapPluginFactory {
            get_plugin_count: Some(factory_get_plugin_count::<P>),
            get_plugin_descriptor: Some(factory_get_plugin_descriptor::<P>),
            create_plugin: Some(factory_create_plugin::<P>),
        }
    }
}

/// The `clap_entry` value for `P`.
pub const fn entry<P: ClapExport>() -> sys::ClapPluginEntry {
    sys::ClapPluginEntry {
        clap_version: sys::CLAP_VERSION,
        init: Some(entry_init),
        deinit: Some(entry_deinit),
        get_factory: Some(entry_get_factory::<P>),
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
    pub const fn new(features: &[Feature]) -> Self {
        let mut out = [ptr::null::<c_char>(); N];
        assert!(features.len() < N, "too many features for the CLAP descriptor");
        let mut i = 0;
        while i < features.len() {
            out[i] = features[i].clap_name().as_ptr();
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
mod gui;
mod host;
mod params;
mod plugin;
mod process;
mod state_ext;

pub use entry::*;
pub(crate) use gesture::*;
pub use gesture::controller;
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
    /// Index into LAYOUTS of the host-selected layout
    /// audio-ports-config select() picks a declared member;
    /// port queries and the process aux path serve THAT member.
    pub(crate) config: core::sync::atomic::AtomicUsize,
    params_vtable: sys::ClapPluginParams,
    state_vtable: sys::ClapPluginState,
    shared: SharedState,
    plugin: PluginCell<P>,
    /// Typed parameter table; host params served from here
    /// with stable ids. CLAP wire values are PLAIN, matching the store.
    params: Arc<dyn Params>,
    /// Unknown-content retention across state cycles.
    state_store: std::sync::Mutex<StateStore>,
    /// UI→host gesture events, drained into `out_events`
    /// during process/flush so the DAW records and undoes GUI edits;
    /// shared with every face's gesture sink.
    gestures: Arc<GestureQueue>,
    gui_vtable: sys::ClapPluginGui,
    /// X11: the run-loop callbacks the view's descriptor and timer use.
    #[cfg(all(unix, not(target_os = "macos")))]
    timer_vtable: sys::ClapPluginTimerSupport,
    #[cfg(all(unix, not(target_os = "macos")))]
    fd_vtable: sys::ClapPluginPosixFdSupport,
    /// X11: the timer and descriptor registered with the host while the
    /// view is attached. [main-thread]
    #[cfg(all(unix, not(target_os = "macos")))]
    run_loop: core::cell::Cell<Option<(sys::ClapId, Option<core::ffi::c_int>)>>,
    note_ports_vtable: sys::ClapPluginNotePorts,
    /// The plugin's face; None = no `clap.gui`.
    face: Option<stave_face::Face>,
    /// Factory presets (one list, shared with every face).
    presets: Arc<[Preset]>,
    /// The open face, between `gui.create` and `gui.destroy`. Only the
    /// [main-thread] gui calls touch it (clap gui.h).
    editor: core::cell::RefCell<Option<FaceView>>,
    /// The instance's telemetry: the plugin's taps, and the wrapper's
    /// I/O peaks, CPU load and capture for its face.
    telemetry: Arc<Telemetry>,
    taps: WrapperTaps,
    tail_vtable: sys::ClapPluginTail,
    latency_vtable: sys::ClapPluginLatency,
    /// Id of the param flagged `bypass`, if any.
    bypass_id: Option<ParamId>,
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
