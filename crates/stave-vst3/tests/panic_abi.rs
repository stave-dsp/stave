//! A plugin panic stays inside the plugin: the call fails safe, the
//! instance goes silent, and the host process keeps running.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;

use stave_core::{Plugin, PluginDescriptor, ProcessCtx};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

struct Panics;

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
        Self
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
