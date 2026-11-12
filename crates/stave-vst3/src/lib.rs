//! VST3 backend for stave. Implements the VST3 3.7.x SDK C++ ABI from
//! scratch and adapts it to the host-agnostic `stave_core::Plugin` trait.
//!
//! Users invoke `stave_vst3_export!(MyPlugin, component_cid, controller_cid)`
//! once in their cdylib to expose the `GetPluginFactory` symbol that DAWs
//! look for. Component and controller CIDs are 16-byte TUIDs the user
//! generates (e.g. via `uuidgen`); they must be globally unique.

#![deny(rust_2018_idioms)]

pub(crate) mod buses;
pub(crate) mod events;
#[cfg(target_os = "macos")]
pub(crate) mod gestures;
pub(crate) mod host;
pub(crate) mod host_context;
pub mod ids;
mod programs;
/// The raw VST3 ABI (`stave-vst3-sys`), re-exported for the export macros.
pub use stave_vst3_sys as sys;
mod units;
pub(crate) mod wire_strings;
pub mod wrapper;

pub use stave_core;
/// The design system, for a plugin's face (`FaceSpec`, `HasFace`).
pub use stave_face;

/// Exports a type implementing `stave_core::Plugin` as a VST3 plugin.
///
/// Emits per-type statics (vtables, factory object) plus the
/// `GetPluginFactory` / `bundleEntry` / `bundleExit` symbols hosts look up.
///
/// CIDs are pairs of `u32` written as four 32-bit hex constants, matching
/// the Steinberg SDK's `DECLARE_CLASS_IID(name, 0xXXXX, 0xYYYY, 0xZZZZ, 0xWWWW)`.
/// The macro converts them to the platform's `INLINE_UID` byte layout via
/// `make_tuid` (GUID order on Windows, big-endian words elsewhere).
///
/// Example:
/// ```ignore
/// stave_vst3_export!(
///     MySynth,
///     component_cid: (0x12345678, 0x9ABCDEF0, 0x11111111, 0x22222222),
///     controller_cid: (0x12345678, 0x9ABCDEF0, 0x33333333, 0x44444444),
/// );
/// ```
#[macro_export]
macro_rules! stave_vst3_export {
    (
        $engine_ty:ty,
        component_cid: ($c1:expr, $c2:expr, $c3:expr, $c4:expr),
        controller_cid: ($e1:expr, $e2:expr, $e3:expr, $e4:expr) $(,)?
    ) => {
        static _STAVE_VST3_COMPONENT_VTBL: $crate::sys::IComponentVtbl =
            $crate::wrapper::build_component_vtbl::<$crate::stave_core::StavePlugin<$engine_ty>>();
        static _STAVE_VST3_PROCESSOR_VTBL: $crate::sys::IAudioProcessorVtbl =
            $crate::wrapper::build_processor_vtbl::<$crate::stave_core::StavePlugin<$engine_ty>>();
        static _STAVE_VST3_CONTROLLER_VTBL: $crate::sys::IEditControllerVtbl =
            $crate::wrapper::build_controller_vtbl::<$crate::stave_core::StavePlugin<$engine_ty>>();
        static _STAVE_VST3_FACTORY_VTBL: $crate::sys::IPluginFactoryVtbl =
            $crate::wrapper::build_factory_vtbl::<$crate::stave_core::StavePlugin<$engine_ty>>();
        static _STAVE_VST3_FACTORY: $crate::wrapper::FactoryObj = $crate::wrapper::FactoryObj {
            vtbl: &_STAVE_VST3_FACTORY_VTBL,
        };

        impl $crate::ids::Vst3Ids for $engine_ty {
            const COMPONENT_CID: $crate::sys::Tuid = $crate::sys::make_tuid($c1, $c2, $c3, $c4);
            const CONTROLLER_CID: $crate::sys::Tuid = $crate::sys::make_tuid($e1, $e2, $e3, $e4);

            fn component_vtbl() -> *const $crate::sys::IComponentVtbl {
                &_STAVE_VST3_COMPONENT_VTBL
            }
            fn processor_vtbl() -> *const $crate::sys::IAudioProcessorVtbl {
                &_STAVE_VST3_PROCESSOR_VTBL
            }
            fn controller_vtbl() -> *const $crate::sys::IEditControllerVtbl {
                &_STAVE_VST3_CONTROLLER_VTBL
            }
        }

        /// VST3 bundle entry. Called when the host loads the bundle.
        #[no_mangle]
        pub extern "C" fn bundleEntry(_ref: *mut ::core::ffi::c_void) -> bool {
            true
        }

        /// VST3 bundle exit. Called when the host unloads the bundle.
        #[no_mangle]
        pub extern "C" fn bundleExit() -> bool {
            true
        }

        /// The host calls this to obtain the plugin factory. Returns a
        /// pointer to our static factory object.
        #[no_mangle]
        pub extern "C" fn GetPluginFactory() -> *mut $crate::sys::FUnknown {
            &_STAVE_VST3_FACTORY as *const $crate::wrapper::FactoryObj as *mut $crate::sys::FUnknown
        }

        /// stave-specific export. A telemetry-aware host (stave-play,
        /// stave-host) can dlsym this and pass a component pointer it got
        /// back from `createInstance` (any of IComponent / IAudioProcessor /
        /// IEditController, since they all share the same parent layout) to
        /// get a `*const Telemetry`. Returns null if the pointer is null.
        ///
        /// The returned pointer is owned by the plugin instance — do not
        /// free, only read. Valid until the instance is released.
        #[no_mangle]
        pub unsafe extern "C" fn stave_telemetry_for(
            component: *mut ::core::ffi::c_void,
        ) -> *const $crate::stave_core::Telemetry {
            if component.is_null() {
                return ::core::ptr::null();
            }
            // Every sub-object (component, processor, controller) starts with
            // { *const Vtbl; *mut Instance<P>; }, so the second pointer is the
            // instance regardless of which interface the caller passed.
            let obj = component
                as *const $crate::wrapper::ComponentObj<
                    $crate::stave_core::StavePlugin<$engine_ty>,
                >;
            let parent = unsafe { (*obj).parent };
            if parent.is_null() {
                return ::core::ptr::null();
            }
            $crate::wrapper::telemetry_ptr::<$crate::stave_core::StavePlugin<$engine_ty>>(unsafe {
                &*parent
            })
        }
    };

    (
        raw $plugin_ty:ty,
        component_cid: ($c1:expr, $c2:expr, $c3:expr, $c4:expr),
        controller_cid: ($e1:expr, $e2:expr, $e3:expr, $e4:expr) $(,)?
    ) => {
        static _STAVE_VST3_COMPONENT_VTBL: $crate::sys::IComponentVtbl =
            $crate::wrapper::build_component_vtbl::<$plugin_ty>();
        static _STAVE_VST3_PROCESSOR_VTBL: $crate::sys::IAudioProcessorVtbl =
            $crate::wrapper::build_processor_vtbl::<$plugin_ty>();
        static _STAVE_VST3_CONTROLLER_VTBL: $crate::sys::IEditControllerVtbl =
            $crate::wrapper::build_controller_vtbl::<$plugin_ty>();
        static _STAVE_VST3_FACTORY_VTBL: $crate::sys::IPluginFactoryVtbl =
            $crate::wrapper::build_factory_vtbl::<$plugin_ty>();
        static _STAVE_VST3_FACTORY: $crate::wrapper::FactoryObj = $crate::wrapper::FactoryObj {
            vtbl: &_STAVE_VST3_FACTORY_VTBL,
        };

        impl $crate::wrapper::Vst3Export for $plugin_ty {
            const COMPONENT_CID: $crate::sys::Tuid = $crate::sys::make_tuid($c1, $c2, $c3, $c4);
            const CONTROLLER_CID: $crate::sys::Tuid = $crate::sys::make_tuid($e1, $e2, $e3, $e4);

            fn component_vtbl() -> *const $crate::sys::IComponentVtbl {
                &_STAVE_VST3_COMPONENT_VTBL
            }
            fn processor_vtbl() -> *const $crate::sys::IAudioProcessorVtbl {
                &_STAVE_VST3_PROCESSOR_VTBL
            }
            fn controller_vtbl() -> *const $crate::sys::IEditControllerVtbl {
                &_STAVE_VST3_CONTROLLER_VTBL
            }
        }

        /// VST3 bundle entry. Called when the host loads the bundle.
        #[no_mangle]
        pub extern "C" fn bundleEntry(_ref: *mut ::core::ffi::c_void) -> bool {
            true
        }

        /// VST3 bundle exit. Called when the host unloads the bundle.
        #[no_mangle]
        pub extern "C" fn bundleExit() -> bool {
            true
        }

        /// The host calls this to obtain the plugin factory. Returns a
        /// pointer to our static factory object.
        #[no_mangle]
        pub extern "C" fn GetPluginFactory() -> *mut $crate::sys::FUnknown {
            &_STAVE_VST3_FACTORY as *const $crate::wrapper::FactoryObj as *mut $crate::sys::FUnknown
        }

        /// stave-specific export. A telemetry-aware host (stave-play,
        /// stave-host) can dlsym this and pass a component pointer it got
        /// back from `createInstance` (any of IComponent / IAudioProcessor /
        /// IEditController, since they all share the same parent layout) to
        /// get a `*const Telemetry`. Returns null if the pointer is null.
        ///
        /// The returned pointer is owned by the plugin instance — do not
        /// free, only read. Valid until the instance is released.
        #[no_mangle]
        pub unsafe extern "C" fn stave_telemetry_for(
            component: *mut ::core::ffi::c_void,
        ) -> *const $crate::stave_core::Telemetry {
            if component.is_null() {
                return ::core::ptr::null();
            }
            // Every sub-object (component, processor, controller) starts with
            // { *const Vtbl; *mut Instance<P>; }, so the second pointer is the
            // instance regardless of which interface the caller passed.
            let obj = component as *const $crate::wrapper::ComponentObj<$plugin_ty>;
            let parent = unsafe { (*obj).parent };
            if parent.is_null() {
                return ::core::ptr::null();
            }
            $crate::wrapper::telemetry_ptr::<$plugin_ty>(unsafe { &*parent })
        }
    };
}
