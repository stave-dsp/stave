//! Raw CLAP 1.x C ABI declarations. Hand-written against the upstream
//! `clap.h` (<https://github.com/free-audio/clap>). No external bindings crate.
//!
//! Every struct here is `#[repr(C)]` and laid out to match the C header
//! byte-for-byte. Function pointer signatures use `extern "C"` and use the
//! same nullable convention as the C side: a `None` `Option<fn>` is the
//! NULL function pointer.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_void};

pub const CLAP_VERSION_MAJOR: u32 = 1;
pub const CLAP_VERSION_MINOR: u32 = 2;
pub const CLAP_VERSION_REVISION: u32 = 1;

pub const CLAP_VERSION: ClapVersion = ClapVersion {
    major: CLAP_VERSION_MAJOR,
    minor: CLAP_VERSION_MINOR,
    revision: CLAP_VERSION_REVISION,
};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ClapVersion {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

pub type ClapId = u32;
pub const CLAP_INVALID_ID: ClapId = u32::MAX;

// ===========================================================================
// Host
// ===========================================================================

#[repr(C)]
pub struct ClapHost {
    pub clap_version: ClapVersion,
    pub host_data: *mut c_void,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub version: *const c_char,
    pub get_extension: Option<
        unsafe extern "C" fn(host: *const ClapHost, extension_id: *const c_char) -> *const c_void,
    >,
    pub request_restart: Option<unsafe extern "C" fn(host: *const ClapHost)>,
    pub request_process: Option<unsafe extern "C" fn(host: *const ClapHost)>,
    pub request_callback: Option<unsafe extern "C" fn(host: *const ClapHost)>,
}

// ===========================================================================
// Plugin descriptor & instance
// ===========================================================================

#[repr(C)]
pub struct ClapPluginDescriptor {
    pub clap_version: ClapVersion,
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub manual_url: *const c_char,
    pub support_url: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    /// NULL-terminated array of feature C-strings.
    pub features: *const *const c_char,
}

// SAFETY: descriptors are read-only and shared across threads (the host
// caches them). All fields are 'static C-string pointers we own.
unsafe impl Sync for ClapPluginDescriptor {}

#[repr(C)]
pub struct ClapPlugin {
    pub desc: *const ClapPluginDescriptor,
    pub plugin_data: *mut c_void,
    pub init: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub destroy: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub activate: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            sample_rate: f64,
            min_frames_count: u32,
            max_frames_count: u32,
        ) -> bool,
    >,
    pub deactivate: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub start_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> bool>,
    pub stop_processing: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub reset: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
    pub process: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            process: *const ClapProcess,
        ) -> ClapProcessStatus,
    >,
    pub get_extension:
        Option<unsafe extern "C" fn(plugin: *const ClapPlugin, id: *const c_char) -> *const c_void>,
    pub on_main_thread: Option<unsafe extern "C" fn(plugin: *const ClapPlugin)>,
}

// ===========================================================================
// Process
// ===========================================================================

pub type ClapProcessStatus = i32;
pub const CLAP_PROCESS_ERROR: ClapProcessStatus = 0;
pub const CLAP_PROCESS_CONTINUE: ClapProcessStatus = 1;
pub const CLAP_PROCESS_CONTINUE_IF_NOT_QUIET: ClapProcessStatus = 2;
pub const CLAP_PROCESS_TAIL: ClapProcessStatus = 3;
pub const CLAP_PROCESS_SLEEP: ClapProcessStatus = 4;

#[repr(C)]
pub struct ClapAudioBuffer {
    pub data32: *mut *mut f32,
    pub data64: *mut *mut f64,
    pub channel_count: u32,
    pub latency: u32,
    pub constant_mask: u64,
}

#[repr(C)]
pub struct ClapProcess {
    pub steady_time: i64,
    pub frames_count: u32,
    pub transport: *const ClapEventTransport,
    pub audio_inputs: *const ClapAudioBuffer,
    pub audio_outputs: *mut ClapAudioBuffer,
    pub audio_inputs_count: u32,
    pub audio_outputs_count: u32,
    pub in_events: *const ClapInputEvents,
    pub out_events: *const ClapOutputEvents,
}

// ===========================================================================
// Events
// ===========================================================================

pub const CLAP_CORE_EVENT_SPACE_ID: u16 = 0;

pub const CLAP_EVENT_NOTE_ON: u16 = 0;
pub const CLAP_EVENT_NOTE_OFF: u16 = 1;
pub const CLAP_EVENT_NOTE_CHOKE: u16 = 2;
pub const CLAP_EVENT_NOTE_END: u16 = 3;
pub const CLAP_EVENT_NOTE_EXPRESSION: u16 = 4;
pub const CLAP_EVENT_PARAM_VALUE: u16 = 5;
pub const CLAP_EVENT_PARAM_MOD: u16 = 6;
pub const CLAP_EVENT_PARAM_GESTURE_BEGIN: u16 = 7;
pub const CLAP_EVENT_PARAM_GESTURE_END: u16 = 8;
pub const CLAP_EVENT_TRANSPORT: u16 = 9;
pub const CLAP_EVENT_MIDI: u16 = 10;
pub const CLAP_EVENT_MIDI_SYSEX: u16 = 11;
pub const CLAP_EVENT_MIDI2: u16 = 12;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ClapEventHeader {
    pub size: u32,
    pub time: u32,
    pub space_id: u16,
    pub r#type: u16,
    pub flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ClapEventNote {
    pub header: ClapEventHeader,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub velocity: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ClapEventParamValue {
    pub header: ClapEventHeader,
    pub param_id: ClapId,
    pub cookie: *mut c_void,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub value: f64,
}

#[repr(C)]
pub struct ClapEventTransport {
    pub header: ClapEventHeader,
    pub flags: u32,
    pub song_pos_beats: i64,
    pub song_pos_seconds: i64,
    pub tempo: f64,
    pub tempo_inc: f64,
    pub loop_start_beats: i64,
    pub loop_end_beats: i64,
    pub loop_start_seconds: i64,
    pub loop_end_seconds: i64,
    pub bar_start: i64,
    pub bar_number: i32,
    pub tsig_num: u16,
    pub tsig_denom: u16,
}

pub const CLAP_TRANSPORT_HAS_TEMPO: u32 = 1 << 0;
pub const CLAP_TRANSPORT_HAS_BEATS_TIMELINE: u32 = 1 << 1;
pub const CLAP_TRANSPORT_HAS_SECONDS_TIMELINE: u32 = 1 << 2;
pub const CLAP_TRANSPORT_HAS_TIME_SIGNATURE: u32 = 1 << 3;
pub const CLAP_TRANSPORT_IS_PLAYING: u32 = 1 << 4;
pub const CLAP_TRANSPORT_IS_RECORDING: u32 = 1 << 5;
pub const CLAP_TRANSPORT_IS_LOOP_ACTIVE: u32 = 1 << 6;
pub const CLAP_TRANSPORT_IS_WITHIN_PRE_ROLL: u32 = 1 << 7;

/// Fixed-point seconds/beats encoding used by CLAP transports.
/// Divide by `CLAP_SECTIME_FACTOR` (1 << 31) to get a float.
pub const CLAP_SECTIME_FACTOR: f64 = (1u64 << 31) as f64;

#[repr(C)]
pub struct ClapInputEvents {
    pub ctx: *mut c_void,
    pub size: Option<unsafe extern "C" fn(list: *const ClapInputEvents) -> u32>,
    pub get: Option<
        unsafe extern "C" fn(list: *const ClapInputEvents, index: u32) -> *const ClapEventHeader,
    >,
}

#[repr(C)]
pub struct ClapOutputEvents {
    pub ctx: *mut c_void,
    pub try_push: Option<
        unsafe extern "C" fn(list: *const ClapOutputEvents, event: *const ClapEventHeader) -> bool,
    >,
}

// ===========================================================================
// Factory & entry
// ===========================================================================

pub const CLAP_PLUGIN_FACTORY_ID: &core::ffi::CStr = c"clap.plugin-factory";

#[repr(C)]
pub struct ClapPluginFactory {
    pub get_plugin_count: Option<unsafe extern "C" fn(factory: *const ClapPluginFactory) -> u32>,
    pub get_plugin_descriptor: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            index: u32,
        ) -> *const ClapPluginDescriptor,
    >,
    pub create_plugin: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactory,
            host: *const ClapHost,
            plugin_id: *const c_char,
        ) -> *const ClapPlugin,
    >,
}

// SAFETY: the factory we ship is a 'static value with no interior mutability;
// CLAP hosts treat it as read-only.
unsafe impl Sync for ClapPluginFactory {}

#[repr(C)]
pub struct ClapPluginEntry {
    pub clap_version: ClapVersion,
    pub init: Option<unsafe extern "C" fn(plugin_path: *const c_char) -> bool>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub get_factory: Option<unsafe extern "C" fn(factory_id: *const c_char) -> *const c_void>,
}

// SAFETY: the entry struct is static-only and read by the host.
unsafe impl Sync for ClapPluginEntry {}

// ===========================================================================
// Audio-ports extension
// ===========================================================================

pub const CLAP_EXT_AUDIO_PORTS: &core::ffi::CStr = c"clap.audio-ports";
pub const CLAP_PORT_MONO: &core::ffi::CStr = c"mono";
pub const CLAP_PORT_STEREO: &core::ffi::CStr = c"stereo";

pub const CLAP_AUDIO_PORT_IS_MAIN: u32 = 1 << 0;
pub const CLAP_AUDIO_PORT_SUPPORTS_64BITS: u32 = 1 << 1;
pub const CLAP_AUDIO_PORT_PREFERS_64BITS: u32 = 1 << 2;
pub const CLAP_AUDIO_PORT_REQUIRES_COMMON_SAMPLE_SIZE: u32 = 1 << 3;

#[repr(C)]
pub struct ClapAudioPortInfo {
    pub id: ClapId,
    pub name: [c_char; 256],
    pub flags: u32,
    pub channel_count: u32,
    pub port_type: *const c_char,
    pub in_place_pair: ClapId,
}

#[repr(C)]
pub struct ClapPluginAudioPorts {
    pub count: Option<unsafe extern "C" fn(plugin: *const ClapPlugin, is_input: bool) -> u32>,
    pub get: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            index: u32,
            is_input: bool,
            info: *mut ClapAudioPortInfo,
        ) -> bool,
    >,
}

// ===========================================================================
// clap.params extension (host-facing parameter table)
// ===========================================================================

pub const CLAP_EXT_PARAMS: &core::ffi::CStr = c"clap.params";

pub const CLAP_PARAM_IS_STEPPED: u32 = 1 << 0;
pub const CLAP_PARAM_IS_HIDDEN: u32 = 1 << 2;
pub const CLAP_PARAM_IS_BYPASS: u32 = 1 << 4;
pub const CLAP_PARAM_IS_AUTOMATABLE: u32 = 1 << 5;

pub const CLAP_NAME_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// audio-ports-config extension: the declared configuration SET.
// ---------------------------------------------------------------------------

pub const CLAP_EXT_AUDIO_PORTS_CONFIG: &core::ffi::CStr = c"clap.audio-ports-config";

/// One member of the plugin's declared configuration set
/// (clap/ext/audio-ports-config.h).
#[repr(C)]
pub struct ClapAudioPortsConfig {
    pub id: ClapId,
    pub name: [c_char; CLAP_NAME_SIZE],
    pub input_port_count: u32,
    pub output_port_count: u32,
    pub has_main_input: bool,
    pub main_input_channel_count: u32,
    pub main_input_port_type: *const c_char,
    pub has_main_output: bool,
    pub main_output_channel_count: u32,
    pub main_output_port_type: *const c_char,
}

#[repr(C)]
pub struct ClapPluginAudioPortsConfig {
    pub count: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
    pub get: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            index: u32,
            config: *mut ClapAudioPortsConfig,
        ) -> bool,
    >,
    pub select: Option<unsafe extern "C" fn(plugin: *const ClapPlugin, config_id: ClapId) -> bool>,
}

pub const CLAP_PATH_SIZE: usize = 1024;

#[repr(C)]
pub struct ClapParamInfo {
    pub id: ClapId,
    pub flags: u32,
    pub cookie: *mut core::ffi::c_void,
    pub name: [core::ffi::c_char; CLAP_NAME_SIZE],
    pub module: [core::ffi::c_char; CLAP_PATH_SIZE],
    pub min_value: f64,
    pub max_value: f64,
    pub default_value: f64,
}

/// Plugin-side params vtable. CLAP parameter values are PLAIN (unlike
/// VST3's normalized wire values).
#[repr(C)]
pub struct ClapPluginParams {
    pub count: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
    pub get_info: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_index: u32,
            info: *mut ClapParamInfo,
        ) -> bool,
    >,
    pub get_value: Option<
        unsafe extern "C" fn(plugin: *const ClapPlugin, param_id: ClapId, out: *mut f64) -> bool,
    >,
    pub value_to_text: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_id: ClapId,
            value: f64,
            out: *mut core::ffi::c_char,
            size: u32,
        ) -> bool,
    >,
    pub text_to_value: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            param_id: ClapId,
            text: *const core::ffi::c_char,
            out: *mut f64,
        ) -> bool,
    >,
    pub flush: Option<
        unsafe extern "C" fn(
            plugin: *const ClapPlugin,
            in_events: *const ClapInputEvents,
            out_events: *const ClapOutputEvents,
        ),
    >,
}

// ===========================================================================
// clap.state extension + streams
// ===========================================================================

pub const CLAP_EXT_STATE: &core::ffi::CStr = c"clap.state";

#[repr(C)]
pub struct ClapIstream {
    pub ctx: *mut core::ffi::c_void,
    /// Returns bytes read; 0 = end of stream; < 0 = error.
    pub read: Option<
        unsafe extern "C" fn(
            stream: *const ClapIstream,
            buffer: *mut core::ffi::c_void,
            size: u64,
        ) -> i64,
    >,
}

#[repr(C)]
pub struct ClapOstream {
    pub ctx: *mut core::ffi::c_void,
    /// Returns bytes written; < 0 = error.
    pub write: Option<
        unsafe extern "C" fn(
            stream: *const ClapOstream,
            buffer: *const core::ffi::c_void,
            size: u64,
        ) -> i64,
    >,
}

#[repr(C)]
pub struct ClapPluginState {
    pub save:
        Option<unsafe extern "C" fn(plugin: *const ClapPlugin, stream: *const ClapOstream) -> bool>,
    pub load:
        Option<unsafe extern "C" fn(plugin: *const ClapPlugin, stream: *const ClapIstream) -> bool>,
}

/// Gesture bracket event (types CLAP_EVENT_PARAM_GESTURE_BEGIN / _END).
#[repr(C)]
pub struct ClapEventParamGesture {
    pub header: ClapEventHeader,
    pub param_id: ClapId,
}

// ===========================================================================
// clap.tail extension + transport flags
// ===========================================================================

pub const CLAP_EXT_TAIL: &core::ffi::CStr = c"clap.tail";

#[repr(C)]
pub struct ClapPluginTail {
    /// Tail length in samples; u32::MAX = infinite.
    pub get: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
}

/// CLAP fixed-point time: 1 unit = 1/2^31 of a second or beat.
pub const CLAP_SECTIME_UNIT: f64 = 2147483648.0; // 2^31

// ===========================================================================
// clap.latency extension
// ===========================================================================

pub const CLAP_EXT_LATENCY: &core::ffi::CStr = c"clap.latency";

#[repr(C)]
pub struct ClapPluginLatency {
    /// Latency in samples the host should compensate for.
    pub get: Option<unsafe extern "C" fn(plugin: *const ClapPlugin) -> u32>,
}

// ===========================================================================
// clap.log host extension
// ===========================================================================

pub const CLAP_EXT_LOG: &core::ffi::CStr = c"clap.log";

/// `clap_param_rescan_flags`: the parameter values changed (e.g. after a
/// state load); the host re-reads them.
pub const CLAP_PARAM_RESCAN_VALUES: u32 = 1 << 0;

/// The host's `clap.params` (ext/params.h).
#[repr(C)]
pub struct ClapHostParams {
    /// [main-thread]
    pub rescan: Option<unsafe extern "C" fn(host: *const ClapHost, flags: u32)>,
    /// [main-thread]
    pub clear: Option<unsafe extern "C" fn(host: *const ClapHost, param_id: ClapId, flags: u32)>,
    /// [thread-safe, !audio-thread]
    pub request_flush: Option<unsafe extern "C" fn(host: *const ClapHost)>,
}

/// `clap_log_severity`: stave_core's `Severity` discriminants.
pub type ClapLogSeverity = i32;

#[repr(C)]
pub struct ClapHostLog {
    /// [thread-safe]
    pub log: Option<
        unsafe extern "C" fn(
            host: *const ClapHost,
            severity: ClapLogSeverity,
            msg: *const core::ffi::c_char,
        ),
    >,
}

// The C layouts, measured from clap.h (sizeof/_Alignof on
// aarch64-apple-darwin). A mirror that drifts is a memory-safety bug
// against every host, so it fails the build.
const _: () = {
    use core::mem::{align_of, size_of};
    macro_rules! layout {
        ($($t:ty => $size:expr, $align:expr;)*) => {
            $(assert!(size_of::<$t>() == $size && align_of::<$t>() == $align);)*
        };
    }
    layout! {
        ClapVersion => 12, 4;
        ClapHost => 88, 8;
        ClapPluginDescriptor => 88, 8;
        ClapPlugin => 96, 8;
        ClapAudioBuffer => 32, 8;
        ClapProcess => 64, 8;
        ClapEventHeader => 16, 4;
        ClapEventNote => 40, 8;
        ClapEventParamValue => 56, 8;
        ClapEventTransport => 104, 8;
        ClapInputEvents => 24, 8;
        ClapOutputEvents => 16, 8;
        ClapPluginFactory => 24, 8;
        ClapPluginEntry => 40, 8;
        ClapAudioPortInfo => 288, 8;
        ClapPluginAudioPorts => 16, 8;
        ClapAudioPortsConfig => 304, 8;
        ClapPluginAudioPortsConfig => 24, 8;
        ClapParamInfo => 1320, 8;
        ClapPluginParams => 48, 8;
        ClapIstream => 16, 8;
        ClapOstream => 16, 8;
        ClapPluginState => 16, 8;
        ClapEventParamGesture => 20, 4;
        ClapPluginTail => 8, 8;
        ClapPluginLatency => 8, 8;
        ClapHostLog => 8, 8;
        ClapHostParams => 24, 8;
    }
};
