//! A plugin panic stays inside the plugin: the call fails safe, the
//! instance goes silent, and the host process keeps running.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use stave_core::params::{AnyParam, Params};
use stave_core::{Plugin, PluginDescriptor, ProcessCtx};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

struct Panics;

/// Arms a panic in `Panics::new()`.
static PANIC_IN_NEW: AtomicBool = AtomicBool::new(false);
/// Arms a panic in the param table's `count()` (reached from editor code).
static PANIC_IN_TABLE: AtomicBool = AtomicBool::new(false);
/// The tests share the arming statics and run on parallel threads.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An empty table whose `count()` panics while armed.
struct TrapTable;

impl Params for TrapTable {
    fn count(&self) -> usize {
        assert!(!PANIC_IN_TABLE.load(Ordering::SeqCst), "a param table bug");
        0
    }
    fn by_index(&self, _index: usize) -> Option<&dyn AnyParam> {
        None
    }
}

impl Plugin for Panics {
    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.stave.test.panics",
        name: c"Panics",
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

    fn param_table(&self) -> Arc<dyn Params> {
        Arc::new(TrapTable)
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        ctx.output.channel(0).fill(0.5);
        panic!("a plugin bug");
    }
}

stave_vst3::stave_vst3_export!(
    raw Panics,
    component_cid: (0x7e57_0101, 0x7e57_0102, 0x7e57_0103, 0x7e57_0104),
    controller_cid: (0x7e57_0101, 0x7e57_0102, 0x7e57_0105, 0x7e57_0106),
);

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

#[test]
fn a_panicking_process_is_contained_and_the_instance_goes_silent() {
    let _serial = serial();
    unsafe {
        let factory = GetPluginFactory() as *mut FactoryObj;
        let cid = <Panics as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
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
        let processor = proc_obj.cast::<FUnknown>();
        let ap = &*(*processor.cast::<RawObj<IAudioProcessorVtbl>>()).vtbl;
        let mut setup = ProcessSetup {
            process_mode: 0,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: 64,
            sample_rate: 48_000.0,
        };
        assert_eq!((ap.setup_processing)(processor, &mut setup), K_RESULT_OK);
        assert_eq!((comp.set_active)(component, 1), K_RESULT_OK);

        let mut input = [[0.25f32; 64]; 2];
        let mut output = [[1.0f32; 64]; 2];
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
            num_samples: 64,
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
        assert_eq!((ap.process)(processor, &mut data), K_INTERNAL_ERROR);
        assert_eq!((ap.process)(processor, &mut data), K_INTERNAL_ERROR);
        assert_eq!((comp.set_active)(component, 0), K_INTERNAL_ERROR);
        assert_eq!(out_bus.silence_flags, 0b11);
        (comp.release)(processor);
        (comp.release)(component);
        assert_eq!(
            output, [[0.0; 64]; 2],
            "silence, not the half-written block"
        );
    }
}

/// `createInstance` for the component class; returns the tresult and the
/// object written to `obj`.
unsafe fn create_component() -> (Tresult, *mut FUnknown) {
    unsafe {
        let factory = GetPluginFactory() as *mut FactoryObj;
        let cid = <Panics as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
        // Non-null sentinel: the call must overwrite it with null.
        let mut obj: *mut c_void = ptr::NonNull::dangling().as_ptr();
        let result = ((*(*factory).vtbl).create_instance)(
            factory.cast(),
            cid.as_ptr().cast(),
            IID_ICOMPONENT.as_ptr().cast(),
            &mut obj,
        );
        (result, obj.cast())
    }
}

/// S8: a panic in `P::new()` is an error code, not an unwind into the
/// host (which aborts at best).
#[test]
fn a_panicking_constructor_fails_create_instance() {
    let _serial = serial();
    unsafe {
        PANIC_IN_NEW.store(true, Ordering::SeqCst);
        let (result, obj) = create_component();
        PANIC_IN_NEW.store(false, Ordering::SeqCst);
        assert_eq!((result, obj), (K_INTERNAL_ERROR, ptr::null_mut()));

        // The factory still works afterwards.
        let (result, component) = create_component();
        assert_eq!(result, K_RESULT_OK);
        let comp = &*(*component.cast::<RawObj<IComponentVtbl>>()).vtbl;
        assert_eq!((comp.release)(component), 0);
    }
}

/// S8: a panic inside an `IPlugView` call is an error code; the instance
/// is disabled, and the view and instance still release cleanly.
#[test]
fn a_panicking_view_call_fails_safe() {
    let _serial = serial();
    unsafe {
        let (result, component) = create_component();
        assert_eq!(result, K_RESULT_OK);
        let comp = &*(*component.cast::<RawObj<IComponentVtbl>>()).vtbl;
        let mut ctl_obj: *mut c_void = ptr::null_mut();
        assert_eq!(
            (comp.query_interface)(component, &IID_IEDIT_CONTROLLER, &mut ctl_obj),
            K_RESULT_OK
        );
        let controller = ctl_obj.cast::<FUnknown>();
        let ec = &*(*controller.cast::<RawObj<IEditControllerVtbl>>()).vtbl;
        let view = (ec.create_view)(controller, c"editor".as_ptr());
        assert!(!view.is_null());
        let vt = &*(*view.cast::<RawObj<IPlugViewVtbl>>()).vtbl;

        // `attached` asks the table for its size before touching the
        // parent view, so the armed panic fires first and the bogus
        // (never dereferenced) parent is safe.
        PANIC_IN_TABLE.store(true, Ordering::SeqCst);
        let bogus_parent = ptr::NonNull::<c_void>::dangling().as_ptr();
        let result = (vt.attached)(view, bogus_parent, c"NSView".as_ptr());
        PANIC_IN_TABLE.store(false, Ordering::SeqCst);
        assert_eq!(result, K_INTERNAL_ERROR);

        // Disabled: later calls fail without running plugin code.
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        assert_eq!((vt.get_size)(view, &mut rect), K_INTERNAL_ERROR);
        assert!((ec.create_view)(controller, c"editor".as_ptr()).is_null());

        assert_eq!((vt.release)(view), 0);
        (comp.release)(controller);
        assert_eq!((comp.release)(component), 0);
    }
}
