//! A breakpoint's pause, through the real CLAP entry: the plugin keeps
//! running, the capture shows what it produced, and the host hears
//! silence flagged constant — what the VST3 wrapper does
//! (`mute_if_paused` serves both).

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract. Miri runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use stave_clap::sys::*;
use stave_core::runtime::Erased;
use stave_core::{brk_if, Block, Info, Telemetry};

/// The instance's telemetry, as the test sees it (one instance at a time).
static TELEMETRY: Mutex<Option<Arc<Telemetry>>> = Mutex::new(None);
/// Blocks the plugin ran: a paused plugin still runs.
static RAN: AtomicU32 = AtomicU32::new(0);

/// Writes 1.0 everywhere and checks the "loud" breakpoint each block.
struct Loud {
    telemetry: Arc<Telemetry>,
}

impl Erased for Loud {
    const INFO: Info = Info {
        id: "com.stave.test.pause-clap",
        name: "Pause (CLAP)",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "breakpoint pause fixture",
        features: &[],
    };

    fn new(telemetry: &Telemetry) -> Self {
        let telemetry = telemetry.shared();
        *TELEMETRY.lock().unwrap() = Some(telemetry.clone());
        Self { telemetry }
    }

    fn process(&mut self, block: &mut Block<'_>) {
        RAN.fetch_add(1, Ordering::Relaxed);
        for c in 0..block.output().channel_count() {
            block.output().channel_mut(c).fill(1.0);
        }
        brk_if!(self.telemetry, "loud", true);
    }
}

impl stave_clap::wrapper::ClapExport for Loud {}

stave_clap::export_clap!(Loud);

/// Run one block of `FRAMES` through `plugin`; returns the output and
/// its `constant_mask`.
unsafe fn run_block(plugin: *const ClapPlugin) -> ([Vec<f32>; 2], u64) {
    const FRAMES: usize = 32;
    let mut ins = [vec![0.25f32; FRAMES], vec![0.25f32; FRAMES]];
    let mut outs = [vec![0.5f32; FRAMES], vec![0.5f32; FRAMES]];
    let mut in_ptrs = [ins[0].as_mut_ptr(), ins[1].as_mut_ptr()];
    let mut out_ptrs = [outs[0].as_mut_ptr(), outs[1].as_mut_ptr()];
    let in_buf = ClapAudioBuffer {
        data32: in_ptrs.as_mut_ptr(),
        data64: ptr::null_mut(),
        channel_count: 2,
        latency: 0,
        constant_mask: 0,
    };
    let mut out_buf = ClapAudioBuffer {
        data32: out_ptrs.as_mut_ptr(),
        data64: ptr::null_mut(),
        channel_count: 2,
        latency: 0,
        // Stale bits from the host: the plugin must set the mask itself.
        constant_mask: u64::MAX,
    };
    let process = ClapProcess {
        steady_time: -1,
        frames_count: FRAMES as u32,
        transport: ptr::null(),
        audio_inputs: &in_buf,
        audio_outputs: &mut out_buf,
        audio_inputs_count: 1,
        audio_outputs_count: 1,
        in_events: ptr::null(),
        out_events: ptr::null(),
    };
    let f = unsafe { (*plugin).process.expect("process") };
    assert_eq!(unsafe { f(plugin, &process) }, CLAP_PROCESS_CONTINUE);
    let mask = out_buf.constant_mask;
    (outs, mask)
}

#[test]
fn an_armed_breakpoint_mutes_the_clap_output_but_not_the_capture() {
    let get_factory = clap_entry.get_factory.expect("entry has get_factory");
    let factory =
        unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const ClapPluginFactory;
    let create = unsafe { (*factory).create_plugin.expect("create_plugin") };
    let plugin = unsafe { create(factory, ptr::null(), c"com.stave.test.pause-clap".as_ptr()) };
    assert!(!plugin.is_null());
    assert!(unsafe { (*plugin).init.unwrap()(plugin) });
    assert!(unsafe { (*plugin).activate.unwrap()(plugin, 48_000.0, 1, 512) });
    let telemetry = TELEMETRY.lock().unwrap().clone().expect("the plugin was built");

    // Unarmed: the plugin's output reaches the host, nothing flagged.
    let (outs, mask) = unsafe { run_block(plugin) };
    assert!(outs.iter().flatten().all(|&s| s == 1.0));
    assert_eq!(mask, 0, "a sounding block clears the constant mask");
    assert!(!telemetry.is_paused());

    // Armed, mute on hit: the breakpoint fires and the host hears silence.
    let bp = telemetry.breakpoints().find(|b| b.name() == "loud").expect("registered");
    bp.set_mute_on_hit(true);
    bp.set_armed(true);
    let ran = RAN.load(Ordering::Relaxed);
    let (outs, mask) = unsafe { run_block(plugin) };
    assert!(telemetry.is_paused());
    assert_eq!(bp.hits(), 1);
    assert_eq!(RAN.load(Ordering::Relaxed), ran + 1, "a paused plugin keeps running");
    assert!(outs.iter().flatten().all(|&s| s == 0.0), "paused output is silent");
    assert_eq!(mask, 0b11, "both channels flagged constant");
    // The capture holds what the plugin produced, not the mute.
    let mut capture = [0.0f32; 32];
    let (len, _) = telemetry.read_capture_channel(0, &mut capture);
    let captured = &capture[..len as usize];
    assert!(!captured.is_empty() && captured.iter().all(|&s| s == 1.0), "capture: {captured:?}");

    // Disarmed and resumed: sound again.
    bp.set_armed(false);
    telemetry.set_paused(false);
    let (outs, mask) = unsafe { run_block(plugin) };
    assert!(outs.iter().flatten().all(|&s| s == 1.0));
    assert_eq!(mask, 0);

    unsafe { (*plugin).deactivate.unwrap()(plugin) };
    unsafe { (*plugin).destroy.unwrap()(plugin) };
}
