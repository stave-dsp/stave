//! The raw VST3 ABI: SDK types, constants, interface vtables, IIDs and
//! `tresult` codes, hand-written against the Steinberg
//! `vst3_pluginterfaces` headers — no external bindings crate, and no
//! logic. `stave-vst3` (the plugin side) and `stave-host` (the host side)
//! both speak VST3 through this crate, so a host need not link the plugin
//! wrapper (or its editor platform crate) to read the ABI.
//!
//! `unsafe` appears only where the ABI requires it: the `unsafe extern "C"`
//! function pointers of the vtables.
//!
//! All structs are `#[repr(C)]` with field order matching the C++ headers.
//! Interface vtables are `#[repr(C)]` structs of plain (non-null)
//! `unsafe extern "C" fn` pointers laid out in C++ declaration order
//! (FUnknown methods first, then any inherited interface methods, then own
//! methods).
//!
//! Two things depend on the SDK's `COM_COMPATIBLE` switch
//! (`pluginterfaces/base/funknown.h`), which is 1 on Windows only: the
//! `tresult` values and the TUID byte order of `INLINE_UID`. Both follow
//! `cfg(windows)` here, so a CID declared once means the same class on
//! every platform.

#![cfg_attr(not(test), no_std)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use core::ffi::{c_char, c_void};

// ===========================================================================
// Primitive types matching pluginterfaces/base/ftypes.h
// ===========================================================================

pub type Tresult = i32;
pub type Tuid = [u8; 16];
pub type FidString = *const c_char;
pub type String128 = [u16; 128];
pub type TChar = u16;
pub type TBool = u8;

pub type SampleRate = f64;
pub type ParamId = u32;
pub type ParamValue = f64;
pub type UnitId = i32;
pub type Sample32 = f32;
pub type Sample64 = f64;
pub type SpeakerArrangement = u64;
pub type TQuarterNotes = f64;

// tresult constants (funknown.h). `kResultOk`/`kResultTrue` = 0 and
// `kResultFalse` = 1 everywhere; the errors are HRESULTs on Windows
// (COM_COMPATIBLE) and small integers elsewhere. Never test a result's
// sign: on macOS/Linux `kResultFalse` and every error but
// `kNoInterface` are positive.
pub const K_RESULT_OK: Tresult = 0;
pub const K_RESULT_TRUE: Tresult = K_RESULT_OK;
pub const K_RESULT_FALSE: Tresult = 1;
#[cfg(windows)]
pub const K_NO_INTERFACE: Tresult = 0x8000_4002_u32 as i32; // E_NOINTERFACE
#[cfg(windows)]
pub const K_INVALID_ARGUMENT: Tresult = 0x8007_0057_u32 as i32; // E_INVALIDARG
#[cfg(windows)]
pub const K_NOT_IMPLEMENTED: Tresult = 0x8000_4001_u32 as i32; // E_NOTIMPL
#[cfg(windows)]
pub const K_INTERNAL_ERROR: Tresult = 0x8000_4005_u32 as i32; // E_FAIL
#[cfg(windows)]
pub const K_NOT_INITIALIZED: Tresult = 0x8000_FFFF_u32 as i32; // E_UNEXPECTED
#[cfg(windows)]
pub const K_OUT_OF_MEMORY: Tresult = 0x8007_000E_u32 as i32; // E_OUTOFMEMORY
#[cfg(not(windows))]
pub const K_NO_INTERFACE: Tresult = -1;
#[cfg(not(windows))]
pub const K_INVALID_ARGUMENT: Tresult = 2;
#[cfg(not(windows))]
pub const K_NOT_IMPLEMENTED: Tresult = 3;
#[cfg(not(windows))]
pub const K_INTERNAL_ERROR: Tresult = 4;
#[cfg(not(windows))]
pub const K_NOT_INITIALIZED: Tresult = 5;
#[cfg(not(windows))]
pub const K_OUT_OF_MEMORY: Tresult = 6;

/// The SDK name of a result code on this platform (`kNotImplemented`, …),
/// or `None` for a value that is none of them.
pub const fn tresult_name(result: Tresult) -> Option<&'static str> {
    Some(match result {
        K_RESULT_OK => "kResultOk",
        K_RESULT_FALSE => "kResultFalse",
        K_NO_INTERFACE => "kNoInterface",
        K_INVALID_ARGUMENT => "kInvalidArgument",
        K_NOT_IMPLEMENTED => "kNotImplemented",
        K_INTERNAL_ERROR => "kInternalError",
        K_NOT_INITIALIZED => "kNotInitialized",
        K_OUT_OF_MEMORY => "kOutOfMemory",
        _ => return None,
    })
}

// SpeakerArrangement constants (vsttypes.h).
pub const SPEAKER_L: SpeakerArrangement = 1 << 0;
pub const SPEAKER_R: SpeakerArrangement = 1 << 1;
pub const SPEAKER_C: SpeakerArrangement = 1 << 2;
pub const SPEAKER_LFE: SpeakerArrangement = 1 << 3;
pub const SPEAKER_LS: SpeakerArrangement = 1 << 4;
pub const SPEAKER_RS: SpeakerArrangement = 1 << 5;
pub const K_EMPTY: SpeakerArrangement = 0;
pub const K_MONO: SpeakerArrangement = SPEAKER_C;
pub const K_STEREO: SpeakerArrangement = SPEAKER_L | SPEAKER_R;
/// 5.1 (vst3sdk k51): L R C LFE Ls Rs.
pub const K_51: SpeakerArrangement =
    SPEAKER_L | SPEAKER_R | SPEAKER_C | SPEAKER_LFE | SPEAKER_LS | SPEAKER_RS;

// MediaType enum
pub const MEDIA_TYPE_AUDIO: i32 = 0;
pub const MEDIA_TYPE_EVENT: i32 = 1;

// BusDirection enum
pub const BUS_DIR_INPUT: i32 = 0;
pub const BUS_DIR_OUTPUT: i32 = 1;

// BusType enum
pub const BUS_TYPE_MAIN: i32 = 0;
pub const BUS_TYPE_AUX: i32 = 1;

// BusInfo flags
pub const BUS_FLAG_DEFAULT_ACTIVE: u32 = 1 << 0;
pub const BUS_FLAG_IS_CONTROL_VOLTAGE: u32 = 1 << 1;

// IoMode enum
pub const IO_MODE_SIMPLE: i32 = 0;
pub const IO_MODE_ADVANCED: i32 = 1;
pub const IO_MODE_OFFLINE: i32 = 2;

// ProcessModes
pub const PROCESS_MODE_REALTIME: i32 = 0;
pub const PROCESS_MODE_PREFETCH: i32 = 1;
pub const PROCESS_MODE_OFFLINE: i32 = 2;

// SymbolicSampleSizes
pub const SAMPLE_SIZE_32: i32 = 0;
pub const SAMPLE_SIZE_64: i32 = 1;

// Event types
pub const EVENT_NOTE_ON: u16 = 0;
pub const EVENT_NOTE_OFF: u16 = 1;
pub const EVENT_DATA: u16 = 2;
pub const EVENT_POLY_PRESSURE: u16 = 3;
pub const EVENT_NOTE_EXPRESSION_VALUE: u16 = 4;

// Plugin category strings (from PClassInfo::category).
pub const CATEGORY_EFFECT: &core::ffi::CStr = c"Audio Module Class";

// Class flags (PClassInfo2).
pub const K_DISTRIBUTABLE: u32 = 1 << 0;
pub const K_SIMPLE_MODE_SUPPORTED: u32 = 1 << 1;

// Subcategories — strings, multiple separated by '|'.
// e.g. "Fx" | "Instrument|Synth|Stereo"
pub const SUBCATEGORY_FX: &core::ffi::CStr = c"Fx";
pub const SUBCATEGORY_INSTRUMENT_SYNTH: &core::ffi::CStr = c"Instrument|Synth";

// ===========================================================================
// TUID builder
// ===========================================================================

/// The Steinberg `INLINE_UID(l1, l2, l3, l4)` macro.
///
/// macOS/Linux (non-COM): each 32-bit word is stored big-endian.
/// Windows (COM_COMPATIBLE): the GUID layout — `l1` little-endian, `l2`
/// as two little-endian 16-bit halves (high half first), `l3`/`l4`
/// big-endian.
pub const fn make_tuid(l1: u32, l2: u32, l3: u32, l4: u32) -> Tuid {
    let (a, b, c, d) = (
        l1.to_be_bytes(),
        l2.to_be_bytes(),
        l3.to_be_bytes(),
        l4.to_be_bytes(),
    );
    #[cfg(windows)]
    let (a, b) = ([a[3], a[2], a[1], a[0]], [b[1], b[0], b[3], b[2]]);
    [
        a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3], c[0], c[1], c[2], c[3], d[0], d[1], d[2],
        d[3],
    ]
}

/// A TUID in the order hosts print it (`l1 l2 l3 l4`, each big-endian):
/// [`make_tuid`]'s layout undone. The same bytes on every platform.
pub const fn canonical_tuid(t: Tuid) -> [u8; 16] {
    #[cfg(windows)]
    let t = [
        t[3], t[2], t[1], t[0], t[5], t[4], t[7], t[6], t[8], t[9], t[10], t[11], t[12], t[13],
        t[14], t[15],
    ];
    t
}

// ===========================================================================
// Interface IIDs
// ===========================================================================

pub const IID_FUNKNOWN: Tuid = make_tuid(0x00000000, 0x00000000, 0xC0000000, 0x00000046);
pub const IID_IPLUGIN_BASE: Tuid = make_tuid(0x22888DDB, 0x156E45AE, 0x8358B348, 0x08190625);
pub const IID_IPLUGIN_FACTORY: Tuid = make_tuid(0x7A4D811C, 0x52114A1F, 0xAED9D2EE, 0x0B43BF9F);
pub const IID_IPLUGIN_FACTORY2: Tuid = make_tuid(0x0007B650, 0xF24B4C0B, 0xA464EDB9, 0xF00B2ABB);
pub const IID_IPLUGIN_FACTORY3: Tuid = make_tuid(0x4555A2AB, 0xC1234E57, 0x9B122910, 0x36878931);
pub const IID_ICOMPONENT: Tuid = make_tuid(0xE831FF31, 0xF2D54301, 0x928EBBEE, 0x25697802);
pub const IID_IAUDIO_PROCESSOR: Tuid = make_tuid(0x42043F99, 0xB7DA453C, 0xA569E79D, 0x9AAEC33D);
pub const IID_IEDIT_CONTROLLER: Tuid = make_tuid(0xDCD7BBE3, 0x7742448D, 0xA874AACC, 0x979C759E);
pub const IID_ICOMPONENT_HANDLER: Tuid = make_tuid(0x93A0BEA3, 0x0BD045DB, 0x8E890B0C, 0xC1E46AC6);
pub const IID_IEVENT_LIST: Tuid = make_tuid(0x3A2C4214, 0x346349FE, 0xB2C4F397, 0xB9695A44);
pub const IID_IPLUG_VIEW: Tuid = make_tuid(0x5BC32507, 0xD06049EA, 0xA6151B52, 0x2B755B29);
pub const IID_IHOST_APPLICATION: Tuid = make_tuid(0x58E595CC, 0xDB2D4969, 0x8B6AAF8C, 0x36A664E5);
pub const IID_ICONNECTION_POINT: Tuid = make_tuid(0x70A4156F, 0x6E6E4026, 0x989148BF, 0xAA60D8D1);
pub const IID_IPLUG_FRAME: Tuid = make_tuid(0x367FAF01, 0xAFA94693, 0x8D4DA2A0, 0xED0882A3);
/// Linux only (pluginterfaces/gui/iplugview.h): the host's run loop.
pub const IID_IRUN_LOOP: Tuid = make_tuid(0x18C35366, 0x97764F1A, 0x9C5B8385, 0x7A871389);
pub const IID_IEVENT_HANDLER: Tuid = make_tuid(0x561E65C9, 0x13A0496F, 0x813A2C35, 0x654D7983);
pub const IID_ITIMER_HANDLER: Tuid = make_tuid(0x10BDD94F, 0x41424774, 0x821FAD8F, 0xECA72CA9);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ViewRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `Linux::IRunLoop`: the host's main loop, which an X11 view joins with
/// a file descriptor handler and a timer.
#[repr(C)]
pub struct IRunLoopVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub register_event_handler:
        unsafe extern "C" fn(this: *mut FUnknown, handler: *mut FUnknown, fd: i32) -> Tresult,
    pub unregister_event_handler: unsafe extern "C" fn(this: *mut FUnknown, handler: *mut FUnknown) -> Tresult,
    pub register_timer:
        unsafe extern "C" fn(this: *mut FUnknown, handler: *mut FUnknown, milliseconds: u64) -> Tresult,
    pub unregister_timer: unsafe extern "C" fn(this: *mut FUnknown, handler: *mut FUnknown) -> Tresult,
}

/// `Linux::IEventHandler`: called when a registered descriptor is ready.
#[repr(C)]
pub struct IEventHandlerVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub on_fd_is_set: unsafe extern "C" fn(this: *mut FUnknown, fd: i32),
}

/// `Linux::ITimerHandler`: called on a registered timer.
#[repr(C)]
pub struct ITimerHandlerVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub on_timer: unsafe extern "C" fn(this: *mut FUnknown),
}

#[repr(C)]
pub struct IPlugViewVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub is_platform_type_supported:
        unsafe extern "C" fn(this: *mut FUnknown, ty: FidString) -> Tresult,
    pub attached:
        unsafe extern "C" fn(this: *mut FUnknown, parent: *mut c_void, ty: FidString) -> Tresult,
    pub removed: unsafe extern "C" fn(this: *mut FUnknown) -> Tresult,
    pub on_wheel: unsafe extern "C" fn(this: *mut FUnknown, distance: f32) -> Tresult,
    pub on_key_down: unsafe extern "C" fn(
        this: *mut FUnknown,
        key: i16,
        key_code: i16,
        modifiers: i16,
    ) -> Tresult,
    pub on_key_up: unsafe extern "C" fn(
        this: *mut FUnknown,
        key: i16,
        key_code: i16,
        modifiers: i16,
    ) -> Tresult,
    pub get_size: unsafe extern "C" fn(this: *mut FUnknown, size: *mut ViewRect) -> Tresult,
    pub on_size: unsafe extern "C" fn(this: *mut FUnknown, new_size: *mut ViewRect) -> Tresult,
    pub on_focus: unsafe extern "C" fn(this: *mut FUnknown, state: TBool) -> Tresult,
    pub set_frame: unsafe extern "C" fn(this: *mut FUnknown, frame: *mut FUnknown) -> Tresult,
    pub can_resize: unsafe extern "C" fn(this: *mut FUnknown) -> Tresult,
    pub check_size_constraint:
        unsafe extern "C" fn(this: *mut FUnknown, rect: *mut ViewRect) -> Tresult,
}

// ===========================================================================
// Factory descriptor structs
// ===========================================================================

#[repr(C)]
pub struct PFactoryInfo {
    pub vendor: [c_char; 64],
    pub url: [c_char; 256],
    pub email: [c_char; 128],
    pub flags: i32,
}

pub const FACTORY_FLAG_UNICODE: i32 = 1 << 4;

#[repr(C)]
pub struct PClassInfo {
    pub cid: Tuid,
    pub cardinality: i32,
    pub category: [c_char; 32],
    pub name: [c_char; 64],
}

pub const K_MANY_INSTANCES: i32 = 0x7FFF_FFFF;

#[repr(C)]
pub struct PClassInfo2 {
    pub cid: Tuid,
    pub cardinality: i32,
    pub category: [c_char; 32],
    pub name: [c_char; 64],
    pub class_flags: u32,
    pub sub_categories: [c_char; 128],
    pub vendor: [c_char; 64],
    pub version: [c_char; 64],
    pub sdk_version: [c_char; 64],
}

#[repr(C)]
pub struct PClassInfoW {
    pub cid: Tuid,
    pub cardinality: i32,
    pub category: [c_char; 32],
    pub name: [u16; 64],
    pub class_flags: u32,
    pub sub_categories: [c_char; 128],
    pub vendor: [u16; 64],
    pub version: [u16; 64],
    pub sdk_version: [u16; 64],
}

// ===========================================================================
// Bus / routing
// ===========================================================================

#[repr(C)]
pub struct BusInfo {
    pub media_type: i32,
    pub direction: i32,
    pub channel_count: i32,
    pub name: String128,
    pub bus_type: i32,
    pub flags: u32,
}

#[repr(C)]
pub struct RoutingInfo {
    pub media_type: i32,
    pub bus_index: i32,
    pub channel: i32,
}

// ===========================================================================
// Process types
// ===========================================================================

#[repr(C)]
pub struct ProcessSetup {
    pub process_mode: i32,
    pub symbolic_sample_size: i32,
    pub max_samples_per_block: i32,
    pub sample_rate: SampleRate,
}

#[repr(C)]
pub struct AudioBusBuffers {
    pub num_channels: i32,
    pub silence_flags: u64,
    /// Union of `Sample32**` and `Sample64**`. We always work in f32 (the
    /// plugin advertises SAMPLE_SIZE_32 only) so we treat this as `*mut *mut f32`.
    pub channel_buffers_32: *mut *mut f32,
}

#[repr(C)]
pub struct ProcessData {
    pub process_mode: i32,
    pub symbolic_sample_size: i32,
    pub num_samples: i32,
    pub num_inputs: i32,
    pub num_outputs: i32,
    pub inputs: *mut AudioBusBuffers,
    pub outputs: *mut AudioBusBuffers,
    pub input_parameter_changes: *mut IParameterChanges,
    pub output_parameter_changes: *mut IParameterChanges,
    pub input_events: *mut IEventList,
    pub output_events: *mut IEventList,
    pub process_context: *mut ProcessContext,
}

/// Steinberg::Vst::ProcessContext — full ABI layout (validity of each
/// field is gated by `state` bits; read nothing without its flag).
#[repr(C)]
pub struct ProcessContext {
    pub state: u32,
    pub sample_rate: f64,
    pub project_time_samples: i64,
    pub system_time: i64,
    pub continuous_time_samples: i64,
    pub project_time_music: TQuarterNotes,
    pub bar_position_music: TQuarterNotes,
    pub cycle_start_music: TQuarterNotes,
    pub cycle_end_music: TQuarterNotes,
    pub tempo: f64,
    pub time_sig_numerator: i32,
    pub time_sig_denominator: i32,
    /// Chord (packed: keyNote u8, rootNote u8, chordMask i16).
    pub chord: u32,
    pub smpte_offset_subframes: i32,
    pub frame_rate: [u32; 2],
    pub samples_to_next_clock: i32,
}

/// ProcessContext::state bits (subset consumed).
pub const PC_PLAYING: u32 = 1 << 1;
pub const PC_TEMPO_VALID: u32 = 1 << 10;
pub const PC_PROJECT_TIME_MUSIC_VALID: u32 = 1 << 9;

// ===========================================================================
// Events
// ===========================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NoteOnEvent {
    pub channel: i16,
    pub pitch: i16,
    pub tuning: f32,
    pub velocity: f32,
    pub length: i32,
    pub note_id: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NoteOffEvent {
    pub channel: i16,
    pub pitch: i16,
    pub velocity: f32,
    pub note_id: i32,
    pub tuning: f32,
}

/// 8-byte aligned (matches C++ Event whose union contains f64 fields).
/// We size the union region with `[u64; 5]` (40 bytes), comfortably larger
/// than any single C++ union variant we care about (NoteOnEvent is 20 bytes,
/// the biggest is NoteExpressionTextEvent at ~32 bytes including pointer).
#[repr(C)]
#[derive(Copy, Clone)]
pub union EventData {
    pub note_on: NoteOnEvent,
    pub note_off: NoteOffEvent,
    /// The C++ union's size: its largest member (NoteExpressionTextEvent)
    /// is 24 bytes, 8-aligned.
    pub _padding: [u64; 3],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Event {
    pub bus_index: i32,
    pub sample_offset: i32,
    pub ppq_position: TQuarterNotes,
    pub flags: u16,
    pub r#type: u16,
    pub data: EventData,
}

// ===========================================================================
// Parameter changes (we only read input changes; output is no-op)
// ===========================================================================

#[repr(C)]
pub struct IParameterChangesVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut IParameterChanges,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut IParameterChanges) -> u32,
    pub release: unsafe extern "C" fn(this: *mut IParameterChanges) -> u32,
    pub get_parameter_count: unsafe extern "C" fn(this: *mut IParameterChanges) -> i32,
    pub get_parameter_data:
        unsafe extern "C" fn(this: *mut IParameterChanges, index: i32) -> *mut IParamValueQueue,
    pub add_parameter_data: unsafe extern "C" fn(
        this: *mut IParameterChanges,
        id: *const ParamId,
        index: *mut i32,
    ) -> *mut IParamValueQueue,
}

#[repr(C)]
pub struct IParameterChanges {
    pub vtbl: *const IParameterChangesVtbl,
}

#[repr(C)]
pub struct IParamValueQueueVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut IParamValueQueue,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut IParamValueQueue) -> u32,
    pub release: unsafe extern "C" fn(this: *mut IParamValueQueue) -> u32,
    pub get_parameter_id: unsafe extern "C" fn(this: *mut IParamValueQueue) -> ParamId,
    pub get_point_count: unsafe extern "C" fn(this: *mut IParamValueQueue) -> i32,
    pub get_point: unsafe extern "C" fn(
        this: *mut IParamValueQueue,
        index: i32,
        sample_offset: *mut i32,
        value: *mut ParamValue,
    ) -> Tresult,
    pub add_point: unsafe extern "C" fn(
        this: *mut IParamValueQueue,
        sample_offset: i32,
        value: ParamValue,
        index: *mut i32,
    ) -> Tresult,
}

#[repr(C)]
pub struct IParamValueQueue {
    pub vtbl: *const IParamValueQueueVtbl,
}

// ===========================================================================
// IEventList
// ===========================================================================

#[repr(C)]
pub struct IEventListVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut IEventList,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut IEventList) -> u32,
    pub release: unsafe extern "C" fn(this: *mut IEventList) -> u32,
    pub get_event_count: unsafe extern "C" fn(this: *mut IEventList) -> i32,
    pub get_event:
        unsafe extern "C" fn(this: *mut IEventList, index: i32, event: *mut Event) -> Tresult,
    pub add_event: unsafe extern "C" fn(this: *mut IEventList, event: *mut Event) -> Tresult,
}

#[repr(C)]
pub struct IEventList {
    pub vtbl: *const IEventListVtbl,
}

// ===========================================================================
// FUnknown / IPluginBase / IComponent / IAudioProcessor / IEditController
//
// These are the vtables WE expose to the host. Each starts with the FUnknown
// triple (query_interface/add_ref/release), then any inherited interface
// methods in declaration order, then own methods.
// ===========================================================================

#[repr(C)]
pub struct IPluginFactoryVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub get_factory_info:
        unsafe extern "C" fn(this: *mut FUnknown, info: *mut PFactoryInfo) -> Tresult,
    pub count_classes: unsafe extern "C" fn(this: *mut FUnknown) -> i32,
    pub get_class_info:
        unsafe extern "C" fn(this: *mut FUnknown, index: i32, info: *mut PClassInfo) -> Tresult,
    pub create_instance: unsafe extern "C" fn(
        this: *mut FUnknown,
        cid: FidString,
        iid: FidString,
        obj: *mut *mut c_void,
    ) -> Tresult,
    // IPluginFactory2
    pub get_class_info_2:
        unsafe extern "C" fn(this: *mut FUnknown, index: i32, info: *mut PClassInfo2) -> Tresult,
    // IPluginFactory3
    pub get_class_info_unicode:
        unsafe extern "C" fn(this: *mut FUnknown, index: i32, info: *mut PClassInfoW) -> Tresult,
    pub set_host_context:
        unsafe extern "C" fn(this: *mut FUnknown, context: *mut FUnknown) -> Tresult,
}

/// Opaque "this" type used by all FUnknown-derived vtables. Each concrete
/// object is a different Rust type whose first field is `*const SomeVtbl`,
/// but we cast through `*mut FUnknown` at the ABI boundary.
#[repr(C)]
pub struct FUnknown {
    pub vtbl: *const c_void,
}

#[repr(C)]
pub struct IComponentVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub initialize: unsafe extern "C" fn(this: *mut FUnknown, context: *mut FUnknown) -> Tresult,
    pub terminate: unsafe extern "C" fn(this: *mut FUnknown) -> Tresult,
    pub get_controller_class_id:
        unsafe extern "C" fn(this: *mut FUnknown, class_id: *mut Tuid) -> Tresult,
    pub set_io_mode: unsafe extern "C" fn(this: *mut FUnknown, mode: i32) -> Tresult,
    pub get_bus_count: unsafe extern "C" fn(this: *mut FUnknown, media_type: i32, dir: i32) -> i32,
    pub get_bus_info: unsafe extern "C" fn(
        this: *mut FUnknown,
        media_type: i32,
        dir: i32,
        index: i32,
        bus: *mut BusInfo,
    ) -> Tresult,
    pub get_routing_info: unsafe extern "C" fn(
        this: *mut FUnknown,
        in_info: *mut RoutingInfo,
        out_info: *mut RoutingInfo,
    ) -> Tresult,
    pub activate_bus: unsafe extern "C" fn(
        this: *mut FUnknown,
        media_type: i32,
        dir: i32,
        index: i32,
        state: TBool,
    ) -> Tresult,
    pub set_active: unsafe extern "C" fn(this: *mut FUnknown, state: TBool) -> Tresult,
    pub set_state: unsafe extern "C" fn(this: *mut FUnknown, state: *mut FUnknown) -> Tresult,
    pub get_state: unsafe extern "C" fn(this: *mut FUnknown, state: *mut FUnknown) -> Tresult,
}

#[repr(C)]
pub struct IAudioProcessorVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub set_bus_arrangements: unsafe extern "C" fn(
        this: *mut FUnknown,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> Tresult,
    pub get_bus_arrangement: unsafe extern "C" fn(
        this: *mut FUnknown,
        dir: i32,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> Tresult,
    pub can_process_sample_size:
        unsafe extern "C" fn(this: *mut FUnknown, symbolic_sample_size: i32) -> Tresult,
    pub get_latency_samples: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub setup_processing:
        unsafe extern "C" fn(this: *mut FUnknown, setup: *mut ProcessSetup) -> Tresult,
    pub set_processing: unsafe extern "C" fn(this: *mut FUnknown, state: TBool) -> Tresult,
    pub process: unsafe extern "C" fn(this: *mut FUnknown, data: *mut ProcessData) -> Tresult,
    pub get_tail_samples: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
}

#[repr(C)]
pub struct IEditControllerVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub initialize: unsafe extern "C" fn(this: *mut FUnknown, context: *mut FUnknown) -> Tresult,
    pub terminate: unsafe extern "C" fn(this: *mut FUnknown) -> Tresult,
    pub set_component_state:
        unsafe extern "C" fn(this: *mut FUnknown, state: *mut FUnknown) -> Tresult,
    pub set_state: unsafe extern "C" fn(this: *mut FUnknown, state: *mut FUnknown) -> Tresult,
    pub get_state: unsafe extern "C" fn(this: *mut FUnknown, state: *mut FUnknown) -> Tresult,
    pub get_parameter_count: unsafe extern "C" fn(this: *mut FUnknown) -> i32,
    pub get_parameter_info:
        unsafe extern "C" fn(this: *mut FUnknown, index: i32, info: *mut ParameterInfo) -> Tresult,
    pub get_param_string_by_value: unsafe extern "C" fn(
        this: *mut FUnknown,
        id: ParamId,
        value_normalized: ParamValue,
        string: *mut TChar,
    ) -> Tresult,
    pub get_param_value_by_string: unsafe extern "C" fn(
        this: *mut FUnknown,
        id: ParamId,
        string: *mut TChar,
        value_normalized: *mut ParamValue,
    ) -> Tresult,
    pub normalized_param_to_plain: unsafe extern "C" fn(
        this: *mut FUnknown,
        id: ParamId,
        value_normalized: ParamValue,
    ) -> ParamValue,
    pub plain_param_to_normalized: unsafe extern "C" fn(
        this: *mut FUnknown,
        id: ParamId,
        plain_value: ParamValue,
    ) -> ParamValue,
    pub get_param_normalized: unsafe extern "C" fn(this: *mut FUnknown, id: ParamId) -> ParamValue,
    pub set_param_normalized:
        unsafe extern "C" fn(this: *mut FUnknown, id: ParamId, value: ParamValue) -> Tresult,
    pub set_component_handler:
        unsafe extern "C" fn(this: *mut FUnknown, handler: *mut FUnknown) -> Tresult,
    pub create_view: unsafe extern "C" fn(this: *mut FUnknown, name: FidString) -> *mut FUnknown,
}

#[repr(C)]
pub struct ParameterInfo {
    pub id: ParamId,
    pub title: String128,
    pub short_title: String128,
    pub units: String128,
    pub step_count: i32,
    pub default_normalized_value: ParamValue,
    pub unit_id: UnitId,
    pub flags: i32,
}

// ===========================================================================
// IBStream — the host-provided stream for get/setState
// ===========================================================================

pub const IID_IBSTREAM: Tuid = make_tuid(0xC3BF6EA2, 0x30994752, 0x9B6BF990, 0x1EE33E9B);

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

#[repr(C)]
pub struct IBStreamVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut IBStream,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut IBStream) -> u32,
    pub release: unsafe extern "C" fn(this: *mut IBStream) -> u32,
    pub read: unsafe extern "C" fn(
        this: *mut IBStream,
        buffer: *mut c_void,
        num_bytes: i32,
        num_read: *mut i32,
    ) -> Tresult,
    pub write: unsafe extern "C" fn(
        this: *mut IBStream,
        buffer: *mut c_void,
        num_bytes: i32,
        num_written: *mut i32,
    ) -> Tresult,
    pub seek:
        unsafe extern "C" fn(this: *mut IBStream, pos: i64, mode: i32, result: *mut i64) -> Tresult,
    pub tell: unsafe extern "C" fn(this: *mut IBStream, pos: *mut i64) -> Tresult,
}

#[repr(C)]
pub struct IBStream {
    pub vtbl: *const IBStreamVtbl,
}

// ===========================================================================
// IComponentHandler — the host object that records automation. GUI
// edits MUST round-trip through this (beginEdit/performEdit/endEdit)
// or the DAW can neither record nor undo them.
// ===========================================================================

#[repr(C)]
pub struct IComponentHandlerVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut IComponentHandler,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut IComponentHandler) -> u32,
    pub release: unsafe extern "C" fn(this: *mut IComponentHandler) -> u32,
    pub begin_edit: unsafe extern "C" fn(this: *mut IComponentHandler, id: ParamId) -> Tresult,
    pub perform_edit: unsafe extern "C" fn(
        this: *mut IComponentHandler,
        id: ParamId,
        value_normalized: ParamValue,
    ) -> Tresult,
    pub end_edit: unsafe extern "C" fn(this: *mut IComponentHandler, id: ParamId) -> Tresult,
    pub restart_component:
        unsafe extern "C" fn(this: *mut IComponentHandler, flags: i32) -> Tresult,
}

#[repr(C)]
pub struct IComponentHandler {
    pub vtbl: *const IComponentHandlerVtbl,
}

/// restartComponent flag bits (subset we use).
pub const RESTART_PARAM_VALUES_CHANGED: i32 = 1 << 2;
pub const RESTART_LATENCY_CHANGED: i32 = 1 << 3;

// ===========================================================================
// IProcessContextRequirements — mandatory since SDK 3.7: the plugin
// declares which ProcessContext fields it consumes so hosts can skip
// computing the rest.
// ===========================================================================

pub const IID_IPROCESS_CONTEXT_REQUIREMENTS: Tuid =
    make_tuid(0x2A654303, 0xEF764E3D, 0x95B5FE83, 0x730EF6D0);

pub const PCR_NEED_PROJECT_TIME_MUSIC: u32 = 1 << 2;
pub const PCR_NEED_TEMPO: u32 = 1 << 6;
pub const PCR_NEED_TRANSPORT_STATE: u32 = 1 << 10;

#[repr(C)]
pub struct IProcessContextRequirementsVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub get_process_context_requirements: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
}

// ===========================================================================
// IUnitInfo — parameter folders (units) and program lists, so hosts can
// group parameters and show presets in their own menus.
// ===========================================================================

pub const IID_IUNIT_INFO: Tuid = make_tuid(0x3D4BD6B5, 0x913A4FD2, 0xA886E768, 0xA5EB92C1);

pub type ProgramListId = i32;

pub const ROOT_UNIT_ID: UnitId = 0;
pub const NO_PARENT_UNIT_ID: UnitId = -1;
pub const NO_PROGRAM_LIST_ID: ProgramListId = -1;

#[repr(C)]
pub struct UnitInfo {
    pub id: UnitId,
    pub parent_unit_id: UnitId,
    pub name: String128,
    pub program_list_id: ProgramListId,
}

#[repr(C)]
pub struct ProgramListInfo {
    pub id: ProgramListId,
    pub name: String128,
    pub program_count: i32,
}

#[repr(C)]
pub struct IUnitInfoVtbl {
    pub query_interface: unsafe extern "C" fn(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub get_unit_count: unsafe extern "C" fn(this: *mut FUnknown) -> i32,
    pub get_unit_info:
        unsafe extern "C" fn(this: *mut FUnknown, unit_index: i32, info: *mut UnitInfo) -> Tresult,
    pub get_program_list_count: unsafe extern "C" fn(this: *mut FUnknown) -> i32,
    pub get_program_list_info: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_index: i32,
        info: *mut ProgramListInfo,
    ) -> Tresult,
    pub get_program_name: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_id: ProgramListId,
        program_index: i32,
        name: *mut TChar,
    ) -> Tresult,
    pub get_program_info: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_id: ProgramListId,
        program_index: i32,
        attribute_id: FidString,
        value: *mut TChar,
    ) -> Tresult,
    pub has_program_pitch_names: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_id: ProgramListId,
        program_index: i32,
    ) -> Tresult,
    pub get_program_pitch_name: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_id: ProgramListId,
        program_index: i32,
        midi_pitch: i16,
        name: *mut TChar,
    ) -> Tresult,
    pub get_selected_unit: unsafe extern "C" fn(this: *mut FUnknown) -> UnitId,
    pub select_unit: unsafe extern "C" fn(this: *mut FUnknown, unit_id: UnitId) -> Tresult,
    pub get_unit_by_bus: unsafe extern "C" fn(
        this: *mut FUnknown,
        media_type: i32,
        direction: i32,
        bus_index: i32,
        channel: i32,
        unit_id: *mut UnitId,
    ) -> Tresult,
    pub set_unit_program_data: unsafe extern "C" fn(
        this: *mut FUnknown,
        list_or_unit_id: i32,
        program_index: i32,
        data: *mut IBStream,
    ) -> Tresult,
}

/// `IHostApplication` (ivsthostapplication.h): the host context a host
/// hands to `initialize`.
#[repr(C)]
pub struct IHostApplicationVtbl {
    pub query_interface:
        unsafe extern "C" fn(this: *mut FUnknown, iid: *const Tuid, obj: *mut *mut c_void) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub get_name: unsafe extern "C" fn(this: *mut FUnknown, name: *mut TChar) -> Tresult,
    pub create_instance: unsafe extern "C" fn(
        this: *mut FUnknown,
        cid: *mut Tuid,
        iid: *mut Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult,
}

/// `IConnectionPoint` (ivstmessage.h): the component <-> controller
/// wiring a host makes for split-architecture plugins.
#[repr(C)]
pub struct IConnectionPointVtbl {
    pub query_interface:
        unsafe extern "C" fn(this: *mut FUnknown, iid: *const Tuid, obj: *mut *mut c_void) -> Tresult,
    pub add_ref: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(this: *mut FUnknown) -> u32,
    pub connect: unsafe extern "C" fn(this: *mut FUnknown, other: *mut FUnknown) -> Tresult,
    pub disconnect: unsafe extern "C" fn(this: *mut FUnknown, other: *mut FUnknown) -> Tresult,
    pub notify: unsafe extern "C" fn(this: *mut FUnknown, message: *mut FUnknown) -> Tresult,
}

// The C++ layouts, measured from the VST3 SDK headers (sizeof/alignof on
// aarch64-apple-darwin). A mirror that drifts is a memory-safety bug
// against every third-party host or plugin, so it fails the build.
const _: () = {
    use core::mem::{align_of, size_of};
    macro_rules! layout {
        ($($t:ty => $size:expr, $align:expr;)*) => {
            $(assert!(size_of::<$t>() == $size && align_of::<$t>() == $align);)*
        };
    }
    layout! {
        ViewRect => 16, 4;
        PFactoryInfo => 452, 4;
        PClassInfo => 116, 4;
        PClassInfo2 => 440, 4;
        PClassInfoW => 696, 4;
        BusInfo => 276, 4;
        RoutingInfo => 12, 4;
        ProcessSetup => 24, 8;
        AudioBusBuffers => 24, 8;
        ProcessData => 80, 8;
        ProcessContext => 112, 8;
        NoteOnEvent => 20, 4;
        NoteOffEvent => 16, 4;
        Event => 48, 8;
        ParameterInfo => 792, 8;
        UnitInfo => 268, 4;
        ProgramListInfo => 264, 4;
    }
    assert!(core::mem::offset_of!(Event, data) == 24);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_tuids_read_the_same_on_every_platform() {
        let t = make_tuid(0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700001);
        let want = [
            0x7C, 0x31, 0xA2, 0xE4, 0x88, 0xB1, 0x4D, 0x02, 0xA5, 0xE0, 0xC9, 0xD3, 0x11, 0x70,
            0x00, 0x01,
        ];
        assert_eq!(canonical_tuid(t), want);
    }

    /// funknown.h, non-COM branch (macOS/Linux).
    #[cfg(not(windows))]
    #[test]
    fn tresults_and_tuids_match_the_non_com_sdk() {
        let codes = [
            K_NO_INTERFACE,
            K_RESULT_OK,
            K_RESULT_TRUE,
            K_RESULT_FALSE,
            K_INVALID_ARGUMENT,
            K_NOT_IMPLEMENTED,
            K_INTERNAL_ERROR,
            K_NOT_INITIALIZED,
            K_OUT_OF_MEMORY,
        ];
        assert_eq!(codes, [-1, 0, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(tresult_name(3), Some("kNotImplemented"));
        assert_eq!(tresult_name(7), None);
        assert_eq!(
            make_tuid(0x0102_0304, 0x0506_0708, 0x090A_0B0C, 0x0D0E_0F10),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    /// funknown.h, COM_COMPATIBLE branch (Windows HRESULTs, GUID order).
    #[cfg(windows)]
    #[test]
    fn tresults_and_tuids_match_the_com_sdk() {
        assert_eq!(K_NO_INTERFACE as u32, 0x8000_4002);
        assert_eq!(K_INVALID_ARGUMENT as u32, 0x8007_0057);
        assert_eq!((K_RESULT_OK, K_RESULT_FALSE), (0, 1));
        assert_eq!(
            make_tuid(0x0102_0304, 0x0506_0708, 0x090A_0B0C, 0x0D0E_0F10),
            [4, 3, 2, 1, 6, 5, 8, 7, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }
}
