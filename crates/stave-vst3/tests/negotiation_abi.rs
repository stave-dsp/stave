//! negotiation invariant, driven through the real VST3 vtable path: a
//! plugin declaring TWO port configurations (stereo+sidechain
//! preferred, plain stereo fallback). `setBusArrangements` selects
//! the declared member matching the WHOLE proposal; every bus query
//! then serves that member; a proposal matching no member is refused
//! and leaves the selection unchanged.

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract. Miri
// runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;

use stave_core::{Arrangement, AuxIn, Plugin, PluginDescriptor, PortConfig, ProcessCtx};
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

struct TwoConfig;

impl Plugin for TwoConfig {
    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"com.stave.test.twoconfig",
        name: c"Two Config",
        vendor: c"stave tests",
        version: c"0.0.0",
        url: c"",
        description: c"K4d negotiation fixture",
        features: &[],
    };

    const CONFIGS: &'static [PortConfig] = &[
        PortConfig {
            main_in: Some(Arrangement::Stereo),
            main_out: Arrangement::Stereo,
            aux_in: &[AuxIn {
                name: "sidechain",
                arrangement: Arrangement::Stereo,
            }],
        },
        PortConfig::STEREO_FX,
        PortConfig {
            main_in: Some(Arrangement::Surround51),
            main_out: Arrangement::Surround51,
            aux_in: &[],
        },
    ];

    fn new() -> Self {
        Self
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let chans = ctx.output.channel_count().min(ctx.input.channel_count());
        for c in 0..chans {
            for f in 0..ctx.frames {
                ctx.output.channel(c)[f] = ctx.input.channel_ref(c)[f];
            }
        }
    }
}

impl stave_vst3::stave_face::HasFace for TwoConfig {}

stave_vst3::stave_vst3_export!(
    raw TwoConfig,
    component_cid: (0x7e57_0011, 0x7e57_0012, 0x7e57_0013, 0x7e57_0014),
    controller_cid: (0x7e57_0011, 0x7e57_0012, 0x7e57_0015, 0x7e57_0016),
);

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

unsafe fn create() -> (*mut FUnknown, *mut FUnknown) {
    let factory = GetPluginFactory() as *mut FactoryObj;
    let fvtbl = unsafe { &*(*factory).vtbl };
    let cid = <TwoConfig as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
    let mut obj: *mut c_void = ptr::null_mut();
    let res = unsafe {
        (fvtbl.create_instance)(
            factory as *mut FUnknown,
            cid.as_ptr() as FidString,
            IID_ICOMPONENT.as_ptr() as FidString,
            &mut obj,
        )
    };
    assert_eq!(res, K_RESULT_OK);
    let component = obj as *mut FUnknown;
    let qi = unsafe { (*(*(component as *mut RawObj<IComponentVtbl>)).vtbl).query_interface };
    let mut proc_obj: *mut c_void = ptr::null_mut();
    assert_eq!(
        unsafe { qi(component, &IID_IAUDIO_PROCESSOR, &mut proc_obj) },
        K_RESULT_OK
    );
    (component, proc_obj as *mut FUnknown)
}

unsafe fn release(component: *mut FUnknown, processor: *mut FUnknown) {
    let rel = unsafe { (*(*(component as *mut RawObj<IComponentVtbl>)).vtbl).release };
    unsafe { rel(processor) };
    unsafe { rel(component) };
}

#[test]
fn set_bus_arrangements_selects_a_declared_member() {
    let (component, processor) = unsafe { create() };
    let cvt = unsafe { &*(*(component as *mut RawObj<IComponentVtbl>)).vtbl };
    let pvt = unsafe { &*(*(processor as *mut RawObj<IAudioProcessorVtbl>)).vtbl };

    // Preferred member served by default: main + sidechain.
    assert_eq!(
        unsafe { (cvt.get_bus_count)(component, MEDIA_TYPE_AUDIO, BUS_DIR_INPUT) },
        2,
        "member 0 declares main + aux"
    );

    // Host that won't wire an aux proposes plain stereo → member 1.
    let mut ins = [K_STEREO];
    let mut outs = [K_STEREO];
    assert_eq!(
        unsafe { (pvt.set_bus_arrangements)(processor, ins.as_mut_ptr(), 1, outs.as_mut_ptr(), 1) },
        K_RESULT_OK,
        "plain stereo matches the declared fallback member"
    );
    assert_eq!(
        unsafe { (cvt.get_bus_count)(component, MEDIA_TYPE_AUDIO, BUS_DIR_INPUT) },
        1,
        "selection is served by every bus query"
    );

    // Full sidechain proposal → back to member 0.
    let mut ins2 = [K_STEREO, K_STEREO];
    assert_eq!(
        unsafe {
            (pvt.set_bus_arrangements)(processor, ins2.as_mut_ptr(), 2, outs.as_mut_ptr(), 1)
        },
        K_RESULT_OK
    );
    assert_eq!(
        unsafe { (cvt.get_bus_count)(component, MEDIA_TYPE_AUDIO, BUS_DIR_INPUT) },
        2
    );
    let mut arr: SpeakerArrangement = 0;
    assert_eq!(
        unsafe { (pvt.get_bus_arrangement)(processor, BUS_DIR_INPUT, 1, &mut arr) },
        K_RESULT_OK
    );
    assert_eq!(arr, K_STEREO, "aux bus arrangement served from the member");

    // Surround proposal: the 5.1 member is selected and its
    // semantic arrangement is served back.
    let mut ins51 = [K_51];
    let mut outs51 = [K_51];
    assert_eq!(
        unsafe {
            (pvt.set_bus_arrangements)(processor, ins51.as_mut_ptr(), 1, outs51.as_mut_ptr(), 1)
        },
        K_RESULT_OK,
        "5.1 matches the declared surround member"
    );
    let mut arr51: SpeakerArrangement = 0;
    assert_eq!(
        unsafe { (pvt.get_bus_arrangement)(processor, BUS_DIR_INPUT, 0, &mut arr51) },
        K_RESULT_OK
    );
    assert_eq!(arr51, K_51, "semantic arrangement, not a channel count");

    // Back to the preferred member before the refusal check.
    let mut ins0 = [K_STEREO, K_STEREO];
    assert_eq!(
        unsafe {
            (pvt.set_bus_arrangements)(processor, ins0.as_mut_ptr(), 2, outs.as_mut_ptr(), 1)
        },
        K_RESULT_OK
    );

    // A proposal matching NO member is refused and changes nothing.
    let mut mono = [K_MONO];
    assert_eq!(
        unsafe {
            (pvt.set_bus_arrangements)(processor, mono.as_mut_ptr(), 1, outs.as_mut_ptr(), 1)
        },
        K_RESULT_FALSE,
        "no declared member is mono"
    );
    assert_eq!(
        unsafe { (cvt.get_bus_count)(component, MEDIA_TYPE_AUDIO, BUS_DIR_INPUT) },
        2,
        "refused proposal leaves the selection unchanged"
    );

    unsafe { release(component, processor) };
}
