//! Generic adapter from a plugin (`stave_core::runtime::Erased`) to the
//! Audio Unit v2 plug-in interface. The factory function returns an
//! `Instance` whose first field is the `AudioComponentPlugInInterface`;
//! AudioToolbox asks its `Lookup` for each selector's function and calls
//! it with the interface pointer, from which every entry recovers
//! `&Instance`.
//!
//! Thread contract, as in stave-clap and stave-vst3: entries hold only
//! `&Instance`; the plugin is claimed exclusively per call through
//! `PluginCell`; values every thread reads are atomics. The host's
//! configuration (formats, callbacks, listeners) sits behind mutexes the
//! audio thread only ever `try_lock`s: a block that finds one held is
//! dropped and counted, never waited for.
//!
//! ```text
//! mod.rs          the instance, the factory, Open/Close, Lookup
//! formats.rs      buses: formats, channel negotiation, Initialize
//! properties.rs   GetPropertyInfo / GetProperty / SetProperty
//! params.rs       parameters: info, values, strings, notifications
//! process.rs      the audio callback, notes, transport
//! state.rs        ClassInfo: the session's state
//! view.rs         the Cocoa view (the face)
//! cf.rs           CoreFoundation values
//! ```

mod cf;
mod formats;
mod params;
mod process;
mod properties;
mod state;
mod view;

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stave_core::diagnostics::{CallError, Diagnostics};
use stave_core::params::Params;
use stave_core::runtime::{PanicGuard, PluginCell, SharedState, WrapperTaps};
use stave_core::state::StateStore;
use stave_core::{ParamId, Preset, Telemetry};

use crate::component::{self, Identity};
use crate::sys::{self, OSStatus};
use crate::AuExport;

use formats::{Io, Props};
use process::Notes;

/// The panic boundary for `extern "C"` entries that run no plugin code
/// on an instance (the factory, Open, Close): `call()`, or `failed()` if
/// it panics — an unwind never reaches the host.
fn ffi_entry<P: AuExport, R>(failed: impl FnOnce() -> R, call: impl FnOnce() -> R) -> R {
    stave_core::diagnostics::log_panics(const { &P::INFO });
    // AssertUnwindSafe: on a panic, whatever `call` built is dropped
    // during the unwind and `failed()` touches none of it.
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(call)).unwrap_or_else(|_| failed())
}

/// A poisoned lock still guards consistent data here (every critical
/// section is a plain field update), so poison is ignored.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One Audio Unit instance. `repr(C)` with the interface first, so the
/// pointer the factory returns addresses the whole allocation.
#[repr(C)]
pub(crate) struct Instance<P: AuExport> {
    /// MUST be the first field — see `from_this`.
    interface: sys::AudioComponentPlugInInterface,
    /// The `AudioComponentInstance` AudioToolbox gave `Open`: what
    /// notifications name. Shared with the face's gesture sink, which
    /// may outlive the instance (`Close` clears it).
    unit: Arc<AtomicPtr<c_void>>,
    identity: Identity,
    shared: SharedState,
    plugin: PluginCell<P>,
    params: Arc<dyn Params>,
    state_store: Mutex<StateStore>,
    telemetry: Arc<Telemetry>,
    taps: WrapperTaps,
    face: Option<stave_face::Face>,
    presets: Arc<[Preset]>,
    /// The factory presets as `AUPreset`s, their names owned here.
    factory: Box<[sys::AUPreset]>,
    _factory_names: Vec<cf::Cf>,
    /// The present preset: its number (−1: none, or the host's own) and
    /// name.
    present: Mutex<(i32, String)>,
    bypass_id: Option<ParamId>,
    /// `kAudioUnitProperty_BypassEffect`.
    effect_bypass: AtomicBool,
    tail: AtomicU32,
    last_error: AtomicI32,
    /// The host's configuration, [any thread but audio].
    props: Mutex<Props>,
    /// What the audio callback reads and writes: sources, scratch.
    io: Mutex<Io>,
    /// Notes `MusicDeviceMIDIEvent` queued for the next block.
    notes: Mutex<Notes>,
    diagnostics: Diagnostics,
    guard: PanicGuard,
}

impl<P: AuExport> Instance<P> {
    /// Recover `&Instance` from the interface pointer AudioToolbox passes
    /// every entry. Never `&mut`.
    ///
    /// # Safety
    /// `this` is a pointer `factory` returned for this `P`, not yet
    /// closed.
    pub(crate) unsafe fn from_this<'a>(this: *mut c_void) -> &'a Instance<P> {
        debug_assert!(!this.is_null());
        // SAFETY: `factory` returns the whole `Instance` allocation cast
        // to its first field (`repr(C)`); it lives until `Close`.
        unsafe { &*(this as *const Instance<P>) }
    }

    /// `call()` inside the instance's panic guard; `failed()` once the
    /// instance is disabled.
    fn guarded(&self, name: &str, failed: impl FnOnce() -> OSStatus, call: impl FnOnce() -> OSStatus) -> OSStatus {
        self.guard.run(&self.diagnostics, name, failed, call)
    }

    fn report(&self, call: &str, error: &CallError) {
        self.diagnostics.report(call, error);
    }

    fn unit(&self) -> sys::AudioUnit {
        self.unit.load(Ordering::Acquire)
    }

    /// Tell property listeners `id` changed. [not audio]
    fn changed(&self, id: sys::AudioUnitPropertyID, scope: sys::AudioUnitScope, element: sys::AudioUnitElement) {
        let listeners: Vec<formats::Listener> =
            lock(&self.props).listeners.iter().filter(|l| l.id == id).copied().collect();
        let unit = self.unit();
        for l in listeners {
            // SAFETY: a listener the host registered for this unit and
            // has not removed; called with the arguments its type names.
            unsafe { (l.proc_)(l.refcon, unit, id, scope, element) };
        }
    }

    fn takes_notes() -> bool {
        component::takes_notes(Identity::of::<P>().kind)
    }

    fn is_effect() -> bool {
        !matches!(&Identity::of::<P>().kind, b"aumu")
    }
}

/// Allocate an instance of `P` for AudioToolbox (`export_au!`'s factory
/// function). The description is not read: the bundle registers one
/// component per binary.
pub fn factory<P: AuExport>(_desc: *const sys::AudioComponentDescription) -> *mut sys::AudioComponentPlugInInterface {
    ffi_entry::<P, _>(ptr::null_mut, create::<P>)
}

fn create<P: AuExport>() -> *mut sys::AudioComponentPlugInInterface {
    // Telemetry exists before the plugin so it can attach its taps; the
    // wrapper's I/O taps read the floor until audio runs.
    let telemetry = Telemetry::new();
    let taps = WrapperTaps::new(&telemetry);
    let floor = [WrapperTaps::FLOOR_DB; 3];
    taps.publish_peaks(floor, floor);

    let plugin = P::new(&telemetry);
    let params = plugin.params();
    let diagnostics = Diagnostics::new(&P::INFO);
    if let Err(e) = params.validate() {
        diagnostics.log.write(
            stave_core::diagnostics::Severity::PluginMisbehaving,
            &format_args!("param table invalid: {e}"),
        );
        debug_assert!(false, "param table invalid: {e}");
        return ptr::null_mut();
    }
    diagnostics.watch(&telemetry);
    let (face, presets) = (plugin.face(), plugin.presets());
    let factory_names: Vec<cf::Cf> = presets.iter().map(|p| cf::string(p.name)).collect();
    let factory = factory_names
        .iter()
        .enumerate()
        .map(|(i, n)| sys::AUPreset { presetNumber: i as i32, presetName: n.get() })
        .collect();
    let bypass_id = (0..params.count())
        .filter_map(|i| params.by_index(i))
        .find(|p| p.meta().flags.bypass)
        .map(|p| p.meta().id);
    let instance = Box::new(Instance::<P> {
        interface: sys::AudioComponentPlugInInterface {
            Open: open::<P>,
            Close: close::<P>,
            Lookup: lookup::<P>,
            reserved: ptr::null_mut(),
        },
        unit: Arc::new(AtomicPtr::new(ptr::null_mut())),
        identity: Identity::of::<P>(),
        shared: SharedState::new(formats::DEFAULT_RATE as f32),
        plugin: PluginCell::new(plugin),
        params,
        state_store: Mutex::new(StateStore::default()),
        telemetry,
        taps,
        face,
        presets,
        factory,
        _factory_names: factory_names,
        present: Mutex::new((-1, state::UNTITLED.to_string())),
        bypass_id,
        effect_bypass: AtomicBool::new(false),
        tail: AtomicU32::new(0),
        last_error: AtomicI32::new(0),
        props: Mutex::new(Props::new::<P>()),
        io: Mutex::new(Io::new::<P>()),
        notes: Mutex::new(Notes::default()),
        diagnostics,
        guard: PanicGuard::default(),
    });
    // The whole allocation's pointer (same address: the interface is the
    // first repr(C) field), so every `&Instance` made from it keeps the
    // provenance of the whole.
    Box::into_raw(instance).cast()
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
unsafe extern "C" fn open<P: AuExport>(this: *mut c_void, unit: sys::AudioUnit) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    inst.unit.store(unit, Ordering::Release);
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; nothing uses it
/// after this call.
unsafe extern "C" fn close<P: AuExport>(this: *mut c_void) -> OSStatus {
    ffi_entry::<P, _>(
        || sys::noErr,
        || {
            // SAFETY: this function's contract.
            let inst = unsafe { Instance::<P>::from_this(this) };
            inst.unit.store(ptr::null_mut(), Ordering::Release);
            inst.guard.report(&inst.diagnostics);
            inst.diagnostics.report_faults();
            // SAFETY: `factory` made this pointer with `Box::into_raw`
            // of the whole `Instance`; `Close` is the last call on it.
            let instance = unsafe { Box::from_raw(this as *mut Instance<P>) };
            // A destructor that panics (logged by the hook) must not
            // unwind into the host.
            let _ = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| drop(instance)));
            sys::noErr
        },
    )
}

/// The function serving `selector`, or null (AudioToolbox answers the
/// host `badComponentSelector`).
extern "C" fn lookup<P: AuExport>(selector: i16) -> *const c_void {
    match selector {
        sys::kAudioUnitInitializeSelect => formats::initialize::<P> as *const c_void,
        sys::kAudioUnitUninitializeSelect => formats::uninitialize::<P> as *const c_void,
        sys::kAudioUnitGetPropertyInfoSelect => properties::get_property_info::<P> as *const c_void,
        sys::kAudioUnitGetPropertySelect => properties::get_property::<P> as *const c_void,
        sys::kAudioUnitSetPropertySelect => properties::set_property::<P> as *const c_void,
        sys::kAudioUnitAddPropertyListenerSelect => properties::add_listener::<P> as *const c_void,
        sys::kAudioUnitRemovePropertyListenerSelect => properties::remove_listener::<P> as *const c_void,
        sys::kAudioUnitRemovePropertyListenerWithUserDataSelect => {
            properties::remove_listener_with_user_data::<P> as *const c_void
        }
        sys::kAudioUnitAddRenderNotifySelect => process::add_notify::<P> as *const c_void,
        sys::kAudioUnitRemoveRenderNotifySelect => process::remove_notify::<P> as *const c_void,
        sys::kAudioUnitGetParameterSelect => params::get_parameter::<P> as *const c_void,
        sys::kAudioUnitSetParameterSelect => params::set_parameter::<P> as *const c_void,
        sys::kAudioUnitScheduleParametersSelect => params::schedule_parameters::<P> as *const c_void,
        sys::kAudioUnitRenderSelect => process::process::<P> as *const c_void,
        sys::kAudioUnitResetSelect => process::reset::<P> as *const c_void,
        sys::kMusicDeviceMIDIEventSelect if Instance::<P>::takes_notes() => process::midi_event::<P> as *const c_void,
        sys::kMusicDeviceSysExSelect if Instance::<P>::takes_notes() => process::sysex as *const c_void,
        _ => ptr::null(),
    }
}
