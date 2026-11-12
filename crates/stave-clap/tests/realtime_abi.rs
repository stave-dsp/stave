//! The CLAP process path is realtime: a block carrying more parameter
//! changes than a stack sort buffer holds (the standard stable sort's
//! cue to allocate) is ordered in place, the bypass param comes from the
//! table's cached index, and the whole `process` call allocates nothing.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use stave_clap::sys::*;
use stave_core::params::{BoolParam, FloatParam, ParamFlags, Range, Taper};
use stave_core::{PluginDescriptor, ProcessCtx, Processor, Spec};

// ---------------------------------------------------------------------------
// Allocation counting, per thread, only while armed.
// ---------------------------------------------------------------------------

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ARMED.try_with(|armed| {
            if armed.get() {
                COUNT.with(|c| c.set(c.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ARMED.try_with(|armed| {
            if armed.get() {
                COUNT.with(|c| c.set(c.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn allocations_in(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

// ---------------------------------------------------------------------------
// Fixture: a StavePlugin (indexed table) with many params and a bypass.
// ---------------------------------------------------------------------------

const PARAMS: u32 = 64;
const BYPASS_ID: u32 = 1_000;

struct WideParams {
    knobs: Vec<FloatParam>,
    bypass: BoolParam,
}

impl stave_core::params::Params for WideParams {
    fn count(&self) -> usize {
        self.knobs.len() + 1
    }
    fn by_index(&self, index: usize) -> Option<&dyn stave_core::params::AnyParam> {
        match self.knobs.get(index) {
            Some(knob) => Some(knob),
            None => (index == self.knobs.len()).then_some(&self.bypass as _),
        }
    }
}

/// What the plugin saw: its block's changes were in (frame, arrival)
/// order, and how many there were.
static SORTED: AtomicBool = AtomicBool::new(false);
static SEEN: AtomicUsize = AtomicUsize::new(0);

struct Wide;

impl Processor for Wide {
    type Params = WideParams;
    fn new(_params: Arc<WideParams>) -> Self {
        Wide
    }
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let changes = ctx.params.changes();
        // Each event's value encodes its arrival order.
        let sorted = changes.windows(2).all(|w| {
            (w[0].sample_offset, w[0].plain_value) < (w[1].sample_offset, w[1].plain_value)
        });
        SORTED.store(sorted, Ordering::SeqCst);
        SEEN.store(changes.len(), Ordering::SeqCst);
        for c in 0..ctx.output.channel_count() {
            ctx.output
                .channel(c)
                .copy_from_slice(ctx.input.channel_ref(c));
        }
    }
}

impl Spec for Wide {
    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.stave.test.wide-clap",
        name: c"Wide (CLAP)",
        vendor: c"stave tests",
        version: c"0.0.0",
        url: c"",
        description: c"realtime fixture",
        features: &[],
    };
    fn params() -> Arc<WideParams> {
        Arc::new(WideParams {
            knobs: (0..PARAMS)
                .map(|i| FloatParam::new(i, "knob", 0.0, Range::new(0.0, 1.0, Taper::Linear)))
                .collect(),
            bypass: BoolParam::new(BYPASS_ID, "bypass", false).with_flags(ParamFlags {
                bypass: true,
                ..Default::default()
            }),
        })
    }
}

stave_clap::stave_clap_export!(Wide);

// ---------------------------------------------------------------------------
// Host side: an input event list of param values.
// ---------------------------------------------------------------------------

struct Events(Vec<ClapEventParamValue>);

unsafe extern "C" fn events_size(list: *const ClapInputEvents) -> u32 {
    unsafe { (*((*list).ctx as *const Events)).0.len() as u32 }
}

unsafe extern "C" fn events_get(
    list: *const ClapInputEvents,
    index: u32,
) -> *const ClapEventHeader {
    let events = unsafe { &*((*list).ctx as *const Events) };
    events
        .0
        .get(index as usize)
        .map_or(ptr::null(), |e| &e.header as *const ClapEventHeader)
}

fn param_event(time: u32, id: u32, value: f64) -> ClapEventParamValue {
    ClapEventParamValue {
        header: ClapEventHeader {
            size: size_of::<ClapEventParamValue>() as u32,
            time,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            r#type: CLAP_EVENT_PARAM_VALUE,
            flags: 0,
        },
        param_id: id,
        cookie: ptr::null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value,
    }
}

const FRAMES: usize = 64;

#[test]
fn a_block_of_many_param_changes_is_sorted_without_allocating() {
    unsafe {
        let factory = (clap_entry.get_factory.unwrap())(CLAP_PLUGIN_FACTORY_ID.as_ptr())
            .cast::<ClapPluginFactory>();
        let plugin = ((*factory).create_plugin.unwrap())(
            factory,
            ptr::null(),
            <Wide as Spec>::DESCRIPTOR.id.as_ptr(),
        );
        assert!(!plugin.is_null());
        assert!(((*plugin).init.unwrap())(plugin));
        assert!(((*plugin).activate.unwrap())(
            plugin,
            48_000.0,
            1,
            FRAMES as u32
        ));

        // 250 changes, frames descending — the worst case for an insertion
        // sort, and past the stack buffer of the standard stable sort —
        // with runs of equal frames whose arrival order must survive.
        // Values encode arrival order (the bypass, off, arrives first).
        let mut events = vec![param_event(0, BYPASS_ID, 0.0)];
        events.extend(
            (0..250u32).map(|i| param_event((249 - i) / 4, i % PARAMS, f64::from(i + 1) / 1_000.0)),
        );
        let events = Events(events);
        let list = ClapInputEvents {
            ctx: &events as *const Events as *mut c_void,
            size: Some(events_size),
            get: Some(events_get),
        };

        let mut in_l = vec![0.25f32; FRAMES];
        let mut in_r = vec![0.25f32; FRAMES];
        let mut out_l = vec![0.0f32; FRAMES];
        let mut out_r = vec![0.0f32; FRAMES];
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
        let process = ClapProcess {
            steady_time: -1,
            frames_count: FRAMES as u32,
            transport: ptr::null(),
            audio_inputs: &in_buf,
            audio_outputs: &mut out_buf,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &list,
            out_events: ptr::null(),
        };
        let process_fn = (*plugin).process.unwrap();
        // Warm up once (lazy statics, first-touch), then count.
        assert_eq!(process_fn(plugin, &process), CLAP_PROCESS_CONTINUE);
        let mut status = 0;
        let allocations = allocations_in(|| status = process_fn(plugin, &process));
        assert_eq!(status, CLAP_PROCESS_CONTINUE);
        assert_eq!(allocations, 0, "the process call allocated");
        assert_eq!(SEEN.load(Ordering::SeqCst), 251);
        assert!(
            SORTED.load(Ordering::SeqCst),
            "changes reach the plugin in (frame, arrival) order"
        );
        assert_eq!(out_l, in_l, "not bypassed: the plugin's copy");

        ((*plugin).destroy.unwrap())(plugin);
    }
}
