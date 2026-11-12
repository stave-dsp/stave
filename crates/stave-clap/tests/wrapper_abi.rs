//! ABI-level soundness tests for the CLAP wrapper, mirroring
//! `stave-vst3/tests/wrapper_abi.rs`: drive the plugin through the real
//! `clap_entry` → factory → create → activate → process path.
//!
//! In-place processing bit-exact vs split buffers; two simultaneously-live
//! instances keep independent DSP state (proven through delay-line state).

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract. Miri runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ptr;

use stave_clap::sys::*;
use stave_core::runtime::Erased;
use stave_core::{Block, Info, Tail, Telemetry};

// Same shape as the VST3 fixture: linear (gain) + stateful (delay).
const TAPS: usize = 16;

struct TestEcho {
    dly: [[f32; TAPS]; 2],
    pos: usize,
}

impl Erased for TestEcho {
    const INFO: Info = Info {
        id: "com.stave.test.echo-clap",
        name: "Test Echo (CLAP)",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "ABI soundness fixture",
        features: &[],
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self {
            dly: [[0.0; TAPS]; 2],
            pos: 0,
        }
    }

    fn reset(&mut self) {
        self.dly = [[0.0; TAPS]; 2];
        self.pos = 0;
    }

    fn tail(&self) -> Tail {
        Tail::Samples(TAPS as u32)
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let chans = block.output().channel_count()
            .min(2)
            .min(block.input().channel_count());
        for f in 0..block.frames() {
            for c in 0..chans {
                let x = block.input().channel(c)[f];
                let d = self.dly[c][self.pos];
                self.dly[c][self.pos] = x;
                block.output().channel_mut(c)[f] = 0.5 * x + 0.25 * d;
            }
            self.pos = (self.pos + 1) % TAPS;
        }
    }
}

impl stave_clap::wrapper::ClapExport for TestEcho {}

stave_clap::export_clap!(TestEcho);

// ===========================================================================
// Minimal host plumbing
// ===========================================================================

struct HostInstance {
    plugin: *const ClapPlugin,
}

unsafe fn create_instance() -> HostInstance {
    let get_factory = clap_entry.get_factory.expect("entry has get_factory");
    let factory =
        unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const ClapPluginFactory;
    assert!(!factory.is_null(), "factory lookup failed");
    let create = unsafe { (*factory).create_plugin.expect("factory has create_plugin") };
    let plugin = unsafe { create(factory, ptr::null(), c"com.stave.test.echo-clap".as_ptr()) };
    assert!(!plugin.is_null(), "create_plugin failed");
    let init = unsafe { (*plugin).init.expect("plugin has init") };
    assert!(unsafe { init(plugin) });
    HostInstance { plugin }
}

impl HostInstance {
    unsafe fn activate(&self, sample_rate: f64, max_frames: u32) {
        let f = unsafe { (*self.plugin).activate.expect("activate") };
        assert!(unsafe { f(self.plugin, sample_rate, 1, max_frames) });
    }

    unsafe fn process(&self, in_ptrs: &mut [*mut f32], out_ptrs: &mut [*mut f32], frames: usize) {
        let in_buf = ClapAudioBuffer {
            data32: in_ptrs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: in_ptrs.len() as u32,
            latency: 0,
            constant_mask: 0,
        };
        let mut out_buf = ClapAudioBuffer {
            data32: out_ptrs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: out_ptrs.len() as u32,
            latency: 0,
            constant_mask: 0,
        };
        let process = ClapProcess {
            steady_time: -1,
            frames_count: frames as u32,
            transport: ptr::null(),
            audio_inputs: &in_buf,
            audio_outputs: &mut out_buf,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: ptr::null(),
            out_events: ptr::null(),
        };
        let f = unsafe { (*self.plugin).process.expect("process") };
        assert_eq!(unsafe { f(self.plugin, &process) }, CLAP_PROCESS_CONTINUE);
    }

    unsafe fn destroy(self) {
        let f = unsafe { (*self.plugin).destroy.expect("destroy") };
        unsafe { f(self.plugin) };
    }
}

fn test_signal(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2654435761).max(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

const FRAMES: usize = 64;
const BLOCKS: usize = 8;

// ===========================================================================
// Checks
// ===========================================================================

#[test]
fn in_place_processing_matches_split_buffers() {
    let signal_l = test_signal(FRAMES * BLOCKS, 1);
    let signal_r = test_signal(FRAMES * BLOCKS, 2);

    let reference = unsafe {
        let inst = create_instance();
        inst.activate(48_000.0, FRAMES as u32);
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
        inst.destroy();
        out_all
    };

    let in_place = unsafe {
        let inst = create_instance();
        inst.activate(48_000.0, FRAMES as u32);
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
        inst.destroy();
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

/// Host reset clears DSP state (no stale ring-in), and
/// the tail extension reports the trait's value.
#[test]
fn reset_clears_state_and_tail_is_reported() {
    unsafe {
        let inst = create_instance();
        inst.activate(48_000.0, FRAMES as u32);

        // clap.tail reflects Tail::Samples(16).
        let get_ext = (*inst.plugin).get_extension.expect("get_extension");
        let tail_ext = get_ext(inst.plugin, CLAP_EXT_TAIL.as_ptr());
        assert!(!tail_ext.is_null(), "clap.tail must be served");
        let tail_ext = &*(tail_ext as *const ClapPluginTail);
        assert_eq!((tail_ext.get.unwrap())(inst.plugin), TAPS as u32);

        // Prime the delay line with signal…
        let sig = test_signal(FRAMES, 5);
        let mut in_l = sig.clone();
        let mut in_r = sig.clone();
        let mut out_l = vec![0.0f32; FRAMES];
        let mut out_r = vec![0.0f32; FRAMES];
        let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        inst.process(&mut ins, &mut outs, FRAMES);

        // …host resets…
        ((*inst.plugin).reset.expect("reset"))(inst.plugin);

        // …then silence in must be silence out: no tap residue.
        let mut zin_l = vec![0.0f32; FRAMES];
        let mut zin_r = vec![0.0f32; FRAMES];
        let mut zins = [zin_l.as_mut_ptr(), zin_r.as_mut_ptr()];
        inst.process(&mut zins, &mut outs, FRAMES);
        assert!(
            out_l.iter().all(|&v| v == 0.0),
            "stale delay state rang past reset"
        );
        inst.destroy();
    }
}

#[test]
fn two_instances_keep_independent_state() {
    unsafe {
        let a = create_instance();
        let b = create_instance();
        a.activate(48_000.0, FRAMES as u32);
        b.activate(48_000.0, FRAMES as u32);

        let sig = test_signal(FRAMES, 7);
        let silence = vec![0.0f32; FRAMES];

        let run = |inst: &HostInstance, input: &[f32]| -> Vec<f32> {
            let mut in_l = input.to_vec();
            let mut in_r = input.to_vec();
            let mut out_l = vec![0.0f32; FRAMES];
            let mut out_r = vec![0.0f32; FRAMES];
            let mut ins = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
            let mut outs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
            inst.process(&mut ins, &mut outs, FRAMES);
            out_l
        };

        // Prime A's delay line with signal; B's with silence.
        run(&a, &sig);
        run(&b, &silence);

        // Same input now — outputs must differ exactly by A's primed
        // delay state: y = 0.5·x + 0.25·x[n−16], where the first TAPS
        // frames pull from the PREVIOUS block's tail.
        let out_a = run(&a, &sig);
        let out_b = run(&b, &sig);

        let tail = &sig[FRAMES - TAPS..]; // what block 1 left in the line
        for f in 0..TAPS {
            let expect_a = 0.5 * sig[f] + 0.25 * tail[f];
            let expect_b = 0.5 * sig[f]; // B's line held silence
            assert_eq!(out_a[f].to_bits(), expect_a.to_bits(), "A frame {f}");
            assert_eq!(out_b[f].to_bits(), expect_b.to_bits(), "B frame {f}");
        }

        a.destroy();
        b.destroy();
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
            inst.activate(48_000.0, FRAMES as u32);
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
            inst.destroy();
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
