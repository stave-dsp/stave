//! Host input the wrapper must refuse without harm: a `clap_istream`
//! whose `read` claims more bytes than it was asked for is a refused
//! `state.load`, not a panic that disables the instance.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract. Miri runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;

use stave_clap::sys::*;
use stave_core::runtime::Erased;
use stave_core::{Block, Info, Telemetry};

struct Passthrough;

impl Erased for Passthrough {
    const INFO: Info = Info {
        id: "com.stave.test.ingress-clap",
        name: "Ingress (CLAP)",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "host-ingress fixture",
        features: &[],
        au: None,
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let chans = block.output().channel_count().min(block.input().channel_count());
        for c in 0..chans {
            for f in 0..block.frames() {
                block.output().channel_mut(c)[f] = block.input().channel(c)[f];
            }
        }
    }
}

impl stave_clap::wrapper::ClapExport for Passthrough {}

stave_clap::export_clap!(Passthrough);

/// Claims one byte more than the buffer it was handed (writing none).
unsafe extern "C" fn read_too_much(_stream: *const ClapIstream, _buffer: *mut c_void, size: u64) -> i64 {
    size as i64 + 1
}

unsafe fn process_one_block(plugin: *const ClapPlugin) -> ClapProcessStatus {
    let mut left = [0.25f32; 8];
    let mut right = [0.5f32; 8];
    let mut out_l = [0.0f32; 8];
    let mut out_r = [0.0f32; 8];
    let mut ins = [left.as_mut_ptr(), right.as_mut_ptr()];
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
    let process = ClapProcess {
        steady_time: -1,
        frames_count: 8,
        transport: ptr::null(),
        audio_inputs: &in_buf,
        audio_outputs: &mut out_buf,
        audio_inputs_count: 1,
        audio_outputs_count: 1,
        in_events: ptr::null(),
        out_events: ptr::null(),
    };
    let status = unsafe { ((*plugin).process.unwrap())(plugin, &process) };
    if status == CLAP_PROCESS_CONTINUE {
        assert_eq!(out_r, [0.5; 8], "the instance did not pass audio");
    }
    status
}

#[test]
fn a_stream_read_past_the_buffer_is_refused_and_the_instance_keeps_running() {
    unsafe {
        let get_factory = clap_entry.get_factory.unwrap();
        let factory = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<ClapPluginFactory>();
        let create = (*factory).create_plugin.unwrap();
        let plugin = create(factory, ptr::null(), c"com.stave.test.ingress-clap".as_ptr());
        assert!(((*plugin).init.unwrap())(plugin));
        assert!(((*plugin).activate.unwrap())(plugin, 48_000.0, 1, 64));
        assert!(((*plugin).start_processing.unwrap())(plugin));

        let get_ext = (*plugin).get_extension.unwrap();
        let state = &*get_ext(plugin, CLAP_EXT_STATE.as_ptr()).cast::<ClapPluginState>();
        let stream = ClapIstream {
            ctx: ptr::null_mut(),
            read: Some(read_too_much),
        };
        assert!(!(state.load.unwrap())(plugin, &stream), "an over-long read was accepted");
        assert_eq!(
            process_one_block(plugin),
            CLAP_PROCESS_CONTINUE,
            "a bad host stream disabled the instance"
        );

        ((*plugin).stop_processing.unwrap())(plugin);
        ((*plugin).deactivate.unwrap())(plugin);
        ((*plugin).destroy.unwrap())(plugin);
    }
}
