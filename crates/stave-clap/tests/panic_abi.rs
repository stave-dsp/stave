//! A plugin panic stays inside the plugin: the call fails safe, the
//! instance goes silent, and the host process keeps running.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ptr;

use stave_clap::sys::*;
use stave_core::{Plugin, PluginDescriptor, ProcessCtx};

struct Panics;

impl Plugin for Panics {
    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.stave.test.panics-clap",
        name: c"Panics (CLAP)",
        vendor: c"stave tests",
        version: c"0.0.0",
        url: c"",
        description: c"panic containment fixture",
        features: &[],
    };

    fn new() -> Self {
        Self
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        ctx.output.channel(0).fill(0.5);
        panic!("a plugin bug");
    }
}

stave_clap::stave_clap_export!(raw Panics);

#[test]
fn a_panicking_process_is_contained_and_the_instance_goes_silent() {
    unsafe {
        let get_factory = clap_entry.get_factory.unwrap();
        let factory = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<ClapPluginFactory>();
        let plugin = ((*factory).create_plugin.unwrap())(
            factory,
            ptr::null(),
            Panics::DESCRIPTOR.id.as_ptr(),
        );
        assert!(((*plugin).init.unwrap())(plugin));
        assert!(((*plugin).activate.unwrap())(plugin, 48_000.0, 1, 64));

        let mut left = [1.0f32; 64];
        let mut right = [1.0f32; 64];
        let mut channels = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut output = ClapAudioBuffer {
            data32: channels.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let block = ClapProcess {
            steady_time: -1,
            frames_count: 64,
            transport: ptr::null(),
            audio_inputs: ptr::null(),
            audio_outputs: &mut output,
            audio_inputs_count: 0,
            audio_outputs_count: 1,
            in_events: ptr::null(),
            out_events: ptr::null(),
        };
        let process = (*plugin).process.unwrap();
        assert_eq!(process(plugin, &block), CLAP_PROCESS_ERROR);
        assert_eq!(process(plugin, &block), CLAP_PROCESS_ERROR);
        assert!(!((*plugin).activate.unwrap())(plugin, 48_000.0, 1, 64));
        ((*plugin).destroy.unwrap())(plugin);
        assert_eq!(
            (left, right),
            ([0.0; 64], [0.0; 64]),
            "silence, not the half-written block"
        );
    }
}
