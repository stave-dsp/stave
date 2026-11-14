//! A5: a GUI fault refuses the window and never latches the audio
//! instance. On macOS, `IPlugView::attached` from a thread other than
//! the main one (a host bug: AppKit is main-thread only) answers
//! `kResultFalse`; the instance's panic latch stays clear, so later view
//! calls and the processor keep working. (libtest runs tests off the
//! main thread, which is exactly the violation.)

// Every `unsafe` block here plays the host: it calls the plugin's COM
// ABI with pointers this file owns, under the VST3 SDK's contract.
#![allow(clippy::undocumented_unsafe_blocks)]

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Arc;

    use stave_core::params::{FloatParam, Params, Range};
    use stave_core::runtime::Erased;
    use stave_core::{Block, Info, ParamId, Telemetry};
    use stave_vst3::stave_face;
    use stave_vst3::sys::*;
    use stave_vst3::wrapper::FactoryObj;

    #[derive(stave_core::Params)]
    struct OneKnob {
        drive: FloatParam,
    }

    struct Fixture {
        params: Arc<OneKnob>,
    }

    impl Erased for Fixture {
        const INFO: Info = Info {
            id: "com.stave.test.view-refused",
            name: "View Refused",
            vendor: "stave tests",
            version: "0.0.0",
            url: "",
            description: "IPlugView refusal fixture",
            features: &[],
        };
        fn new(_telemetry: &Telemetry) -> Self {
            Fixture {
                params: Arc::new(OneKnob {
                    drive: FloatParam::new(ParamId(1), "drive", Range::linear(0.0, 1.0)),
                }),
            }
        }
        fn params(&self) -> Arc<dyn Params> {
            self.params.clone()
        }
        fn process(&mut self, _block: &mut Block<'_>) {}
    }

    impl stave_vst3::wrapper::Vst3Export for Fixture {
        fn face(&self) -> Option<stave_face::Face> {
            use stave_face::prelude::*;
            Some(Face::new(
                Theme::graphite(),
                Knob::new(self.params.drive.bind()).node(),
            ))
        }
    }

    stave_vst3::export_vst3!(Fixture);

    #[repr(C)]
    struct RawObj<V> {
        vtbl: *const V,
    }

    #[test]
    fn attaching_off_the_main_thread_refuses_the_window_and_keeps_the_instance() {
        unsafe {
            let factory = GetPluginFactory() as *mut FactoryObj;
            let cid = <Fixture as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
            let mut obj: *mut c_void = ptr::null_mut();
            let res = ((*(*factory).vtbl).create_instance)(
                factory.cast(),
                cid.as_ptr().cast(),
                IID_ICOMPONENT.as_ptr().cast(),
                &mut obj,
            );
            assert_eq!(res, K_RESULT_OK);
            let component = obj.cast::<FUnknown>();
            let comp = &*(*component.cast::<RawObj<IComponentVtbl>>()).vtbl;
            let mut ctl: *mut c_void = ptr::null_mut();
            assert_eq!(
                (comp.query_interface)(component, &IID_IEDIT_CONTROLLER, &mut ctl),
                K_RESULT_OK
            );
            let controller = ctl.cast::<FUnknown>();
            let ec = &*(*controller.cast::<RawObj<IEditControllerVtbl>>()).vtbl;
            let view = (ec.create_view)(controller, c"editor".as_ptr());
            assert!(!view.is_null(), "createView");
            let vt = &*(*view.cast::<RawObj<IPlugViewVtbl>>()).vtbl;

            // A non-null "NSView" the plugin must never touch: the thread
            // is checked first.
            let parent = ptr::NonNull::<c_void>::dangling().as_ptr();
            assert_eq!(
                (vt.attached)(view, parent, c"NSView".as_ptr()),
                K_RESULT_FALSE,
                "refused (a panic would answer kInternalError and latch the instance)"
            );

            // Not latched: view calls still run plugin code...
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            assert_eq!((vt.get_size)(view, &mut rect), K_RESULT_OK);
            assert!(
                rect.right > rect.left && rect.bottom > rect.top,
                "the face's size"
            );
            // ...and the processor still activates.
            assert_eq!((comp.set_active)(component, 1), K_RESULT_OK);
            assert_eq!((comp.set_active)(component, 0), K_RESULT_OK);

            assert_eq!((vt.release)(view), 0);
            (comp.release)(controller);
            assert_eq!((comp.release)(component), 0);
        }
    }
}
