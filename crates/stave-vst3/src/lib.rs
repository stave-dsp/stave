//! VST3 backend for stave. Implements the VST3 3.7.x SDK C++ ABI from
//! scratch and drives a plugin through `stave_core::runtime::Erased`.
//!
//! Authors never name this crate: `stave::export!(MyPlugin)` emits the
//! `GetPluginFactory` symbol DAWs look for, with class ids derived from
//! the plugin's `Info::id` ([`ids::derive_cid`]).

#![deny(rust_2018_idioms)]

pub(crate) mod buses;
pub(crate) mod events;
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
/// The design system, for a plugin's face.
pub use stave_face;

/// The VST3 entry symbols for `$plugin` (a [`wrapper::Vst3Export`]):
/// `GetPluginFactory`, the module entry and exit (`bundleEntry`/
/// `bundleExit`, plus `ModuleEntry`/`ModuleExit` on Linux and
/// `InitDll`/`ExitDll` on Windows), and
/// `stave_telemetry_for` (stave-aware hosts read an instance's telemetry
/// through it). Once per cdylib; `stave::export!` invokes it.
#[doc(hidden)]
#[macro_export]
macro_rules! export_vst3 {
    ($plugin:ty) => {
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

        /// Linux module entry (`ModuleEntry`), given the library's handle.
        #[cfg(all(unix, not(target_os = "macos")))]
        #[no_mangle]
        pub extern "C" fn ModuleEntry(_handle: *mut ::core::ffi::c_void) -> bool {
            true
        }

        /// Linux module exit.
        #[cfg(all(unix, not(target_os = "macos")))]
        #[no_mangle]
        pub extern "C" fn ModuleExit() -> bool {
            true
        }

        /// Windows module entry (`InitDll`, optional in the SDK).
        #[cfg(windows)]
        #[no_mangle]
        pub extern "C" fn InitDll() -> bool {
            true
        }

        /// Windows module exit.
        #[cfg(windows)]
        #[no_mangle]
        pub extern "C" fn ExitDll() -> bool {
            true
        }

        /// The plugin factory.
        #[no_mangle]
        pub extern "C" fn GetPluginFactory() -> *mut $crate::sys::FUnknown {
            $crate::wrapper::factory::<$plugin>()
        }

        /// The telemetry of the instance behind `component` (any of its
        /// interfaces), or null. Read only; valid while the instance lives.
        ///
        /// # Safety
        /// `component` is null or a live interface this plugin created.
        #[no_mangle]
        pub unsafe extern "C" fn stave_telemetry_for(
            component: *mut ::core::ffi::c_void,
        ) -> *const $crate::stave_core::Telemetry {
            // SAFETY: this function's contract is `telemetry_for`'s.
            unsafe { $crate::wrapper::telemetry_for::<$plugin>(component) }
        }
    };
}
