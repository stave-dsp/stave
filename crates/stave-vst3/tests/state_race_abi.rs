//! A state save on the main thread racing a block on the audio thread
//! (a host saving a project during playback — the VST3 threading model
//! allows it): the save holds the plugin while it serializes, and a block
//! that arrives meanwhile finds it held. That block is dropped, never
//! refused: `kResultOk`, silence over whatever the host's buffers held,
//! and a fault in the trace (`ConcurrentProcess` while active,
//! `Unprepared` while not). The interleaving is forced, not raced: the
//! plugin's `save` blocks until the block has run. The simulated host's
//! fuzz finding (`SaveDuringProcess` answered `kInternalError`) is this
//! race, hit by chance.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract. Miri
// runs it (and checks the two threads for data races).
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use stave_core::params::{FloatParam, Params, Range};
use stave_core::runtime::Erased;
use stave_core::telemetry::{Event, Fault};
use stave_core::{Block, Info, Telemetry};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

const FRAMES: usize = if cfg!(miri) { 8 } else { 64 };

/// Where the plugin's `save` is: 0 idle, 1 inside (holding the plugin),
/// 2 released by the test.
static SAVE: (Mutex<u8>, Condvar) = (Mutex::new(0), Condvar::new());
/// The tests share `SAVE` and run on parallel threads.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn save_phase_is(phase: u8) {
    let (lock, cv) = &SAVE;
    let mut at = lock.lock().unwrap();
    while *at != phase {
        at = cv.wait(at).unwrap();
    }
}

fn set_save_phase(phase: u8) {
    let (lock, cv) = &SAVE;
    *lock.lock().unwrap() = phase;
    cv.notify_all();
}

#[derive(stave_core::Params)]
struct OneParam {
    level: FloatParam,
}

/// Writes a constant, so a block that ran is unmistakable; its save
/// blocks until the test releases it.
struct SlowSave {
    params: Arc<OneParam>,
}

impl Erased for SlowSave {
    const INFO: Info = Info {
        id: "com.stave.test.slow-save",
        name: "Slow Save",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "state-save race fixture",
        features: &[],
        au: None,
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self {
            params: Arc::new(OneParam {
                level: FloatParam::new(stave_core::ParamId(0), "level", Range::linear(0.0, 1.0)).default(0.5),
            }),
        }
    }

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(&mut self, block: &mut Block<'_>) {
        for c in 0..block.output().channel_count() {
            block.output().channel_mut(c).fill(0.75);
        }
    }

    fn save(&self) -> Vec<u8> {
        set_save_phase(1);
        save_phase_is(2);
        vec![1, 2, 3]
    }
}

impl stave_vst3::wrapper::Vst3Export for SlowSave {}

stave_vst3::export_vst3!(SlowSave);

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

struct HostInstance {
    component: *mut FUnknown,
    processor: *mut FUnknown,
}

/// A raw pointer the save thread may carry: the instance outlives it.
#[derive(Copy, Clone)]
struct Shared(*mut FUnknown);
unsafe impl Send for Shared {}

impl HostInstance {
    unsafe fn new() -> Self {
        let factory = GetPluginFactory() as *mut FactoryObj;
        let fvtbl = unsafe { &*(*factory).vtbl };
        let cid = <SlowSave as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
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
        let mut processor: *mut c_void = ptr::null_mut();
        assert_eq!(unsafe { qi(component, &IID_IAUDIO_PROCESSOR, &mut processor) }, K_RESULT_OK);
        Self {
            component,
            processor: processor.cast(),
        }
    }

    unsafe fn comp(&self) -> &'static IComponentVtbl {
        unsafe { &*(*(self.component as *mut RawObj<IComponentVtbl>)).vtbl }
    }

    unsafe fn proc(&self) -> &'static IAudioProcessorVtbl {
        unsafe { &*(*(self.processor as *mut RawObj<IAudioProcessorVtbl>)).vtbl }
    }

    unsafe fn setup(&self) {
        let mut setup = ProcessSetup {
            process_mode: PROCESS_MODE_REALTIME,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: FRAMES as i32,
            sample_rate: 48_000.0,
        };
        assert_eq!(unsafe { (self.proc().setup_processing)(self.processor, &mut setup) }, K_RESULT_OK);
    }

    unsafe fn set_active(&self, on: bool) {
        assert_eq!(unsafe { (self.comp().set_active)(self.component, u8::from(on)) }, K_RESULT_OK);
    }

    /// One stereo block over host buffers holding stale 0.25s: the result
    /// code and what the outputs hold after it.
    unsafe fn process(&self) -> (Tresult, Vec<f32>) {
        let mut ins = [[0.1f32; FRAMES]; 2];
        let mut outs = [[0.25f32; FRAMES]; 2];
        let mut in_ptrs: Vec<*mut f32> = ins.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let mut out_ptrs: Vec<*mut f32> = outs.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let mut in_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: in_ptrs.as_mut_ptr(),
        };
        let mut out_bus = AudioBusBuffers {
            num_channels: 2,
            silence_flags: 0,
            channel_buffers_32: out_ptrs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: PROCESS_MODE_REALTIME,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: FRAMES as i32,
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
        let r = unsafe { (self.proc().process)(self.processor, &mut data) };
        (r, outs.concat())
    }

    /// A block run while `getState` on another thread holds the plugin
    /// inside its `save`; the save is then let go and must succeed.
    unsafe fn process_during_save(&self) -> (Tresult, Vec<f32>) {
        set_save_phase(0);
        let component = Shared(self.component);
        let get_state = unsafe { self.comp().get_state };
        let saver = std::thread::spawn(move || {
            let component = component;
            let mut stream = MockStream::default();
            let r = unsafe { get_state(component.0, stream.as_funknown()) };
            (r, stream.data)
        });
        save_phase_is(1);
        let block = unsafe { self.process() };
        set_save_phase(2);
        let (r, bytes) = saver.join().expect("the save thread");
        assert_eq!(r, K_RESULT_OK, "getState during a block");
        assert!(!bytes.is_empty(), "the save wrote the state");
        block
    }

    unsafe fn release(self) {
        unsafe {
            let release = self.comp().release;
            release(self.processor);
            release(self.component);
        }
    }
}

fn faults(telemetry: &Telemetry, from: u64) -> Vec<Fault> {
    telemetry
        .events()
        .read_from(from)
        .filter_map(|r| match r.event {
            Event::Fault(f) => Some(f),
            _ => None,
        })
        .collect()
}

/// While active: the block is dropped (silence, `kResultOk`, a
/// `ConcurrentProcess` fault), and the next block runs as ever.
#[test]
fn a_block_during_a_save_is_dropped_not_refused() {
    let _serial = serial();
    unsafe {
        let host = HostInstance::new();
        let telemetry = &*stave_telemetry_for(host.component.cast());
        host.setup();
        host.set_active(true);
        let (r, out) = host.process();
        assert_eq!(r, K_RESULT_OK);
        assert!(out.iter().all(|&s| s == 0.75), "an ordinary block runs");

        let from = telemetry.events().written();
        let (r, out) = host.process_during_save();
        assert_eq!(r, K_RESULT_OK, "a block during a save answers kResultOk, got {r}");
        assert!(out.iter().all(|&s| s == 0.0), "a dropped block is silence: {out:?}");
        assert_eq!(faults(telemetry, from), [Fault::ConcurrentProcess]);

        let (r, out) = host.process();
        assert_eq!(r, K_RESULT_OK);
        assert!(out.iter().all(|&s| s == 0.75), "the block after the save runs");
        host.set_active(false);
        host.release();
    }
}

/// While inactive (the fuzz finding's order: a deactivated plugin, a
/// host processing anyway, a save racing it): the block was to be silent
/// and unprepared regardless — the save changes nothing about the answer.
#[test]
fn an_unprepared_block_during_a_save_is_silent_and_unprepared() {
    let _serial = serial();
    unsafe {
        let host = HostInstance::new();
        let telemetry = &*stave_telemetry_for(host.component.cast());
        host.setup();
        host.set_active(true);
        host.set_active(false);

        let from = telemetry.events().written();
        let (r, out) = host.process_during_save();
        assert_eq!(r, K_RESULT_OK, "an unprepared block during a save answers kResultOk, got {r}");
        assert!(out.iter().all(|&s| s == 0.0), "an unprepared block is silence: {out:?}");
        assert_eq!(faults(telemetry, from), [Fault::Unprepared]);

        let (r, out) = host.process();
        assert_eq!(r, K_RESULT_OK);
        assert!(out.iter().all(|&s| s == 0.0));
        host.release();
    }
}

// ===========================================================================
// A Vec-backed host stream (write only: getState)
// ===========================================================================

#[repr(C)]
struct MockStream {
    vtbl: *const IBStreamVtbl,
    data: Vec<u8>,
}

impl Default for MockStream {
    fn default() -> Self {
        Self {
            vtbl: &MS_VTBL,
            data: Vec::new(),
        }
    }
}

impl MockStream {
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
unsafe extern "C" fn ms_read(_t: *mut IBStream, _b: *mut c_void, _n: i32, read: *mut i32) -> Tresult {
    if !read.is_null() {
        unsafe { *read = 0 };
    }
    K_RESULT_FALSE
}
unsafe extern "C" fn ms_write(t: *mut IBStream, buffer: *mut c_void, n: i32, written: *mut i32) -> Tresult {
    let s = unsafe { &mut *(t as *mut MockStream) };
    s.data
        .extend_from_slice(unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), n as usize) });
    if !written.is_null() {
        unsafe { *written = n };
    }
    K_RESULT_OK
}
unsafe extern "C" fn ms_seek(t: *mut IBStream, _pos: i64, _mode: i32, result: *mut i64) -> Tresult {
    let s = unsafe { &*(t as *const MockStream) };
    if !result.is_null() {
        unsafe { *result = s.data.len() as i64 };
    }
    K_RESULT_OK
}
unsafe extern "C" fn ms_tell(t: *mut IBStream, pos: *mut i64) -> Tresult {
    let s = unsafe { &*(t as *const MockStream) };
    unsafe { *pos = s.data.len() as i64 };
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
