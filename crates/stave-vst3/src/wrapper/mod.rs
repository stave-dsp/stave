//! Adapts a `stave_core::Plugin` to a VST3 component exposing three
//! interfaces (`IComponent`, `IAudioProcessor`, `IEditController`) on one
//! object. Implements the VST3 multi-interface convention by giving each
//! interface its own sub-struct (each starts with its own vtable pointer)
//! that share a common backing `Instance`.

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

extern crate alloc;
use alloc::sync::Arc;

use stave_core::diagnostics::{BusSink, CallError, Diagnostics, ProcessFault, Severity};
use stave_core::params::Params;
use stave_core::state::StateStore;
use stave_core::wrapper::state::{self as wrapper_state, MAX_STATE_BYTES};
use stave_core::wrapper::PanicGuard;
use stave_core::{
    ranges_alias, AudioBuffer, DebugBus, Event as StaveEvent, EventKind, Plugin, PluginCell,
    ProcessCtx, SharedState,
};

use crate::buses;
use crate::editor_bridge::Vst3EditorHost;
pub use crate::editor_bridge::{gesture_begin, gesture_end, gesture_perform};
use crate::events;
use crate::host::{out_param, string128, tchar_str, ComponentHandler, Process, Stream};
pub(crate) use crate::host_context::{build_pcr_vtbl, translate_transport};
use crate::sys::*;
pub(crate) use crate::wire_strings::{
    cstr_to_str, write_cstr_into, write_subcategories, write_utf16_name,
};

mod component;
mod controller;
mod factory;
mod processor;
mod unit_info;
mod view;

use component::*;
use controller::*;
pub use factory::{build_factory_vtbl, FactoryObj};
use processor::*;
use unit_info::*;
use view::*;

// ===========================================================================
// Per-plugin-type glue
// ===========================================================================

/// Implemented by the `stave_vst3_export!` macro for the user's plugin type.
/// Lets generic wrapper code reach the per-type statics (factory, class IDs)
/// that have to live in the user's crate (Rust statics can't be generic).
pub trait Vst3Export: Plugin {
    /// 16-byte TUID identifying this plugin's IComponent class. Must be
    /// unique across all VST3 plugins on the user's system.
    const COMPONENT_CID: Tuid;
    /// 16-byte TUID for the IEditController class. Returned by
    /// `IComponent::getControllerClassId`; can equal `COMPONENT_CID` since
    /// we expose component+controller fused on one object.
    const CONTROLLER_CID: Tuid;

    /// Per-type vtable pointers. The macro emits each as a `static` in the
    /// user's crate (Rust statics can't be generic) and returns their
    /// addresses from these methods.
    fn component_vtbl() -> *const IComponentVtbl;
    fn processor_vtbl() -> *const IAudioProcessorVtbl;
    fn controller_vtbl() -> *const IEditControllerVtbl;
}

// ===========================================================================
// Interface objects — one per VST3 interface, all sharing one backing Instance
// ===========================================================================

#[repr(C)]
pub struct ComponentObj<P: Vst3Export> {
    pub vtbl: *const IComponentVtbl,
    pub parent: *mut Instance<P>,
}

#[repr(C)]
pub struct ProcessorObj<P: Vst3Export> {
    pub vtbl: *const IAudioProcessorVtbl,
    pub parent: *mut Instance<P>,
}

#[repr(C)]
pub struct ControllerObj<P: Vst3Export> {
    pub vtbl: *const IEditControllerVtbl,
    pub parent: *mut Instance<P>,
}

#[repr(C)]
pub struct PcrObj<P: Vst3Export> {
    pub vtbl: *const IProcessContextRequirementsVtbl,
    pub parent: *mut Instance<P>,
}

#[repr(C)]
pub struct UnitInfoObj<P: Vst3Export> {
    pub vtbl: *const IUnitInfoVtbl,
    pub parent: *mut Instance<P>,
}

/// Heap-allocated per-plugin-instance state. All three interface sub-objects
/// share this backing store; refcount tracks total host references.
///
/// Thread contract: entry points materialize only `&Instance` from the
/// host's raw pointer — never `&mut`. Cross-thread fields live in
/// `shared` (atomics); the plugin sits in a `PluginCell` claimed
/// exclusively per call (audio thread during `process`, main thread
/// during activate); `editor` is main-thread-only by VST3 convention.
#[repr(C)]
pub struct Instance<P: Vst3Export> {
    pub component: ComponentObj<P>,
    pub processor: ProcessorObj<P>,
    pub controller: ControllerObj<P>,
    pub pcr: PcrObj<P>,
    /// PCR vtable stored by value so the export macro needs no new
    /// per-type static.
    pub pcr_vtbl: IProcessContextRequirementsVtbl,
    pub unit_info: UnitInfoObj<P>,
    /// Stored by value for the same reason as `pcr_vtbl`.
    pub unit_info_vtbl: IUnitInfoVtbl,
    pub ref_count: AtomicU32,
    pub shared: SharedState,
    pub plugin: PluginCell<P>,
    /// Main-thread only (`createView` and view callbacks). UnsafeCell
    /// because we only ever hold `&Instance`; the single-thread protocol
    /// is what makes the interior `&mut` sound.
    pub editor: UnsafeCell<Option<Box<EditorBox>>>,
    /// The typed parameter table, served with STABLE wire ids. Shared
    /// with the plugin via Arc — host threads read it without touching
    /// the claimed processor.
    pub param_table: Arc<dyn Params>,
    /// Factory presets, exposed as the program-change parameter and the
    /// root unit's program list.
    pub(crate) programs: crate::programs::Programs,
    /// Parameter groups as VST3 units (folders).
    pub(crate) units: crate::units::Units,
    /// The project-OWNED editor tree; None = bare dev container.
    pub editor_ui: Option<stave_core::UiNode>,
    /// Unknown-content retention across state load→save cycles
    /// Mutex: state calls are non-realtime
    /// host threads; the audio path never touches this.
    pub state_store: std::sync::Mutex<StateStore>,
    /// The host's IComponentHandler: GUI edits round-trip
    /// through beginEdit/performEdit/endEdit so the DAW records and
    /// undoes them. AddRef'd on store, released on replace/drop.
    pub(crate) handler: ComponentHandler,
    /// Id of the param flagged `bypass`, if any — drives the
    /// framework-owned click-free bypass gate.
    pub bypass_id: Option<u32>,
    /// Tail mirror (like latency): hosts query from any thread.
    pub tail: AtomicU32,
    /// In-process debugger bus. Always allocated; plugins that don't use it
    /// just never call probe/breakpoint macros. The wrapper writes capture
    /// frames here unconditionally so the host can inspect output.
    pub debug_bus: Arc<DebugBus>,
    /// Probe-slot caches for the generic I/O level meters published by the
    /// wrapper (peak in/out per block) so the editor's frame rails are live.
    pub io_in_probe: AtomicU16,
    pub io_out_probe: AtomicU16,
    pub io_in_left_probe: AtomicU16,
    pub io_in_right_probe: AtomicU16,
    pub io_out_left_probe: AtomicU16,
    pub io_out_right_probe: AtomicU16,
    /// CPU probe slot: process-block duration over the block's
    /// realtime budget (0..1+), published every block — the editor's
    /// load meter and the "is something O(n²) in here" glance.
    pub cpu_probe: AtomicU16,
    /// Index into CONFIGS of the host-accepted port configuration
    /// setBusArrangements selects a declared member; every bus
    /// query and the process aux path serve THAT member. Default 0 =
    /// the preferred config.
    pub config: core::sync::atomic::AtomicUsize,
    /// VST3 has no host log: messages go to the plugin's log file, and
    /// audio-thread faults are reported from the next lifecycle or
    /// state call.
    pub diagnostics: Diagnostics,
    pub(crate) guard: PanicGuard,
}

impl<P: Vst3Export> Instance<P> {
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

    /// The call's result code, after logging a refusal.
    pub(crate) fn answer(&self, call: &str, result: Result<(), CallError>) -> Tresult {
        match result {
            Ok(()) => K_RESULT_OK,
            Err(error) => {
                self.diagnostics.report(call, &error);
                failure_code(&error)
            }
        }
    }
}

fn failure_code(error: &CallError) -> Tresult {
    match error {
        CallError::NullArgument(_)
        | CallError::UnknownParamId(_)
        | CallError::UnknownConfigId(_)
        | CallError::UnknownUnitId(_)
        | CallError::UnknownProgramListId(_)
        | CallError::IndexOutOfRange(_) => K_INVALID_ARGUMENT,
        CallError::ProcessorBusy => K_INTERNAL_ERROR,
        CallError::IncompleteStream
        | CallError::StreamWrite { .. }
        | CallError::StreamRead
        | CallError::StateTooLarge { .. }
        | CallError::State(_)
        | CallError::ExtraStateSkipped => K_RESULT_FALSE,
    }
}

/// Stable C ABI helper: return a raw pointer to the instance's DebugBus.
/// Used by the per-type exported lookup symbol the `stave_vst3_export!`
/// macro emits. The pointer is owned by the Instance — the host must not
/// free it. Lifetime: valid until the plugin instance is released.
/// IPlugView container with vtable header so it can be returned as
/// `*mut FUnknown`. Allocated lazily on the first createView().
#[repr(C)]
pub struct EditorBox {
    pub vtbl: *const crate::sys::IPlugViewVtbl,
    pub view: Box<crate::cocoa_view::PluginEditorView>,
}

// SAFETY: the VST3 threading model confines the editor to the main
// thread; the bounds only let it sit inside the shared `Instance`.
unsafe impl Send for EditorBox {}
// SAFETY: as above.
unsafe impl Sync for EditorBox {}

pub fn debug_bus_ptr<P: Vst3Export>(inst: &Instance<P>) -> *const DebugBus {
    Arc::as_ptr(&inst.debug_bus)
}

// ===========================================================================
// Shared FUnknown logic
//
// All three sub-objects have layout { *const Vtbl; *mut Instance<P>; }, so we
// can read `parent` from any of them via the ComponentObj layout. The vtable
// pointer that arrives as `this` from the host could be pointing at any of
// the three sub-objects — we just need the parent.
// ===========================================================================

/// # Safety
/// `this` is an interface pointer of a live `Instance<P>`.
#[inline]
unsafe fn parent_of<P: Vst3Export>(this: *mut FUnknown) -> *mut Instance<P> {
    // SAFETY: every interface object is `{ vtbl, parent }` (`repr(C)`), so
    // `ComponentObj`'s layout reads `parent` from any of them.
    unsafe { (*(this as *mut ComponentObj<P>)).parent }
}

/// Scratch must fit the WIDEST declared member — the host can
/// renegotiate configs between activations.
fn max_in_channels<P: Plugin>() -> usize {
    P::CONFIGS
        .iter()
        .map(|c| c.in_channels() as usize)
        .max()
        .unwrap_or(0)
}

/// Shared view of the instance. This is the ONLY reference entry points
/// create — exclusive access goes through `Instance::plugin.claim()`.
///
/// # Safety
/// As [`parent_of`]; the host's reference through `this` keeps the
/// instance alive for the call. The VST3 host contract: every entry point
/// receives such a `this`.
#[inline]
pub(crate) unsafe fn parent_ref<'a, P: Vst3Export>(this: *mut FUnknown) -> &'a Instance<P> {
    // SAFETY: this function's contract.
    unsafe { &*parent_of::<P>(this) }
}

pub(crate) unsafe extern "C" fn shared_query_interface<P: Vst3Export>(
    this: *mut FUnknown,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`); `iid` and `obj` are null
    // or valid.
    let (parent, iid, obj) = unsafe { (parent_of::<P>(this), iid.as_ref(), out_param(obj, "obj")) };
    let (Some(&iid), Ok(obj)) = (iid, obj) else {
        return K_INVALID_ARGUMENT;
    };
    // SAFETY: the host's reference keeps `parent` alive; `addr_of_mut!`
    // makes sub-object pointers without a `&mut` (other threads hold
    // `&Instance` meanwhile).
    let interface: *mut c_void = unsafe {
        if iid == IID_FUNKNOWN || iid == IID_IPLUGIN_BASE || iid == IID_ICOMPONENT {
            ptr::addr_of_mut!((*parent).component).cast()
        } else if iid == IID_IAUDIO_PROCESSOR {
            ptr::addr_of_mut!((*parent).processor).cast()
        } else if iid == IID_IEDIT_CONTROLLER {
            ptr::addr_of_mut!((*parent).controller).cast()
        } else if iid == IID_IPROCESS_CONTEXT_REQUIREMENTS {
            ptr::addr_of_mut!((*parent).pcr).cast()
        } else if iid == IID_IUNIT_INFO {
            ptr::addr_of_mut!((*parent).unit_info).cast()
        } else {
            *obj = ptr::null_mut();
            return K_NO_INTERFACE;
        }
    };
    *obj = interface;
    // SAFETY: as above.
    unsafe { shared_add_ref::<P>(this) };
    K_RESULT_OK
}

pub(crate) unsafe extern "C" fn shared_add_ref<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

pub(crate) unsafe extern "C" fn shared_release<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // SAFETY: host contract (see `crate::host`).
    let parent = unsafe { parent_of::<P>(this) };
    // SAFETY: the host's reference keeps `parent` alive until this
    // decrement.
    let now = unsafe { (*parent).ref_count.fetch_sub(1, Ordering::SeqCst) } - 1;
    if now == 0 {
        // SAFETY: the last reference is gone; the factory made `parent`
        // with `Box::into_raw`.
        let instance = unsafe { Box::from_raw(parent) };
        // A destructor that panics (logged by the hook) must not unwind
        // into the host.
        let _ = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| drop(instance)));
    }
    now
}

// ===========================================================================
// ===========================================================================
// Vtable builders — these are const fns so the macro can put their results
// into per-type statics.
// ===========================================================================

pub const fn build_component_vtbl<P: Vst3Export>() -> IComponentVtbl {
    IComponentVtbl {
        query_interface: shared_query_interface::<P>,
        add_ref: shared_add_ref::<P>,
        release: shared_release::<P>,
        initialize: comp_initialize,
        terminate: comp_terminate,
        get_controller_class_id: comp_get_controller_class_id::<P>,
        set_io_mode: comp_set_io_mode,
        get_bus_count: buses::comp_get_bus_count::<P>,
        get_bus_info: buses::comp_get_bus_info::<P>,
        get_routing_info: buses::comp_get_routing_info,
        activate_bus: buses::comp_activate_bus,
        set_active: comp_set_active::<P>,
        set_state: comp_set_state::<P>,
        get_state: comp_get_state::<P>,
    }
}

pub const fn build_processor_vtbl<P: Vst3Export>() -> IAudioProcessorVtbl {
    IAudioProcessorVtbl {
        query_interface: shared_query_interface::<P>,
        add_ref: shared_add_ref::<P>,
        release: shared_release::<P>,
        set_bus_arrangements: buses::ap_set_bus_arrangements::<P>,
        get_bus_arrangement: buses::ap_get_bus_arrangement::<P>,
        can_process_sample_size: ap_can_process_sample_size,
        get_latency_samples: ap_get_latency_samples::<P>,
        setup_processing: ap_setup_processing::<P>,
        set_processing: ap_set_processing,
        process: ap_process::<P>,
        get_tail_samples: ap_get_tail_samples::<P>,
    }
}

pub const fn build_controller_vtbl<P: Vst3Export>() -> IEditControllerVtbl {
    IEditControllerVtbl {
        query_interface: shared_query_interface::<P>,
        add_ref: shared_add_ref::<P>,
        release: shared_release::<P>,
        initialize: ec_initialize,
        terminate: ec_terminate,
        set_component_state: ec_set_component_state::<P>,
        set_state: ec_set_state,
        get_state: ec_get_state,
        get_parameter_count: ec_get_parameter_count::<P>,
        get_parameter_info: ec_get_parameter_info::<P>,
        get_param_string_by_value: ec_get_param_string_by_value::<P>,
        get_param_value_by_string: ec_get_param_value_by_string::<P>,
        normalized_param_to_plain: ec_normalized_param_to_plain::<P>,
        plain_param_to_normalized: ec_plain_param_to_normalized::<P>,
        get_param_normalized: ec_get_param_normalized::<P>,
        set_param_normalized: ec_set_param_normalized::<P>,
        set_component_handler: ec_set_component_handler::<P>,
        create_view: ec_create_view::<P>,
    }
}

// ===========================================================================
// Factory
// ===========================================================================
// Helpers
// ===========================================================================
