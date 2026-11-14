//! ABI checks for the typed param table over CLAP: the
//! clap.params extension serves the same descriptor table as VST3 —
//! parity is structural — with CLAP's plain-valued wire convention.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract. Miri runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::Arc;

use stave_clap::sys::*;
use stave_core::params::{FloatParam, Params, Range, Smooth, Smoother, Unit};
use stave_core::runtime::Erased;
use stave_core::{Block, Controller, Environment, Info, ParamId, Telemetry};

const ID_GAIN: u32 = 42;
const ID_DRIVE: u32 = 7;

#[derive(stave_core::Params)]
struct TableParams {
    gain: FloatParam,
    drive: FloatParam,
}

impl TableParams {
    fn new() -> Self {
        Self {
            gain: FloatParam::new(ParamId(ID_GAIN), "gain", Range::linear(0.0, 1.0)).default(1.0)
                .smooth(Smooth::Off),
            drive: FloatParam::new(ParamId(ID_DRIVE), "drive", Range::linear(-12.0, 36.0))
            .unit(Unit::Db)
            .group("Main"),
        }
    }
}

struct TableGain {
    params: Arc<TableParams>,
    gain: Smoother,
}

static SEEN_TEMPO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SEEN_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Erased for TableGain {
    const INFO: Info = Info {
        id: "com.stave.test.table-gain-clap",
        name: "Table Gain (CLAP)",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "param-table ABI fixture",
        features: &[],
        au: None,
    };

    fn new(_telemetry: &Telemetry) -> Self {
        let params = Arc::new(TableParams::new());
        Self {
            gain: Smoother::new(&params.gain),
            params,
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.gain.prepare(env);
    }

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(&mut self, block: &mut Block<'_>) {
        if let Some(t) = block.transport() {
            SEEN_PLAYING.store(t.playing, std::sync::atomic::Ordering::Relaxed);
            SEEN_TEMPO.store(
                t.tempo_bpm.unwrap_or(f64::NAN).to_bits(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        let mut gain = self.gain.ramp(block, &self.params.gain); // Off → schedule-exact
        let frames = block.frames();
        let (input, output) = block.io();
        let mut channels: Vec<_> = input.channels().zip(output.channels_mut()).collect();
        for f in 0..frames {
            let g = gain.next();
            for (x, y) in channels.iter_mut() {
                y[f] = g * x[f];
            }
        }
    }
}

/// A face, so the instance serves `clap.gui`.
impl stave_clap::wrapper::ClapExport for TableGain {
    fn face(&self) -> Option<stave_clap::stave_face::Face> {
        use stave_clap::stave_face::prelude::*;
        Some(Face::new(Theme::graphite(), Knob::new(self.params.drive.bind()).node()))
    }
}

stave_clap::export_clap!(TableGain);

// ===========================================================================
// Host plumbing
// ===========================================================================

unsafe fn create() -> *const ClapPlugin {
    let get_factory = clap_entry.get_factory.expect("get_factory");
    let factory =
        unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const ClapPluginFactory;
    let create = unsafe { (*factory).create_plugin.expect("create_plugin") };
    let plugin = unsafe { create(factory, ptr::null(), c"com.stave.test.table-gain-clap".as_ptr()) };
    assert!(!plugin.is_null());
    assert!(unsafe { ((*plugin).init.expect("init"))(plugin) });
    plugin
}

unsafe fn params_ext(plugin: *const ClapPlugin) -> &'static ClapPluginParams {
    let get_ext = unsafe { (*plugin).get_extension.expect("get_extension") };
    let ext = unsafe { get_ext(plugin, CLAP_EXT_PARAMS.as_ptr()) };
    assert!(!ext.is_null(), "clap.params extension must be served");
    unsafe { &*(ext as *const ClapPluginParams) }
}

// ===========================================================================
// Checks
// ===========================================================================

#[test]
fn params_extension_serves_the_table_plain_valued() {
    unsafe {
        let plugin = create();
        let params = params_ext(plugin);
        assert_eq!((params.count.unwrap())(plugin), 2);

        let mut info: ClapParamInfo = std::mem::zeroed();
        assert!((params.get_info.unwrap())(plugin, 1, &mut info));
        assert_eq!(info.id, ID_DRIVE, "stable id, not index");
        assert_ne!(info.flags & CLAP_PARAM_IS_AUTOMATABLE, 0);
        // CLAP wire values are PLAIN.
        assert_eq!(info.min_value, -12.0);
        assert_eq!(info.max_value, 36.0);
        assert_eq!(info.default_value, 0.0);
        let name = CStr::from_ptr(info.name.as_ptr()).to_str().unwrap();
        assert_eq!(name, "drive");
        let module = CStr::from_ptr(info.module.as_ptr()).to_str().unwrap();
        assert_eq!(module, "Main");

        let mut v = f64::NAN;
        assert!((params.get_value.unwrap())(plugin, ID_GAIN, &mut v));
        assert_eq!(v, 1.0);

        // value_to_text / text_to_value round the same Unit grammar.
        let mut buf = [0 as c_char; 64];
        assert!((params.value_to_text.unwrap())(
            plugin,
            ID_DRIVE,
            12.0,
            buf.as_mut_ptr(),
            64
        ));
        assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "12.0 dB");
        let text = c"-6 dB";
        let mut parsed = f64::NAN;
        assert!((params.text_to_value.unwrap())(
            plugin,
            ID_DRIVE,
            text.as_ptr(),
            &mut parsed
        ));
        assert_eq!(parsed, -6.0);

        ((*plugin).destroy.expect("destroy"))(plugin);
    }
}

// ===========================================================================
// clap.state round trip
// ===========================================================================

unsafe extern "C" fn os_write(stream: *const ClapOstream, buffer: *const c_void, size: u64) -> i64 {
    let data = unsafe { &mut *((*stream).ctx as *mut Vec<u8>) };
    let bytes = unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), size as usize) };
    data.extend_from_slice(bytes);
    size as i64
}

struct ReadCtx {
    data: Vec<u8>,
    pos: usize,
}

unsafe extern "C" fn is_read(stream: *const ClapIstream, buffer: *mut c_void, size: u64) -> i64 {
    let ctx = unsafe { &mut *((*stream).ctx as *mut ReadCtx) };
    let take = (size as usize).min(ctx.data.len() - ctx.pos);
    unsafe {
        core::ptr::copy_nonoverlapping(ctx.data[ctx.pos..].as_ptr(), buffer.cast::<u8>(), take);
    }
    ctx.pos += take;
    take as i64
}

#[test]
fn clap_state_round_trip_is_bit_exact() {
    unsafe {
        let a = create();
        let params_a = params_ext(a);
        // Set via the params ext (plain values), then save.
        let get_ext = (*a).get_extension.expect("get_extension");
        let state_a = get_ext(a, CLAP_EXT_STATE.as_ptr());
        assert!(!state_a.is_null(), "clap.state must be served");
        let state_a = &*(state_a as *const ClapPluginState);

        // drive to −6 dB via text entry, gain to 0.37 via flush-less
        // set: use text_to_value + a param event through flush.
        let mut captured: Vec<u8> = Vec::new();
        // Directly poke values through the wrapper's own params ext
        // (host-visible path): flush with events would need a list;
        // simpler — text_to_value gives plain, then we verify get_value
        // after load instead.
        let mut drive_plain = f64::NAN;
        assert!((params_a.text_to_value.unwrap())(
            a,
            ID_DRIVE,
            c"-6 dB".as_ptr(),
            &mut drive_plain
        ));
        assert_eq!(drive_plain, -6.0);
        // Apply via flush event.
        let mut ev = MockEvents {
            list: ClapInputEvents {
                ctx: ptr::null_mut(),
                size: Some(me_size),
                get: Some(me_get),
            },
            event: ClapEventParamValue {
                header: ClapEventHeader {
                    size: std::mem::size_of::<ClapEventParamValue>() as u32,
                    time: 0,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    r#type: CLAP_EVENT_PARAM_VALUE,
                    flags: 0,
                },
                param_id: ID_DRIVE,
                cookie: ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value: -6.0,
            },
        };
        (params_a.flush.unwrap())(a, ptr::addr_of!(ev).cast(), ptr::null());
        ev.event.param_id = ID_GAIN;
        ev.event.value = 0.37;
        (params_a.flush.unwrap())(a, ptr::addr_of!(ev).cast(), ptr::null());

        let ostream = ClapOstream {
            ctx: ptr::addr_of_mut!(captured).cast(),
            write: Some(os_write),
        };
        assert!((state_a.save.unwrap())(a, &ostream));
        assert!(!captured.is_empty());

        // Fresh instance loads the bytes: values bit-exact.
        let b = create();
        let state_b = get_ext(b, CLAP_EXT_STATE.as_ptr());
        let state_b = &*(state_b as *const ClapPluginState);
        let mut read_ctx = ReadCtx {
            data: captured,
            pos: 0,
        };
        let istream = ClapIstream {
            ctx: ptr::addr_of_mut!(read_ctx).cast(),
            read: Some(is_read),
        };
        assert!((state_b.load.unwrap())(b, &istream));

        let params_b = params_ext(b);
        let mut v = f64::NAN;
        assert!((params_b.get_value.unwrap())(b, ID_DRIVE, &mut v));
        assert_eq!(v.to_bits(), (-6.0f64).to_bits());
        assert!((params_b.get_value.unwrap())(b, ID_GAIN, &mut v));
        assert_eq!(v.to_bits(), 0.37f64.to_bits());

        ((*a).destroy.expect("destroy"))(a);
        ((*b).destroy.expect("destroy"))(b);
    }
}

// ===========================================================================
// Mock input-event list carrying one param-value event
// ===========================================================================

#[repr(C)]
struct MockEvents {
    list: ClapInputEvents,
    event: ClapEventParamValue,
}

unsafe extern "C" fn me_size(_l: *const ClapInputEvents) -> u32 {
    1
}
unsafe extern "C" fn me_get(l: *const ClapInputEvents, index: u32) -> *const ClapEventHeader {
    if index != 0 {
        return ptr::null();
    }
    // `list` is the first field of MockEvents. Raw projection, NOT
    // `&(*me).event.header`: a header reference carries 16-byte
    // provenance, and the wrapper reads the whole 56-byte event
    // through this pointer.
    let me = l as *const MockEvents;
    unsafe { ptr::addr_of!((*me).event).cast::<ClapEventHeader>() }
}

// ===========================================================================
// Outbound gestures + sample-accurate events
// ===========================================================================

unsafe extern "C" fn oc_push(list: *const ClapOutputEvents, event: *const ClapEventHeader) -> bool {
    // ctx points at the collecting Vec.
    let events = unsafe { &mut *((*list).ctx as *mut Vec<(u16, u32, f64)>) };
    let header = unsafe { *event };
    match header.r#type {
        CLAP_EVENT_PARAM_VALUE => {
            let ev = unsafe { &*(event as *const ClapEventParamValue) };
            events.push((header.r#type, ev.param_id, ev.value));
        }
        CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
            let ev = unsafe { &*(event as *const ClapEventParamGesture) };
            events.push((header.r#type, ev.param_id, f64::NAN));
        }
        _ => {}
    }
    true
}

/// A face edit (through the controller `gui.create` gives the view)
/// reaches the host as gesture-begin, values, gesture-end on the next
/// flush, and the table holds the value at once.
#[test]
fn gestures_flow_to_the_host_through_out_events() {
    unsafe {
        let plugin = create();
        // GUI thread: a knob drag on drive.
        let controller = stave_clap::wrapper::controller::<TableGain>(plugin);
        let c: &dyn Controller = &controller;
        let drive = ParamId(ID_DRIVE);
        c.begin(drive);
        c.set(drive, stave_core::Normalized::new(0.5)); // 12 dB on −12..36
        c.set(drive, stave_core::Normalized::new(0.125)); // −6 dB
        c.end(drive);

        // Host flushes (transport stopped): events arrive in order.
        let mut events: Vec<(u16, u32, f64)> = Vec::new();
        let collector = ClapOutputEvents {
            ctx: std::ptr::addr_of_mut!(events).cast(),
            try_push: Some(oc_push),
        };
        let params = params_ext(plugin);
        (params.flush.unwrap())(plugin, ptr::null(), &collector);

        assert_eq!(events.len(), 4);
        assert_eq!(events[0].0, CLAP_EVENT_PARAM_GESTURE_BEGIN);
        assert_eq!(events[0].1, ID_DRIVE);
        assert_eq!(
            (events[1].0, events[1].1, events[1].2),
            (CLAP_EVENT_PARAM_VALUE, ID_DRIVE, 12.0)
        );
        assert_eq!(
            (events[2].0, events[2].1, events[2].2),
            (CLAP_EVENT_PARAM_VALUE, ID_DRIVE, -6.0)
        );
        assert_eq!(events[3].0, CLAP_EVENT_PARAM_GESTURE_END);

        // Table holds the final value (GUI truth applied immediately).
        let mut v = f64::NAN;
        assert!((params.get_value.unwrap())(plugin, ID_DRIVE, &mut v));
        assert_eq!(v, -6.0);

        ((*plugin).destroy.expect("destroy"))(plugin);
    }
}

/// The one window API a face embeds in on this platform.
#[cfg(target_os = "macos")]
const API: &CStr = CLAP_WINDOW_API_COCOA;
#[cfg(windows)]
const API: &CStr = CLAP_WINDOW_API_WIN32;
#[cfg(all(unix, not(target_os = "macos")))]
const API: &CStr = CLAP_WINDOW_API_X11;

/// `clap.gui`: an embedded view of the face's own size in this
/// platform's window API (headless: nothing touches the window system
/// until `set_parent`); an effect offers no note ports.
#[test]
fn the_face_is_served_as_an_embedded_gui() {
    unsafe {
        let plugin = create();
        let get_ext = (*plugin).get_extension.expect("get_extension");
        assert!(get_ext(plugin, CLAP_EXT_NOTE_PORTS.as_ptr()).is_null(), "an effect takes no notes");
        let gui = get_ext(plugin, CLAP_EXT_GUI.as_ptr()).cast::<ClapPluginGui>();
        assert!(!gui.is_null(), "a plugin with a face serves clap.gui");
        let gui = &*gui;
        let api = API.as_ptr();
        assert!((gui.is_api_supported.unwrap())(plugin, api, false));
        assert!(!(gui.is_api_supported.unwrap())(plugin, api, true), "never floating");
        for other in [CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, c"wayland"] {
            if other != API {
                assert!(!(gui.is_api_supported.unwrap())(plugin, other.as_ptr(), false), "{other:?}");
            }
        }
        let (mut preferred, mut floating) = (ptr::null(), true);
        assert!((gui.get_preferred_api.unwrap())(plugin, &mut preferred, &mut floating));
        assert_eq!((CStr::from_ptr(preferred), floating), (API, false));

        let (mut w, mut h) = (0u32, 0u32);
        assert!(!(gui.get_size.unwrap())(plugin, &mut w, &mut h), "no size before create");
        assert!((gui.create.unwrap())(plugin, api, false));
        assert!((gui.get_size.unwrap())(plugin, &mut w, &mut h));
        assert!(w > 0 && h > 0, "{w}×{h}");
        assert!(!(gui.get_size.unwrap())(plugin, ptr::null_mut(), &mut h));
        assert!(!(gui.can_resize.unwrap())(plugin));
        assert!((gui.set_size.unwrap())(plugin, w, h), "its own size");
        assert!(!(gui.set_size.unwrap())(plugin, w + 1, h));
        assert!(!(gui.set_parent.unwrap())(plugin, ptr::null()));
        if cfg!(not(target_os = "macos")) {
            // Windows and X11 sizes are device pixels: a host's scale
            // doubles them.
            assert!((gui.set_scale.unwrap())(plugin, 2.0));
            let (mut w2, mut h2) = (0u32, 0u32);
            assert!((gui.get_size.unwrap())(plugin, &mut w2, &mut h2));
            assert!(w2.abs_diff(2 * w) <= 1 && h2.abs_diff(2 * h) <= 1, "{w}×{h} at 2× is {w2}×{h2}");
            assert!(!(gui.set_scale.unwrap())(plugin, f64::NAN));
        }
        (gui.destroy.unwrap())(plugin);
        assert!(!(gui.get_size.unwrap())(plugin, &mut w, &mut h), "destroyed");

        ((*plugin).destroy.expect("destroy"))(plugin);
    }
}

#[test]
fn param_value_events_reach_table_and_dsp() {
    unsafe {
        let plugin = create();
        assert!(((*plugin).activate.expect("activate"))(
            plugin, 48_000.0, 1, 64
        ));

        let mut events = MockEvents {
            list: ClapInputEvents {
                ctx: ptr::null_mut(),
                size: Some(me_size),
                get: Some(me_get),
            },
            event: ClapEventParamValue {
                header: ClapEventHeader {
                    size: std::mem::size_of::<ClapEventParamValue>() as u32,
                    time: 32, // sample-accurate: lands mid-block
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    r#type: CLAP_EVENT_PARAM_VALUE,
                    flags: 0,
                },
                param_id: ID_GAIN,
                cookie: ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value: 0.25, // plain
            },
        };

        let mut in_l = vec![1.0f32; 64];
        let mut in_r = vec![1.0f32; 64];
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let in_buf = ClapAudioBuffer {
            data32: ins.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut out_buf = ClapAudioBuffer {
            data32: outs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        // Whole-struct pointer, NOT `&events.list`: a first-field
        // reference carries field-only provenance, and me_get reads
        // the sibling `event` field through it — UB that Miri catches.
        let events_ptr = ptr::addr_of!(events).cast::<ClapInputEvents>();
        // Transport delivery rides along: tempo flagged
        // valid, beats/seconds not — validity must be honored.
        let transport = ClapEventTransport {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventTransport>() as u32,
                time: 0,
                space_id: CLAP_CORE_EVENT_SPACE_ID,
                r#type: 0,
                flags: 0,
            },
            flags: CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_IS_PLAYING,
            song_pos_beats: 0,
            song_pos_seconds: 0,
            tempo: 174.0,
            tempo_inc: 0.0,
            loop_start_beats: 0,
            loop_end_beats: 0,
            loop_start_seconds: 0,
            loop_end_seconds: 0,
            bar_start: 0,
            bar_number: 0,
            tsig_num: 4,
            tsig_denom: 4,
        };
        let process = ClapProcess {
            steady_time: -1,
            frames_count: 64,
            transport: &transport,
            audio_inputs: &in_buf,
            audio_outputs: &mut out_buf,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: events_ptr,
            out_events: ptr::null(),
        };
        let f = (*plugin).process.expect("process");
        assert_eq!(f(plugin, &process), CLAP_PROCESS_CONTINUE);
        assert_eq!(out_l[31], 1.0, "pre-offset: previous gain");
        assert_eq!(out_l[32], 0.25, "event lands exactly at header.time");
        assert_eq!(out_l[63], 0.25, "event value reached the DSP");
        assert!(SEEN_PLAYING.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(
            f64::from_bits(SEEN_TEMPO.load(std::sync::atomic::Ordering::Relaxed)),
            174.0,
            "tempo delivered through Block::transport"
        );

        // flush path applies events outside processing too.
        events.event.value = 0.5;
        let params = params_ext(plugin);
        (params.flush.unwrap())(plugin, ptr::addr_of!(events).cast(), ptr::null());
        let mut v = f64::NAN;
        assert!((params.get_value.unwrap())(plugin, ID_GAIN, &mut v));
        assert_eq!(v, 0.5);

        ((*plugin).destroy.expect("destroy"))(plugin);
    }
}
