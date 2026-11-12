//! ABI-level soundness tests: drive the wrapper through its real
//! `extern "C"` vtable path, exactly as a host would — factory →
//! createInstance → queryInterface → setupProcessing → setActive →
//! process. No harness shortcuts.
//!
//! Covers two acceptance checks:
//! - In-place processing: a host passing `in == out` pointers runs
//!   bit-identically to the split-buffer run, for a plugin that is both
//!   linear (gain) and stateful (16-tap delay).
//! - Two-instance isolation: simultaneously-live instances keep
//!   independent parameter values and independent DSP state.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract. Miri
// runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use stave_core::params::{FloatParam, Params, Range, Smooth};
use stave_core::runtime::Erased;
use stave_core::{Block, Info, Telemetry};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

// ===========================================================================
// Test plugin: y[n] = gain·x[n] + 0.25·x[n−16]. Linear AND stateful, so the
// in-place comparison catches both overwritten-input bugs and state skew.
// ===========================================================================

#[derive(stave_core::Params)]
struct EchoParams {
    gain: FloatParam,
    mix: FloatParam,
}

fn unit(id: u32, name: &'static str, default: f64) -> FloatParam {
    FloatParam::new(stave_core::ParamId(id), name, Range::linear(0.0, 1.0)).default(default)
        .group("Main")
        .smooth(Smooth::Off)
}

const TAPS: usize = 16;

struct TestEcho {
    params: Arc<EchoParams>,
    dly: [[f32; TAPS]; 2],
    pos: usize,
}

impl Erased for TestEcho {
    const INFO: Info = Info {
        id: "com.stave.test.echo",
        name: "Test Echo",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "ABI soundness fixture",
        features: &[],
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self {
            params: Arc::new(EchoParams {
                gain: unit(0, "gain", 0.5),
                mix: unit(1, "mix", 1.0),
            }),
            dly: [[0.0; TAPS]; 2],
            pos: 0,
        }
    }

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let gain = self.params.gain.value();
        let mix = self.params.mix.value();
        let chans = block.output().channel_count()
            .min(2)
            .min(block.input().channel_count());
        for f in 0..block.frames() {
            for c in 0..chans {
                let x = block.input().channel(c)[f];
                let d = self.dly[c][self.pos];
                self.dly[c][self.pos] = x;
                block.output().channel_mut(c)[f] = mix * (gain * x + 0.25 * d);
            }
            self.pos = (self.pos + 1) % TAPS;
        }
    }
}

impl stave_vst3::wrapper::Vst3Export for TestEcho {}

stave_vst3::export_vst3!(TestEcho);

// ===========================================================================
// Minimal host plumbing
// ===========================================================================

/// Every interface pointer starts with its vtable pointer.
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
    let cid = <TestEcho as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
    let mut obj: *mut c_void = ptr::null_mut();
    let res = unsafe {
        (fvtbl.create_instance)(
            factory as *mut FUnknown,
            cid.as_ptr() as FidString,
            IID_ICOMPONENT.as_ptr() as FidString,
            &mut obj,
        )
    };
    assert_eq!(res, K_RESULT_OK, "createInstance failed");
    assert!(!obj.is_null());
    let component = obj as *mut FUnknown;

    let qi = unsafe { (*(component as *mut RawObj<IComponentVtbl>)).vtbl };
    let qi = unsafe { (*qi).query_interface };
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

    unsafe fn setup(&self, sample_rate: f64, max_frames: i32) {
        let mut setup = ProcessSetup {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: max_frames,
            sample_rate,
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

    unsafe fn process(&self, in_ptrs: &mut [*mut f32], out_ptrs: &mut [*mut f32], frames: usize) {
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
        // The component holds ref 1 from create; the two queryInterface
        // calls added one each.
        unsafe {
            let release = self.comp_vtbl().release;
            release(self.processor);
            release(self.controller);
            release(self.component);
        }
    }
}

/// Deterministic full-scale-ish test signal.
fn test_signal(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2654435761).max(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

// ===========================================================================
// Checks
// ===========================================================================

// Under Miri every frame is interpreted: three short blocks still
// cross every block boundary and every unsafe path.
const FRAMES: usize = if cfg!(miri) { 16 } else { 64 };
const BLOCKS: usize = if cfg!(miri) { 3 } else { 8 };

#[test]
fn in_place_processing_matches_split_buffers() {
    let signal_l = test_signal(FRAMES * BLOCKS, 1);
    let signal_r = test_signal(FRAMES * BLOCKS, 2);

    // Reference: split input/output buffers.
    let reference = unsafe {
        let inst = create_instance();
        inst.setup(48_000.0, FRAMES as i32);
        let mut out_all = vec![vec![0.0f32; FRAMES * BLOCKS]; 2];
        let mut in_l = vec![0.0f32; FRAMES];
        let mut in_r = vec![0.0f32; FRAMES];
        let mut out_l = vec![0.0f32; FRAMES];
        let mut out_r = vec![0.0f32; FRAMES];
        for b in 0..BLOCKS {
            let base = b * FRAMES;
            in_l.copy_from_slice(&signal_l[base..base + FRAMES]);
            in_r.copy_from_slice(&signal_r[base..base + FRAMES]);
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            inst.process(&mut ins, &mut outs, FRAMES);
            out_all[0][base..base + FRAMES].copy_from_slice(&out_l);
            out_all[1][base..base + FRAMES].copy_from_slice(&out_r);
        }
        inst.release();
        out_all
    };

    // In-place: input and output are the SAME buffers, as Live does.
    let in_place = unsafe {
        let inst = create_instance();
        inst.setup(48_000.0, FRAMES as i32);
        let mut out_all = vec![vec![0.0f32; FRAMES * BLOCKS]; 2];
        let mut buf_l = vec![0.0f32; FRAMES];
        let mut buf_r = vec![0.0f32; FRAMES];
        for b in 0..BLOCKS {
            let base = b * FRAMES;
            buf_l.copy_from_slice(&signal_l[base..base + FRAMES]);
            buf_r.copy_from_slice(&signal_r[base..base + FRAMES]);
            let mut ins = [buf_l.as_mut_ptr(), buf_r.as_mut_ptr()];
            let mut outs = [buf_l.as_mut_ptr(), buf_r.as_mut_ptr()];
            inst.process(&mut ins, &mut outs, FRAMES);
            out_all[0][base..base + FRAMES].copy_from_slice(&buf_l);
            out_all[1][base..base + FRAMES].copy_from_slice(&buf_r);
        }
        inst.release();
        out_all
    };

    for c in 0..2 {
        for (i, (a, b)) in reference[c].iter().zip(&in_place[c]).enumerate() {
            assert!(
                a.to_bits() == b.to_bits(),
                "channel {c} sample {i}: split={a} in-place={b} (bit mismatch)"
            );
        }
    }
}

/// Block-size invariance at the ABI level. The same
/// signal chopped into hostile block sizes (1, 16, 333, …) runs
/// bit-identically to the contiguous reference — no state skew, no
/// boundary artifacts, for a STATEFUL plugin.
#[test]
fn hostile_block_sizes_render_bit_identically() {
    // Under Miri every frame is interpreted: 40 frames still run every
    // size (one-frame blocks, 16, 333 cut to fit, the mixed cycle)
    // through the same unsafe paths; the full length runs natively.
    const TOTAL: usize = if cfg!(miri) { 40 } else { 4096 };
    const MAX: i32 = 512;
    let sig_l = test_signal(TOTAL, 11);
    let sig_r = test_signal(TOTAL, 12);

    let run = |sizes: &[usize]| -> Vec<f32> {
        unsafe {
            let inst = create_instance();
            let mut setup = ProcessSetup {
                process_mode: 0,
                symbolic_sample_size: SAMPLE_SIZE_32,
                max_samples_per_block: MAX,
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
            let mut out = Vec::with_capacity(TOTAL);
            let mut pos = 0usize;
            let mut si = 0usize;
            while pos < TOTAL {
                let n = sizes[si % sizes.len()].min(TOTAL - pos).max(1);
                si += 1;
                let mut in_l = sig_l[pos..pos + n].to_vec();
                let mut in_r = sig_r[pos..pos + n].to_vec();
                let mut out_l = vec![0.0f32; n];
                let mut out_r = vec![0.0f32; n];
                let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
                let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
                inst.process(&mut ins, &mut outs, n);
                out.extend_from_slice(&out_l);
                pos += n;
            }
            inst.release();
            out
        }
    };

    let reference = run(&[512]);
    for sizes in [&[1usize][..], &[16], &[333], &[1, 16, 64, 333, 7, 128]] {
        let split = run(sizes);
        for (i, (a, b)) in reference.iter().zip(&split).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sizes {sizes:?}: sample {i} diverges from contiguous reference"
            );
        }
    }
}

#[test]
fn two_instances_are_isolated() {
    unsafe {
        let a = create_instance();
        let b = create_instance();
        a.setup(48_000.0, FRAMES as i32);
        b.setup(48_000.0, FRAMES as i32);

        // Param id 0 is "gain".
        let set_a = a.ctl_vtbl().set_param_normalized;
        let set_b = b.ctl_vtbl().set_param_normalized;
        assert_eq!(set_a(a.controller, 0, 1.0), K_RESULT_OK);
        assert_eq!(set_b(b.controller, 0, 0.0), K_RESULT_OK);

        let get_a = a.ctl_vtbl().get_param_normalized;
        let get_b = b.ctl_vtbl().get_param_normalized;
        assert_eq!(get_a(a.controller, 0), 1.0, "instance A keeps its own gain");
        assert_eq!(get_b(b.controller, 0), 0.0, "instance B keeps its own gain");

        // DSP isolation: same input, different params, different output;
        // and the delayed tap proves state isn't shared either.
        let sig = test_signal(FRAMES, 7);
        let run = |inst: &HostInstance| -> Vec<f32> {
            let mut in_l = sig.clone();
            let mut in_r = sig.clone();
            let mut out_l = vec![0.0f32; FRAMES];
            let mut out_r = vec![0.0f32; FRAMES];
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            inst.process(&mut ins, &mut outs, FRAMES);
            out_l
        };
        let out_a = run(&a);
        let out_b = run(&b);

        // gain=1: y[0] = x[0] (delay line starts empty).
        assert_eq!(out_a[0].to_bits(), sig[0].to_bits());
        // gain=0: y[0] = 0, and y[16] = 0.25·x[0] — state is B's own.
        assert_eq!(out_b[0], 0.0);
        assert_eq!(out_b[TAPS].to_bits(), (0.25 * sig[0]).to_bits());

        a.release();
        b.release();
    }
}

/// VST3 has no host log: refusals reach the plugin's log (file and
/// telemetry), and an audio-thread fault is reported by the next lifecycle
/// call. The wrapper's taps are registered (at the floor) before audio.
#[test]
fn refusals_and_process_faults_are_logged() {
    unsafe {
        let host = create_instance();
        let telemetry = &*stave_telemetry_for(host.component.cast());
        assert_eq!(telemetry.value(stave_core::runtime::WrapperTaps::IO_IN), Some(-120.0));
        assert_eq!(telemetry.value(stave_core::runtime::WrapperTaps::CPU), None);
        assert_eq!(
            (host.proc_vtbl().process)(host.processor, ptr::null_mut()),
            K_INVALID_ARGUMENT
        );
        assert_eq!(
            (host.ctl_vtbl().set_param_normalized)(host.controller, 999, 0.5),
            K_INVALID_ARGUMENT
        );
        host.setup(48_000.0, 64);
        let messages: Vec<String> = telemetry.messages().read_from(0).map(|(_, _, text)| text).collect();
        host.release();
        assert_eq!(
            messages,
            [
                "setParamNormalized: no parameter has id 999",
                "process refused: no process data (×1)",
            ]
        );
    }
}

/// A host's null channel pointer never reaches the plugin as a slice
/// (Miri checks no slice is built from null): a null input reads as
/// silence, a null output is written to scratch, and the real channels
/// run exactly as with real buffers.
#[test]
fn null_channel_pointers_read_silence_and_write_scratch() {
    let signal = test_signal(FRAMES * BLOCKS, 3);
    let run = |null_in_l: bool, null_out_l: bool| -> (Vec<f32>, Vec<f32>) {
        unsafe {
            let inst = create_instance();
            inst.setup(48_000.0, FRAMES as i32);
            let (mut all_l, mut all_r) = (Vec::new(), Vec::new());
            for b in 0..BLOCKS {
                let block = &signal[b * FRAMES..(b + 1) * FRAMES];
                let mut in_l = if null_in_l {
                    vec![0.0; FRAMES]
                } else {
                    block.to_vec()
                };
                let mut in_r = block.to_vec();
                let mut out_l = vec![0.0f32; FRAMES];
                let mut out_r = vec![0.0f32; FRAMES];
                let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
                if null_in_l {
                    ins[0] = ptr::null_mut();
                }
                let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
                if null_out_l {
                    outs[0] = ptr::null_mut();
                }
                inst.process(&mut ins, &mut outs, FRAMES);
                all_l.extend_from_slice(&out_l);
                all_r.extend_from_slice(&out_r);
            }
            inst.release();
            (all_l, all_r)
        }
    };
    let (ref_l, ref_r) = run(false, false);
    let (null_in_l, null_in_r) = run(true, false);
    assert!(
        null_in_l.iter().all(|&v| v == 0.0),
        "a null input is silence"
    );
    assert_eq!(null_in_r, ref_r, "the real channel is untouched");
    let (null_out_l, null_out_r) = run(false, true);
    assert!(
        null_out_l.iter().all(|&v| v == 0.0),
        "the host buffer was never written"
    );
    assert_eq!(null_out_r, ref_r);
    assert!(
        ref_l.iter().any(|&v| v != 0.0),
        "precondition: the fixture runs"
    );
}
