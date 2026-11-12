//! ABI checks for the typed param table, through the real
//! vtable path:
//! - wire ids are the author's STABLE ids, independent of index order
//! - real value strings (getParamStringByValue / valueByString)
//! - host automation queues drain into the table
//! - two instances isolated with ZERO shared registration machinery

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract. Miri
// runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use stave_core::params::{FloatParam, Params, Range, Smooth, Smoother, Unit};
use stave_core::runtime::Erased;
use stave_core::{Block, Environment, Info, Telemetry};
use stave_vst3::sys::*;
use stave_vst3::stave_face;
use stave_vst3::wrapper::FactoryObj;

// Deliberately non-contiguous, non-ordered ids: the wire must carry
// THESE, never indices.
const ID_GAIN: u32 = 42;
const ID_DRIVE: u32 = 7;
const ID_POWER: u32 = 99;
const ID_SGAIN: u32 = 55;

#[derive(stave_core::Params)]
struct TableParams {
    gain: FloatParam,
    drive: FloatParam,
    power: stave_core::params::BoolParam,
    /// Smoothed multiplier — the ramp-fidelity fixture.
    sgain: FloatParam,
}

impl TableParams {
    fn new() -> Self {
        Self {
            gain: FloatParam::new(stave_core::ParamId(ID_GAIN), "gain", Range::linear(0.0, 1.0)).default(1.0)
                .smooth(Smooth::Off),
            drive: FloatParam::new(stave_core::ParamId(ID_DRIVE), "drive", Range::linear(-12.0, 36.0))
            .unit(Unit::Db)
            .group("Main"),
            // VST3 convention: bypass param TRUE = bypassed.
            power: stave_core::params::BoolParam::bypass(stave_core::ParamId(ID_POWER)),
            sgain: FloatParam::new(stave_core::ParamId(ID_SGAIN), "sgain", Range::linear(0.0, 1.0)).default(1.0),
        }
    }
}

struct TableGain {
    params: Arc<TableParams>,
    gain: Smoother,
    sgain: Smoother,
}

// Transport observation point for the transport-delivery check (single
// fixture per test binary; statics are fine).
static SEEN_TEMPO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SEEN_SECONDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SEEN_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Erased for TableGain {
    const INFO: Info = Info {
        id: "com.stave.test.table-gain",
        name: "Table Gain",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "param-table ABI fixture",
        features: &[],
    };

    fn new(_telemetry: &Telemetry) -> Self {
        let params = Arc::new(TableParams::new());
        Self {
            gain: Smoother::new(&params.gain),
            sgain: Smoother::new(&params.sgain),
            params,
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.gain.prepare(env);
        self.sgain.prepare(env);
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
            SEEN_SECONDS.store(
                t.song_pos_seconds.unwrap_or(f64::NAN).to_bits(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        let mut gain = self.gain.ramp(block, &self.params.gain); // Off → follows schedule exactly
        let mut sg = self.sgain.ramp(block, &self.params.sgain);
        let frames = block.frames();
        let (input, output) = block.io();
        let mut channels: Vec<_> = input.channels().zip(output.channels_mut()).collect();
        for f in 0..frames {
            let g = gain.next() * sg.next();
            for (x, y) in channels.iter_mut() {
                y[f] = g * x[f];
            }
        }
    }
}

/// A face, so `createView` makes a view.
impl stave_vst3::wrapper::Vst3Export for TableGain {
    fn face(&self) -> Option<stave_face::Face> {
        use stave_face::prelude::*;
        Some(Face::new(Theme::graphite(), Knob::new(self.params.drive.bind()).node()))
    }
}

stave_vst3::export_vst3!(TableGain);

// ===========================================================================
// Host plumbing (mirrors wrapper_abi.rs)
// ===========================================================================

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

struct HostInstance {
    component: *mut FUnknown,
    processor: *mut FUnknown,
    controller: *mut FUnknown,
}

unsafe fn create_instance() -> HostInstance {
    let factory = GetPluginFactory() as *mut FactoryObj;
    let fvtbl = unsafe { &*(*factory).vtbl };
    let cid = <TableGain as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
    let mut obj: *mut c_void = ptr::null_mut();
    let res = unsafe {
        (fvtbl.create_instance)(
            factory as *mut FUnknown,
            cid.as_ptr() as FidString,
            IID_ICOMPONENT.as_ptr() as FidString,
            &mut obj,
        )
    };
    assert_eq!(res, K_RESULT_OK);
    let component = obj as *mut FUnknown;
    let qi = unsafe { (*(*(component as *mut RawObj<IComponentVtbl>)).vtbl).query_interface };
    let mut proc_obj: *mut c_void = ptr::null_mut();
    assert_eq!(
        unsafe { qi(component, &IID_IAUDIO_PROCESSOR, &mut proc_obj) },
        K_RESULT_OK
    );
    let mut ctl_obj: *mut c_void = ptr::null_mut();
    assert_eq!(
        unsafe { qi(component, &IID_IEDIT_CONTROLLER, &mut ctl_obj) },
        K_RESULT_OK
    );
    HostInstance {
        component,
        processor: proc_obj as *mut FUnknown,
        controller: ctl_obj as *mut FUnknown,
    }
}

impl HostInstance {
    unsafe fn comp_vtbl(&self) -> &'static IComponentVtbl {
        unsafe { &*(*(self.component as *mut RawObj<IComponentVtbl>)).vtbl }
    }
    unsafe fn proc_vtbl(&self) -> &'static IAudioProcessorVtbl {
        unsafe { &*(*(self.processor as *mut RawObj<IAudioProcessorVtbl>)).vtbl }
    }
    unsafe fn ctl_vtbl(&self) -> &'static IEditControllerVtbl {
        unsafe { &*(*(self.controller as *mut RawObj<IEditControllerVtbl>)).vtbl }
    }
    unsafe fn setup(&self) {
        let mut setup = ProcessSetup {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: 64,
            sample_rate: 48_000.0,
        };
        assert_eq!(
            unsafe { (self.proc_vtbl().setup_processing)(self.processor, &mut setup) },
            K_RESULT_OK
        );
        assert_eq!(
            unsafe { (self.comp_vtbl().set_active)(self.component, 1) },
            K_RESULT_OK
        );
    }
    unsafe fn process_block(
        &self,
        in_ptrs: &mut [*mut f32],
        out_ptrs: &mut [*mut f32],
        frames: usize,
    ) {
        let mut in_bus = AudioBusBuffers {
            num_channels: in_ptrs.len() as i32,
            silence_flags: 0,
            channel_buffers_32: in_ptrs.as_mut_ptr(),
        };
        let mut out_bus = AudioBusBuffers {
            num_channels: out_ptrs.len() as i32,
            silence_flags: 0,
            channel_buffers_32: out_ptrs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: frames as i32,
            num_inputs: 1,
            num_outputs: 1,
            inputs: &mut in_bus,
            outputs: &mut out_bus,
            input_parameter_changes: ptr::null_mut(),
            output_parameter_changes: ptr::null_mut(),
            input_events: ptr::null_mut(),
            output_events: ptr::null_mut(),
            process_context: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { (self.proc_vtbl().process)(self.processor, &mut data) },
            K_RESULT_OK
        );
    }

    unsafe fn release(self) {
        unsafe {
            let release = self.comp_vtbl().release;
            release(self.processor);
            release(self.controller);
            release(self.component);
        }
    }
}

const FRAMES: usize = 64;

fn test_signal(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2654435761).max(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

fn utf16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ===========================================================================
// Mock IParameterChanges (host-side automation queue)
// ===========================================================================

#[repr(C)]
struct MockQueue {
    vtbl: *const IParamValueQueueVtbl,
    id: u32,
    points: Vec<(i32, f64)>,
}

unsafe extern "C" fn mq_qi(
    _t: *mut IParamValueQueue,
    _i: *const Tuid,
    _o: *mut *mut c_void,
) -> Tresult {
    K_NO_INTERFACE
}
unsafe extern "C" fn mq_ref(_t: *mut IParamValueQueue) -> u32 {
    1
}
unsafe extern "C" fn mq_id(t: *mut IParamValueQueue) -> ParamId {
    unsafe { (*(t as *mut MockQueue)).id }
}
unsafe extern "C" fn mq_count(t: *mut IParamValueQueue) -> i32 {
    unsafe { (*(t as *mut MockQueue)).points.len() as i32 }
}
unsafe extern "C" fn mq_get(
    t: *mut IParamValueQueue,
    index: i32,
    offset: *mut i32,
    value: *mut ParamValue,
) -> Tresult {
    let q = unsafe { &*(t as *mut MockQueue) };
    let Some(&(o, v)) = q.points.get(index as usize) else {
        return K_INVALID_ARGUMENT;
    };
    unsafe {
        *offset = o;
        *value = v;
    }
    K_RESULT_OK
}
unsafe extern "C" fn mq_add(
    _t: *mut IParamValueQueue,
    _o: i32,
    _v: ParamValue,
    _i: *mut i32,
) -> Tresult {
    K_NOT_IMPLEMENTED
}

static MQ_VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
    query_interface: mq_qi,
    add_ref: mq_ref,
    release: mq_ref,
    get_parameter_id: mq_id,
    get_point_count: mq_count,
    get_point: mq_get,
    add_point: mq_add,
};

#[repr(C)]
struct MockChanges {
    vtbl: *const IParameterChangesVtbl,
    queues: Vec<*mut MockQueue>,
}

unsafe extern "C" fn mc_qi(
    _t: *mut IParameterChanges,
    _i: *const Tuid,
    _o: *mut *mut c_void,
) -> Tresult {
    K_NO_INTERFACE
}
unsafe extern "C" fn mc_ref(_t: *mut IParameterChanges) -> u32 {
    1
}
unsafe extern "C" fn mc_count(t: *mut IParameterChanges) -> i32 {
    unsafe { (*(t as *mut MockChanges)).queues.len() as i32 }
}
unsafe extern "C" fn mc_data(t: *mut IParameterChanges, index: i32) -> *mut IParamValueQueue {
    let c = unsafe { &*(t as *mut MockChanges) };
    c.queues
        .get(index as usize)
        .map_or(ptr::null_mut(), |&q| q as *mut IParamValueQueue)
}
unsafe extern "C" fn mc_add(
    _t: *mut IParameterChanges,
    _id: *const ParamId,
    _i: *mut i32,
) -> *mut IParamValueQueue {
    ptr::null_mut()
}

static MC_VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
    query_interface: mc_qi,
    add_ref: mc_ref,
    release: mc_ref,
    get_parameter_count: mc_count,
    get_parameter_data: mc_data,
    add_parameter_data: mc_add,
};

// ===========================================================================
// Checks
// ===========================================================================

#[test]
fn wire_ids_are_stable_declared_ids() {
    unsafe {
        let inst = create_instance();
        let ctl = inst.ctl_vtbl();
        assert_eq!((ctl.get_parameter_count)(inst.controller), 4);
        let mut ids = Vec::new();
        for i in 0..4 {
            let mut info: ParameterInfo = std::mem::zeroed();
            assert_eq!(
                (ctl.get_parameter_info)(inst.controller, i, &mut info),
                K_RESULT_OK
            );
            ids.push(info.id);
            if info.id == ID_POWER {
                assert_ne!(info.flags & (1 << 16), 0, "bypass flag set");
                assert_eq!(info.step_count, 1, "toggle = 1 step");
            }
            if info.id == ID_DRIVE {
                assert_eq!(utf16_to_string(&info.units), "dB");
                assert_ne!(info.flags & 1, 0, "kCanAutomate");
            }
        }
        assert_eq!(
            ids,
            vec![ID_GAIN, ID_DRIVE, ID_POWER, ID_SGAIN],
            "wire ids = declared ids, not indices"
        );
        inst.release();
    }
}

/// Descriptor snapshot: reviewed once,
/// frozen. Any change to this string is a WIRE-VISIBLE change —
/// deliberate, reviewed, and never a side effect.
#[test]
fn descriptor_snapshot_is_frozen() {
    unsafe {
        let inst = create_instance();
        let ctl = inst.ctl_vtbl();
        let n = (ctl.get_parameter_count)(inst.controller);
        let mut snap = String::new();
        for i in 0..n {
            let mut info: ParameterInfo = std::mem::zeroed();
            assert_eq!(
                (ctl.get_parameter_info)(inst.controller, i, &mut info),
                K_RESULT_OK
            );
            use std::fmt::Write as _;
            let _ = write!(
                snap,
                "{}:{} unit='{}' steps={} flags={:#x} default={};",
                info.id,
                utf16_to_string(&info.title),
                utf16_to_string(&info.units),
                info.step_count,
                info.flags,
                info.default_normalized_value,
            );
        }
        assert_eq!(
            snap,
            "42:gain unit='' steps=0 flags=0x1 default=1;\
             7:Main · drive unit='dB' steps=0 flags=0x1 default=0.25;\
             99:Bypass unit='' steps=1 flags=0x10001 default=0;\
             55:sgain unit='' steps=0 flags=0x1 default=1;",
            "wire descriptor changed — if deliberate, update this snapshot in the same review"
        );
        inst.release();
    }
}

#[test]
fn value_strings_and_typed_entry() {
    unsafe {
        let inst = create_instance();
        let ctl = inst.ctl_vtbl();
        // 0.5 normalized on -12..36 = 12 dB.
        let mut buf = [0u16; 128];
        assert_eq!(
            (ctl.get_param_string_by_value)(inst.controller, ID_DRIVE, 0.5, buf.as_mut_ptr()),
            K_RESULT_OK
        );
        assert_eq!(utf16_to_string(&buf), "12.0 dB");
        // Typed entry: "-6 dB" → normalized (−6−(−12))/48 = 0.125.
        let mut text: Vec<u16> = "-6 dB".encode_utf16().chain([0]).collect();
        let mut norm: ParamValue = -1.0;
        assert_eq!(
            (ctl.get_param_value_by_string)(
                inst.controller,
                ID_DRIVE,
                text.as_mut_ptr(),
                &mut norm
            ),
            K_RESULT_OK
        );
        assert!((norm - 0.125).abs() < 1e-12, "{norm}");
        inst.release();
    }
}

#[test]
fn automation_queue_drains_into_dsp() {
    unsafe {
        let inst = create_instance();
        inst.setup();

        let mut in_l = vec![1.0f32; 64];
        let mut in_r = vec![1.0f32; 64];
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];

        let mut queue = MockQueue {
            vtbl: &MQ_VTBL,
            id: ID_GAIN,
            points: vec![(0, 0.25)],
        };
        let mut changes = MockChanges {
            vtbl: &MC_VTBL,
            queues: vec![&mut queue],
        };

        let mut in_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: [in_l.as_mut_ptr(), in_r.as_mut_ptr()].as_mut_ptr(),
        };
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        in_bus.channel_buffers_32 = ins.as_mut_ptr();
        let mut out_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: outs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: 64,
            num_inputs: 1,
            num_outputs: 1,
            inputs: &mut in_bus,
            outputs: &mut out_bus,
            input_parameter_changes: &mut changes as *mut MockChanges as *mut IParameterChanges,
            output_parameter_changes: ptr::null_mut(),
            input_events: ptr::null_mut(),
            output_events: ptr::null_mut(),
            process_context: ptr::null_mut(),
        };
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_RESULT_OK
        );
        // gain param is Smooth::Off: the queued 0.25 applies this block.
        assert_eq!(out_l[63], 0.25, "queued automation reached the DSP");
        // And the controller reads the same truth back.
        let got = (inst.ctl_vtbl().get_param_normalized)(inst.controller, ID_GAIN);
        assert_eq!(got, 0.25);
        inst.release();
    }
}

// ===========================================================================
// Mock IBStream (Vec-backed host stream)
// ===========================================================================

#[repr(C)]
struct MockStream {
    vtbl: *const IBStreamVtbl,
    data: Vec<u8>,
    pos: usize,
}

impl MockStream {
    fn new() -> Self {
        Self {
            vtbl: &MS_VTBL,
            data: Vec::new(),
            pos: 0,
        }
    }
    fn as_funknown(&mut self) -> *mut FUnknown {
        ptr::addr_of_mut!(*self).cast()
    }
}

unsafe extern "C" fn ms_qi(_t: *mut IBStream, _i: *const Tuid, _o: *mut *mut c_void) -> Tresult {
    K_NO_INTERFACE
}
unsafe extern "C" fn ms_ref(_t: *mut IBStream) -> u32 {
    1
}
unsafe extern "C" fn ms_read(
    t: *mut IBStream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_read: *mut i32,
) -> Tresult {
    let s = unsafe { &mut *(t as *mut MockStream) };
    let take = (num_bytes as usize).min(s.data.len() - s.pos);
    unsafe {
        ptr::copy_nonoverlapping(s.data[s.pos..].as_ptr(), buffer.cast::<u8>(), take);
        if !num_read.is_null() {
            *num_read = take as i32;
        }
    }
    s.pos += take;
    K_RESULT_OK
}
unsafe extern "C" fn ms_write(
    t: *mut IBStream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_written: *mut i32,
) -> Tresult {
    let s = unsafe { &mut *(t as *mut MockStream) };
    let bytes = unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), num_bytes as usize) };
    s.data.extend_from_slice(bytes);
    unsafe {
        if !num_written.is_null() {
            *num_written = num_bytes;
        }
    }
    K_RESULT_OK
}
unsafe extern "C" fn ms_seek(t: *mut IBStream, pos: i64, mode: i32, result: *mut i64) -> Tresult {
    let s = unsafe { &mut *(t as *mut MockStream) };
    let base = match mode {
        SEEK_SET => 0i64,
        SEEK_CUR => s.pos as i64,
        _ => s.data.len() as i64,
    };
    s.pos = (base + pos).clamp(0, s.data.len() as i64) as usize;
    unsafe {
        if !result.is_null() {
            *result = s.pos as i64;
        }
    }
    K_RESULT_OK
}
unsafe extern "C" fn ms_tell(t: *mut IBStream, pos: *mut i64) -> Tresult {
    let s = unsafe { &*(t as *const MockStream) };
    unsafe { *pos = s.pos as i64 };
    K_RESULT_OK
}

static MS_VTBL: IBStreamVtbl = IBStreamVtbl {
    query_interface: ms_qi,
    add_ref: ms_ref,
    release: ms_ref,
    read: ms_read,
    write: ms_write,
    seek: ms_seek,
    tell: ms_tell,
};

/// Session save → fresh instance → load → bit-exact
/// params AND bit-exact render, through the real get/setState ABI.
#[test]
fn state_round_trip_is_bit_exact() {
    unsafe {
        let a = create_instance();
        a.setup();
        let ca = a.ctl_vtbl();
        assert_eq!(
            (ca.set_param_normalized)(a.controller, ID_GAIN, 0.37),
            K_RESULT_OK
        );
        assert_eq!(
            (ca.set_param_normalized)(a.controller, ID_DRIVE, 0.81),
            K_RESULT_OK
        );

        let mut stream = MockStream::new();
        assert_eq!(
            (a.comp_vtbl().get_state)(a.component, stream.as_funknown()),
            K_RESULT_OK
        );
        assert!(!stream.data.is_empty());

        let b = create_instance();
        b.setup();
        stream.pos = 0;
        assert_eq!(
            (b.comp_vtbl().set_state)(b.component, stream.as_funknown()),
            K_RESULT_OK
        );
        let cb = b.ctl_vtbl();
        // Params identical to the bit (plain stored, no normalized trip).
        assert_eq!(
            (ca.get_param_normalized)(a.controller, ID_GAIN).to_bits(),
            (cb.get_param_normalized)(b.controller, ID_GAIN).to_bits()
        );
        assert_eq!(
            (ca.get_param_normalized)(a.controller, ID_DRIVE).to_bits(),
            (cb.get_param_normalized)(b.controller, ID_DRIVE).to_bits()
        );

        // Render null: both instances produce identical output.
        let sig = test_signal(FRAMES, 3);
        let render = |inst: &HostInstance| -> Vec<f32> {
            let mut in_l = sig.clone();
            let mut in_r = sig.clone();
            let mut out_l = vec![0.0f32; FRAMES];
            let mut out_r = vec![0.0f32; FRAMES];
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            inst.process_block(&mut ins, &mut outs, FRAMES);
            out_l
        };
        let out_a = render(&a);
        let out_b = render(&b);
        for (i, (x, y)) in out_a.iter().zip(&out_b).enumerate() {
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "sample {i} differs after state reload"
            );
        }

        // setComponentState (controller side of the fused object) is
        // idempotent over the same chunk.
        stream.pos = 0;
        assert_eq!(
            (cb.set_component_state)(b.controller, stream.as_funknown()),
            K_RESULT_OK
        );
        assert_eq!(
            (cb.get_param_normalized)(b.controller, ID_GAIN).to_bits(),
            0.37f64.to_bits()
        );

        a.release();
        b.release();
    }
}

/// Corrupt chunks are refused and leave state untouched.
#[test]
fn corrupt_state_is_refused_cleanly() {
    unsafe {
        let a = create_instance();
        a.setup();
        let ca = a.ctl_vtbl();
        assert_eq!(
            (ca.set_param_normalized)(a.controller, ID_GAIN, 0.6),
            K_RESULT_OK
        );
        let mut garbage = MockStream::new();
        garbage.data = b"definitely not a STVE chunk".to_vec();
        assert_eq!(
            (a.comp_vtbl().set_state)(a.component, garbage.as_funknown()),
            K_RESULT_FALSE
        );
        assert_eq!((ca.get_param_normalized)(a.controller, ID_GAIN), 0.6);
        a.release();
    }
}

/// A set-point at offset 173 lands at sample 173, not at
/// the block edge. `gain` is Smooth::Off, so the step is exact.
#[test]
fn scheduled_point_lands_within_one_sample() {
    unsafe {
        let inst = create_instance();
        let mut setup = ProcessSetup {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: 512,
            sample_rate: 48_000.0,
        };
        assert_eq!(
            (inst.proc_vtbl().setup_processing)(inst.processor, &mut setup),
            K_RESULT_OK
        );
        assert_eq!(
            (inst.comp_vtbl().set_active)(inst.component, 1),
            K_RESULT_OK
        );

        let mut queue = MockQueue {
            vtbl: &MQ_VTBL,
            id: ID_GAIN,
            points: vec![(173, 0.25)],
        };
        let mut changes = MockChanges {
            vtbl: &MC_VTBL,
            queues: vec![&mut queue],
        };

        let mut in_l = vec![1.0f32; 512];
        let mut in_r = vec![1.0f32; 512];
        let mut out_l = vec![0.0f32; 512];
        let mut out_r = vec![0.0f32; 512];
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let mut in_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: ins.as_mut_ptr(),
        };
        let mut out_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: outs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: 512,
            num_inputs: 1,
            num_outputs: 1,
            inputs: &mut in_bus,
            outputs: &mut out_bus,
            input_parameter_changes: &mut changes as *mut MockChanges as *mut IParameterChanges,
            output_parameter_changes: ptr::null_mut(),
            input_events: ptr::null_mut(),
            output_events: ptr::null_mut(),
            process_context: ptr::null_mut(),
        };
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_RESULT_OK
        );
        assert_eq!(out_l[172], 1.0, "pre-offset: previous value");
        assert_eq!(
            out_l[173], 0.25,
            "step lands exactly at the scheduled sample"
        );
        assert_eq!(out_l[511], 0.25);
        inst.release();
    }
}

// ===========================================================================
// Mock IComponentHandler — records the gesture conversation (the face
// tests: faces show on macOS)
// ===========================================================================

#[cfg(target_os = "macos")]
use mock::{MockHandler, MH_VTBL};

#[cfg(target_os = "macos")]
mod mock {
    use super::*;

    #[repr(C)]
    pub(super) struct MockHandler {
        pub(super) vtbl: *const IComponentHandlerVtbl,
        pub(super) log: Vec<String>,
    }

    unsafe extern "C" fn mh_qi(
        _t: *mut IComponentHandler,
        _i: *const Tuid,
        _o: *mut *mut c_void,
    ) -> Tresult {
        K_NO_INTERFACE
    }
    unsafe extern "C" fn mh_ref(_t: *mut IComponentHandler) -> u32 {
        1
    }
    unsafe extern "C" fn mh_begin(t: *mut IComponentHandler, id: ParamId) -> Tresult {
        unsafe { (*(t as *mut MockHandler)).log.push(format!("begin {id}")) };
        K_RESULT_OK
    }
    unsafe extern "C" fn mh_perform(
        t: *mut IComponentHandler,
        id: ParamId,
        value: ParamValue,
    ) -> Tresult {
        unsafe {
            (*(t as *mut MockHandler))
                .log
                .push(format!("perform {id} {value}"))
        };
        K_RESULT_OK
    }
    unsafe extern "C" fn mh_end(t: *mut IComponentHandler, id: ParamId) -> Tresult {
        unsafe { (*(t as *mut MockHandler)).log.push(format!("end {id}")) };
        K_RESULT_OK
    }
    unsafe extern "C" fn mh_restart(_t: *mut IComponentHandler, _flags: i32) -> Tresult {
        K_RESULT_OK
    }

    pub(super) static MH_VTBL: IComponentHandlerVtbl = IComponentHandlerVtbl {
        query_interface: mh_qi,
        add_ref: mh_ref,
        release: mh_ref,
        begin_edit: mh_begin,
        perform_edit: mh_perform,
        end_edit: mh_end,
        restart_component: mh_restart,
    };
}

/// Block-rate host automation through the REAL ABI path
/// sprays < −60 dBc with default smoothing (the params check,
/// re-measured end to end).
#[test]
#[cfg_attr(
    miri,
    ignore = "3-minute interpreted FFT; float-jittered; pointer paths covered by the other tests"
)]
fn abi_automation_ramp_meets_the_zipper_limit() {
    unsafe {
        let inst = create_instance();
        inst.setup(); // 48 k, 64-frame blocks
        const N: usize = 32_768;
        const M: usize = 683;
        let two_pi = 2.0 * std::f64::consts::PI;
        let tau = 0.015 * 48_000.0;
        let trajectory = |i: usize| 0.25 + 0.75 * (1.0 - (-(i as f64) / tau).exp());

        let mut out = Vec::with_capacity(N);
        let mut in_l = vec![0.0f32; 64];
        let mut in_r = vec![0.0f32; 64];
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        for block_start in (0..N).step_by(64) {
            for f in 0..64 {
                let i = block_start + f;
                let s = (two_pi * M as f64 * i as f64 / N as f64).sin() as f32;
                in_l[f] = s;
                in_r[f] = s;
            }
            // Host automation: one point per block on the SMOOTHED param.
            let mut queue = MockQueue {
                vtbl: &MQ_VTBL,
                id: ID_SGAIN,
                points: vec![(0, trajectory(block_start))],
            };
            let mut changes = MockChanges {
                vtbl: &MC_VTBL,
                queues: vec![&mut queue],
            };
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            let mut in_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers_32: ins.as_mut_ptr(),
            };
            let mut out_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers_32: outs.as_mut_ptr(),
            };
            let mut data = ProcessData {
                process_mode: 0,
                symbolic_sample_size: SAMPLE_SIZE_32,
                num_samples: 64,
                num_inputs: 1,
                num_outputs: 1,
                inputs: &mut in_bus,
                outputs: &mut out_bus,
                input_parameter_changes: &mut changes as *mut MockChanges as *mut IParameterChanges,
                output_parameter_changes: ptr::null_mut(),
                input_events: ptr::null_mut(),
                output_events: ptr::null_mut(),
                process_context: ptr::null_mut(),
            };
            assert_eq!(
                (inst.proc_vtbl().process)(inst.processor, &mut data),
                K_RESULT_OK
            );
            out.extend(out_l.iter().map(|&v| v as f64));
        }
        let r = stave_measure::zipper::zipper_report(
            &out,
            &stave_measure::zipper::ZipperConfig {
                carrier_bin: M,
                sample_rate: 48_000.0,
                guard_hz: 200.0,
                low_cut_hz: 0.0,
            },
        );
        assert!(
            r.spray_db < -60.0,
            "ABI automation ramp spray {:.1} dBc (limit −60); worst {:.1} dB @ {:.0} Hz",
            r.spray_db,
            r.worst.1,
            r.worst.0
        );
        inst.release();
    }
}

/// Framework bypass is click-free. Toggling ramps the
/// output between wet and latency-matched dry over the 10 ms
/// equal-power crossfade — never a step.
#[test]
fn bypass_crossfade_is_click_free_and_null_when_engaged() {
    unsafe {
        let inst = create_instance();
        inst.setup(); // 48 k → fade = 480 samples
        let ctl = inst.ctl_vtbl();
        // wet = 0.5·x, dry = x (latency 0): the two rails differ by 2×.
        assert_eq!(
            (ctl.set_param_normalized)(inst.controller, ID_GAIN, 0.5),
            K_RESULT_OK
        );

        let run_block = |inst: &HostInstance, out: &mut Vec<f32>| {
            let mut in_l = vec![1.0f32; 64];
            let mut in_r = vec![1.0f32; 64];
            let mut out_l = vec![0.0f32; 64];
            let mut out_r = vec![0.0f32; 64];
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            inst.process_block(&mut ins, &mut outs, 64);
            out.extend_from_slice(&out_l);
        };

        let mut out = Vec::new();
        for _ in 0..4 {
            run_block(&inst, &mut out); // active: steady 0.5
        }
        assert_eq!(
            (ctl.set_param_normalized)(inst.controller, ID_POWER, 1.0),
            K_RESULT_OK
        );
        for _ in 0..12 {
            run_block(&inst, &mut out); // fade out + bypassed
        }
        assert_eq!(
            (ctl.set_param_normalized)(inst.controller, ID_POWER, 0.0),
            K_RESULT_OK
        );
        for _ in 0..12 {
            run_block(&inst, &mut out); // fade in + active
        }

        // Steady states are exact.
        assert_eq!(out[255], 0.5, "active = wet");
        assert_eq!(out[4 * 64 + 700], 1.0, "bypassed = dry (post-fade)");
        assert_eq!(*out.last().unwrap(), 0.5, "back to wet");
        // Click limit: the largest sample step must be crossfade-sized,
        // not toggle-sized. Equal-power over 480 samples on a 0.5 gap:
        // max slope ≈ 0.5·(π/2)/480 ≈ 1.6e-3. Limit at 4e-3 (margin 2.4×).
        let max_step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_step < 4e-3,
            "click: max step {max_step} (a hard toggle would be 0.5)"
        );
        inst.release();
    }
}

/// Host transport reaches the Block with per-field
/// validity honored.
#[test]
fn transport_reaches_the_plugin() {
    unsafe {
        let inst = create_instance();
        inst.setup();
        let mut pc: ProcessContext = std::mem::zeroed();
        pc.state = PC_PLAYING | PC_TEMPO_VALID; // music-time NOT valid
        pc.sample_rate = 48_000.0;
        pc.project_time_samples = 96_000; // 2.0 s
        pc.tempo = 128.5;
        pc.project_time_music = 999.0; // must be ignored (flag unset)

        let mut in_l = vec![0.0f32; 64];
        let mut in_r = vec![0.0f32; 64];
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let mut in_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: ins.as_mut_ptr(),
        };
        let mut out_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: outs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: 64,
            num_inputs: 1,
            num_outputs: 1,
            inputs: &mut in_bus,
            outputs: &mut out_bus,
            input_parameter_changes: ptr::null_mut(),
            output_parameter_changes: ptr::null_mut(),
            input_events: ptr::null_mut(),
            output_events: ptr::null_mut(),
            process_context: &mut pc,
        };
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_RESULT_OK
        );
        assert!(SEEN_PLAYING.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(
            f64::from_bits(SEEN_TEMPO.load(std::sync::atomic::Ordering::Relaxed)),
            128.5
        );
        assert_eq!(
            f64::from_bits(SEEN_SECONDS.load(std::sync::atomic::Ordering::Relaxed)),
            2.0
        );
        inst.release();
    }
}

/// Hostile block sizes are refused or no-oped, never UB,
/// and normal processing continues afterward.
#[test]
fn block_size_guards_hold() {
    unsafe {
        let inst = create_instance();
        inst.setup(); // max_frames 64
        let mut in_l = vec![1.0f32; 128];
        let mut in_r = vec![1.0f32; 128];
        let mut out_l = vec![0.0f32; 128];
        let mut out_r = vec![0.0f32; 128];
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let build = |ins: &mut [*mut f32; 2], outs: &mut [*mut f32; 2], n: i32| {
            let in_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers_32: ins.as_mut_ptr(),
            };
            let out_bus = AudioBusBuffers {
                num_channels: 2,
                silence_flags: 0,
                channel_buffers_32: outs.as_mut_ptr(),
            };
            (in_bus, out_bus, n)
        };
        // Zero-length: silent success.
        let (mut ib, mut ob, n) = build(&mut ins, &mut outs, 0);
        let mut data = ProcessData {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: n,
            num_inputs: 1,
            num_outputs: 1,
            inputs: &mut ib,
            outputs: &mut ob,
            input_parameter_changes: ptr::null_mut(),
            output_parameter_changes: ptr::null_mut(),
            input_events: ptr::null_mut(),
            output_events: ptr::null_mut(),
            process_context: ptr::null_mut(),
        };
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_RESULT_OK
        );
        // Oversized (128 > declared 64): refused cleanly.
        data.num_samples = 128;
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_INVALID_ARGUMENT
        );
        // And a normal block still works after the refusal.
        data.num_samples = 64;
        assert_eq!(
            (inst.proc_vtbl().process)(inst.processor, &mut data),
            K_RESULT_OK
        );
        assert_eq!(out_l[63], 1.0);
        inst.release();
    }
}

#[cfg(target_os = "macos")]
/// Headless (a `FaceView` touches no Cocoa until attached): a face edit
/// goes through the view's controller, reaches the host as
/// beginEdit → performEdit(normalized) → endEdit through the REAL
/// IComponentHandler, and lands in the table — the DAW can record AND
/// undo it.
#[test]
fn editor_writes_reach_the_host_and_the_table() {
    unsafe {
        let inst = create_instance();

        // Attach a recording handler, then create the editor.
        let mut handler = MockHandler {
            vtbl: &MH_VTBL,
            log: Vec::new(),
        };
        assert_eq!(
            (inst.ctl_vtbl().set_component_handler)(
                inst.controller,
                ptr::addr_of_mut!(handler).cast()
            ),
            K_RESULT_OK
        );
        let name: Vec<u8> = b"editor\0".to_vec();
        let view_ptr = ((inst.ctl_vtbl()).create_view)(inst.controller, name.as_ptr() as FidString);
        assert!(!view_ptr.is_null(), "createView");
        let editor_box = view_ptr as *mut stave_vst3::wrapper::EditorBox<TableGain>;
        let controller = (*editor_box).view.controller();

        // A knob drag on "drive": the gesture conversation and the table.
        let drive = stave_core::ParamId(ID_DRIVE);
        controller.begin(drive);
        controller.set(drive, stave_core::Normalized::new(0.5)); // 12 dB on −12..36
        controller.set(drive, stave_core::Normalized::new(0.125)); // −6 dB
        controller.end(drive);
        assert_eq!(
            handler.log,
            vec![
                "begin 7".to_string(),
                "perform 7 0.5".to_string(),
                "perform 7 0.125".to_string(),
                "end 7".to_string(),
            ],
            "editor writes reach the DAW via the handler"
        );
        assert_eq!(
            (inst.ctl_vtbl().get_param_normalized)(inst.controller, ID_DRIVE),
            0.125,
            "table is the truth after the gesture"
        );

        // Detach the stack handler before release.
        assert_eq!(
            (inst.ctl_vtbl().set_component_handler)(inst.controller, ptr::null_mut()),
            K_RESULT_OK
        );
        let view_vtbl = &*(*(view_ptr as *mut RawObj<IPlugViewVtbl>)).vtbl;
        assert_eq!((view_vtbl.release)(view_ptr), 0, "createView returned +1");
        inst.release();
    }
}

#[cfg(target_os = "macos")]
/// S7: the view holds a reference on its instance (SDK `CPluginView`
/// pattern). A host that releases every controller/component reference
/// first can still call the view, and the view's last release frees the
/// instance. Under Miri a use-after-free here is a hard error.
#[test]
fn a_view_outliving_the_controller_keeps_the_instance_alive() {
    unsafe {
        let inst = create_instance();
        let name = c"editor";
        let view = (inst.ctl_vtbl().create_view)(inst.controller, name.as_ptr());
        assert!(!view.is_null(), "createView");
        let other = (inst.ctl_vtbl().create_view)(inst.controller, name.as_ptr());
        assert!(!other.is_null() && other != view, "a fresh view per call");
        let vt = &*(*(view as *mut RawObj<IPlugViewVtbl>)).vtbl;
        assert_eq!((vt.release)(other), 0);

        // The host drops the controller (and everything else) first.
        inst.release();

        // Every view method still reaches a live instance.
        let mut rect = ViewRect {
            left: -1,
            top: -1,
            right: -1,
            bottom: -1,
        };
        assert_eq!((vt.get_size)(view, &mut rect), K_RESULT_OK);
        assert!(rect.right > 0 && rect.bottom > 0, "{rect:?}");
        assert_eq!((vt.get_size)(view, ptr::null_mut()), K_INVALID_ARGUMENT);
        assert_eq!(
            (vt.is_platform_type_supported)(view, c"NSView".as_ptr()),
            K_RESULT_OK
        );
        // Null parent: attach declines before touching Cocoa.
        assert_eq!(
            (vt.attached)(view, ptr::null_mut(), c"NSView".as_ptr()),
            K_RESULT_OK
        );
        assert_eq!((vt.removed)(view), K_RESULT_OK);

        // A real refcount: queryInterface and addRef take references.
        let mut same: *mut c_void = ptr::null_mut();
        assert_eq!(
            (vt.query_interface)(view, &IID_IPLUG_VIEW, &mut same),
            K_RESULT_OK
        );
        assert_eq!(same, view.cast());
        assert_eq!((vt.add_ref)(view), 3);
        assert_eq!((vt.release)(view), 2);
        assert_eq!((vt.release)(view), 1);
        // The last reference frees the view, then the instance.
        assert_eq!((vt.release)(view), 0);
    }
}

#[test]
fn two_table_instances_share_nothing() {
    unsafe {
        let a = create_instance();
        let b = create_instance();
        let (ca, cb) = (a.ctl_vtbl(), b.ctl_vtbl());
        assert_eq!(
            (ca.set_param_normalized)(a.controller, ID_GAIN, 0.9),
            K_RESULT_OK
        );
        assert_eq!(
            (cb.set_param_normalized)(b.controller, ID_GAIN, 0.1),
            K_RESULT_OK
        );
        assert_eq!((ca.get_param_normalized)(a.controller, ID_GAIN), 0.9);
        assert_eq!((cb.get_param_normalized)(b.controller, ID_GAIN), 0.1);
        a.release();
        b.release();
    }
}

/// Parameter groups reach the host as VST3 units: the root plus one per
/// group, each parameter filed under its group's unit; no presets means
/// no program list.
#[test]
fn groups_are_units_and_params_sit_in_them() {
    unsafe {
        let inst = create_instance();
        let qi = (*(*(inst.controller as *mut RawObj<IEditControllerVtbl>)).vtbl).query_interface;
        let mut obj: *mut c_void = ptr::null_mut();
        assert_eq!(qi(inst.controller, &IID_IUNIT_INFO, &mut obj), K_RESULT_OK);
        let units = obj as *mut FUnknown;
        let ui = &*(*(units as *mut RawObj<IUnitInfoVtbl>)).vtbl;

        assert_eq!((ui.get_unit_count)(units), 2);
        let mut info: UnitInfo = std::mem::zeroed();
        assert_eq!((ui.get_unit_info)(units, 0, &mut info), K_RESULT_OK);
        assert_eq!(
            (info.id, info.parent_unit_id),
            (ROOT_UNIT_ID, NO_PARENT_UNIT_ID)
        );
        assert_eq!(info.program_list_id, NO_PROGRAM_LIST_ID);
        assert_eq!((ui.get_unit_info)(units, 1, &mut info), K_RESULT_OK);
        assert_eq!((info.id, info.parent_unit_id), (1, ROOT_UNIT_ID));
        assert_eq!(utf16_to_string(&info.name), "Main");
        assert_eq!((ui.get_unit_info)(units, 2, &mut info), K_INVALID_ARGUMENT);
        assert_eq!((ui.get_program_list_count)(units), 0);

        let ctl = inst.ctl_vtbl();
        for i in 0..(ctl.get_parameter_count)(inst.controller) {
            let mut p: ParameterInfo = std::mem::zeroed();
            assert_eq!(
                (ctl.get_parameter_info)(inst.controller, i, &mut p),
                K_RESULT_OK
            );
            let expected = if p.id == ID_DRIVE { 1 } else { ROOT_UNIT_ID };
            assert_eq!(p.unit_id, expected, "param {}", p.id);
        }

        assert_eq!((ui.select_unit)(units, 1), K_RESULT_OK);
        assert_eq!((ui.get_selected_unit)(units), 1);
        assert_eq!((ui.select_unit)(units, 7), K_INVALID_ARGUMENT);
        assert_eq!((ui.get_selected_unit)(units), 1);

        (ui.release)(units);
        inst.release();
    }
}
