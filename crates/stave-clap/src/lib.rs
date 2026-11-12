//! CLAP backend for stave. Implements the CLAP 1.x C ABI from scratch and
//! drives a plugin through `stave_core::runtime::Erased`.
//!
//! Authors never name this crate: `stave::export!(MyPlugin)` emits the
//! `clap_entry` symbol DAWs look for.

#![deny(rust_2018_idioms)]

pub(crate) mod ports;
/// The raw CLAP ABI (`stave-clap-sys`), re-exported for the export macros.
pub use stave_clap_sys as sys;
pub mod wrapper;

pub use stave_core;
/// The design system, for a plugin's face.
pub use stave_face;

/// The `clap_entry` symbol for `$plugin` (a [`wrapper::ClapExport`]).
/// Once per cdylib; `stave::export!` invokes it.
#[doc(hidden)]
#[macro_export]
macro_rules! export_clap {
    ($plugin:ty) => {
        #[no_mangle]
        pub static clap_entry: $crate::sys::ClapPluginEntry = $crate::wrapper::entry::<$plugin>();
    };
}
