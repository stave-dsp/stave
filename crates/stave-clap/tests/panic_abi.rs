//! A plugin panic stays inside the plugin: the call fails safe, the
//! instance goes silent, and the host process keeps running.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use stave_clap::sys::*;
use stave_core::{Plugin, PluginDescriptor, ProcessCtx};

struct Panics;

/// Arms a panic in `Panics::new()`.
static PANIC_IN_NEW: AtomicBool = AtomicBool::new(false);
/// The tests share the arming static and run on parallel threads.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
        assert!(!PANIC_IN_NEW.load(Ordering::SeqCst), "a constructor bug");
        Self
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        ctx.output.channel(0).fill(0.5);
        panic!("a plugin bug");
    }
}

impl stave_clap::stave_face::HasFace for Panics {}

stave_clap::stave_clap_export!(raw Panics);

#[test]
fn a_panicking_process_is_contained_and_the_instance_goes_silent() {
    let _serial = serial();
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

/// S8: a panic in `P::new()` makes `create_plugin` return null instead
/// of unwinding into the host.
#[test]
fn a_panicking_constructor_fails_create_plugin() {
    let _serial = serial();
    unsafe {
        let get_factory = clap_entry.get_factory.unwrap();
        let factory = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<ClapPluginFactory>();
        let create = (*factory).create_plugin.unwrap();
        PANIC_IN_NEW.store(true, Ordering::SeqCst);
        let plugin = create(factory, ptr::null(), Panics::DESCRIPTOR.id.as_ptr());
        PANIC_IN_NEW.store(false, Ordering::SeqCst);
        assert!(plugin.is_null());

        // The factory still works afterwards.
        let plugin = create(factory, ptr::null(), Panics::DESCRIPTOR.id.as_ptr());
        assert!(!plugin.is_null());
        assert!(((*plugin).init.unwrap())(plugin));
        ((*plugin).destroy.unwrap())(plugin);
    }
}
