//! Host input the wrapper must refuse, driven through the real COM vtables:
//! a negative or oversized `num_samples`, a process call before
//! `setupProcessing`, a negative `maxSamplesPerBlock`, and a state stream
//! that reports reading more bytes than it was asked for. Each is a refusal
//! (or silence), never an out-of-bounds write, an abort or a latched
//! instance.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;

use stave_core::runtime::Erased;
use stave_core::{Block, Info, Telemetry};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

/// Writes 0.5 to every output sample, so a block that ran is visible.
struct Half;

impl Erased for Half {
    const INFO: Info = Info {
        id: "com.stave.test.ingress",
        name: "Ingress",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "host-ingress refusal fixture",
        features: &[],
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self
    }

    fn process(&mut self, block: &mut Block<'_>) {
        for c in 0..block.output().channel_count() {
            block.output().channel_mut(c).fill(0.5);
        }
    }
}

impl stave_vst3::wrapper::Vst3Export for Half {}

stave_vst3::export_vst3!(Half);

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

struct Host {
    component: *mut FUnknown,
    processor: *mut FUnknown,
}

impl Host {
    unsafe fn new() -> Self {
        unsafe {
            let factory = GetPluginFactory() as *mut FactoryObj;
            let cid = <Half as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
            let mut obj: *mut c_void = ptr::null_mut();
            assert_eq!(
                ((*(*factory).vtbl).create_instance)(
                    factory.cast(),
                    cid.as_ptr().cast(),
                    IID_ICOMPONENT.as_ptr().cast(),
                    &mut obj,
                ),
                K_RESULT_OK
            );
            let component = obj.cast::<FUnknown>();
            let comp = &*(*component.cast::<RawObj<IComponentVtbl>>()).vtbl;
            let mut proc_obj: *mut c_void = ptr::null_mut();
            assert_eq!(
                (comp.query_interface)(component, &IID_IAUDIO_PROCESSOR, &mut proc_obj),
                K_RESULT_OK
            );
            Self {
                component,
                processor: proc_obj.cast(),
            }
        }
    }

    unsafe fn comp(&self) -> &'static IComponentVtbl {
        unsafe { &*(*self.component.cast::<RawObj<IComponentVtbl>>()).vtbl }
    }

    unsafe fn ap(&self) -> &'static IAudioProcessorVtbl {
        unsafe { &*(*self.processor.cast::<RawObj<IAudioProcessorVtbl>>()).vtbl }
    }

    unsafe fn setup_processing(&self, max_frames: i32) -> Tresult {
        let mut setup = ProcessSetup {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: max_frames,
            sample_rate: 48_000.0,
        };
        unsafe { (self.ap().setup_processing)(self.processor, &mut setup) }
    }

    unsafe fn activate(&self, max_frames: i32) {
        unsafe {
            assert_eq!(self.setup_processing(max_frames), K_RESULT_OK);
            assert_eq!((self.comp().set_active)(self.component, 1), K_RESULT_OK);
        }
    }

    /// One stereo block over 64-frame buffers filled with `fill`, the
    /// host claiming `num_samples`; the result and the output buffers.
    unsafe fn process(&self, num_samples: i32, fill: f32) -> (Tresult, [[f32; 64]; 2]) {
        let mut input = [[0.25f32; 64]; 2];
        let mut output = [[fill; 64]; 2];
        let mut in_ptrs = [input[0].as_mut_ptr(), input[1].as_mut_ptr()];
        let mut out_ptrs = [output[0].as_mut_ptr(), output[1].as_mut_ptr()];
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
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples,
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
        let result = unsafe { (self.ap().process)(self.processor, &mut data) };
        (result, output)
    }

    unsafe fn release(self) {
        unsafe {
            let release = self.comp().release;
            release(self.processor);
            assert_eq!(release(self.component), 0);
        }
    }
}

/// U2: before `setupProcessing` there was no maximum to check against, so
/// a negative `num_samples`, cast to ~`usize::MAX`, reached the silencing
/// path as a frame count and wrote far past the host's buffers.
#[test]
fn a_negative_block_length_is_refused_before_setup() {
    unsafe {
        let host = Host::new();
        for num_samples in [-1, -64, i32::MIN] {
            let (result, output) = host.process(num_samples, 1.0);
            assert_eq!(result, K_INVALID_ARGUMENT, "num_samples {num_samples}");
            assert_eq!(output, [[1.0; 64]; 2], "the host's buffers are untouched");
        }
        host.release();
    }
}

#[test]
fn a_negative_block_length_is_refused_after_setup() {
    unsafe {
        let host = Host::new();
        host.activate(64);
        let (result, output) = host.process(-64, 1.0);
        assert_eq!(result, K_INVALID_ARGUMENT);
        assert_eq!(output, [[1.0; 64]; 2]);
        // The instance still runs.
        assert_eq!(host.process(64, 1.0), (K_RESULT_OK, [[0.5; 64]; 2]));
        host.release();
    }
}

#[test]
fn a_block_beyond_the_declared_maximum_is_refused() {
    unsafe {
        let host = Host::new();
        host.activate(32);
        let (result, output) = host.process(64, 1.0);
        assert_eq!(result, K_INVALID_ARGUMENT);
        assert_eq!(output, [[1.0; 64]; 2]);
        host.release();
    }
}

/// Process before `setupProcessing`: nothing is prepared, so the block is
/// silence over the host's own length and the plugin never runs.
#[test]
fn a_block_before_setup_is_silence() {
    unsafe {
        let host = Host::new();
        assert_eq!(host.process(64, 1.0), (K_RESULT_OK, [[0.0; 64]; 2]));
        host.release();
    }
}

/// A negative `maxSamplesPerBlock` wrapped to ~4 billion frames: the
/// scratch sized for it aborted the process, and every later block passed
/// the length check.
#[test]
fn a_negative_maximum_block_is_refused() {
    unsafe {
        let host = Host::new();
        assert_eq!(host.setup_processing(-1), K_INVALID_ARGUMENT);
        // A valid setup still works afterwards.
        host.activate(64);
        assert_eq!(host.process(64, 1.0), (K_RESULT_OK, [[0.5; 64]; 2]));
        host.release();
    }
}

// ===========================================================================
// A state stream that claims to have read one byte more than it was asked
// ===========================================================================

#[repr(C)]
struct LyingStream {
    vtbl: *const IBStreamVtbl,
}

unsafe extern "C" fn ls_qi(_t: *mut IBStream, _i: *const Tuid, _o: *mut *mut c_void) -> Tresult {
    K_NO_INTERFACE
}
unsafe extern "C" fn ls_ref(_t: *mut IBStream) -> u32 {
    1
}
unsafe extern "C" fn ls_read(_t: *mut IBStream, _b: *mut c_void, n: i32, read: *mut i32) -> Tresult {
    // Writes nothing (the buffer is only `n` long) but reports `n + 1`.
    if !read.is_null() {
        unsafe { *read = n + 1 };
    }
    K_RESULT_OK
}
unsafe extern "C" fn ls_write(_t: *mut IBStream, _b: *mut c_void, _n: i32, written: *mut i32) -> Tresult {
    if !written.is_null() {
        unsafe { *written = 0 };
    }
    K_RESULT_FALSE
}
unsafe extern "C" fn ls_seek(_t: *mut IBStream, _pos: i64, _mode: i32, result: *mut i64) -> Tresult {
    if !result.is_null() {
        unsafe { *result = 0 };
    }
    K_RESULT_OK
}
unsafe extern "C" fn ls_tell(_t: *mut IBStream, pos: *mut i64) -> Tresult {
    if !pos.is_null() {
        unsafe { *pos = 0 };
    }
    K_RESULT_OK
}

static LS_VTBL: IBStreamVtbl = IBStreamVtbl {
    query_interface: ls_qi,
    add_ref: ls_ref,
    release: ls_ref,
    read: ls_read,
    write: ls_write,
    seek: ls_seek,
    tell: ls_tell,
};

/// A4: the read count was used to slice the wrapper's chunk unchecked, so
/// the panic latched the instance (every later call failed). Now the load
/// is refused and the instance keeps running.
#[test]
fn a_stream_overreporting_its_read_is_refused_and_the_instance_lives() {
    unsafe {
        let host = Host::new();
        let mut stream = LyingStream { vtbl: &LS_VTBL };
        let result = (host.comp().set_state)(host.component, ptr::addr_of_mut!(stream).cast());
        assert_eq!(result, K_RESULT_FALSE, "a refusal, not a contained panic");
        host.activate(64);
        assert_eq!(host.process(64, 1.0), (K_RESULT_OK, [[0.5; 64]; 2]));
        host.release();
    }
}
