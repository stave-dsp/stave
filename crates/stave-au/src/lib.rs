//! Audio Unit (v2) backend for stave. Implements the AU v2 plug-in
//! interface (`AudioComponentPlugInInterface` and the selectors its
//! `Lookup` serves) from scratch and drives a plugin through
//! `stave_core::runtime::Erased`, as the CLAP and VST3 backends do.
//!
//! Authors never name this crate: `stave::export!(MyPlugin)` emits the
//! component's factory function and the `AudioComponents` entry the
//! bundle step copies into the `.component`'s `Info.plist`.
//!
//! ```text
//! component.rs   identity: the type, the codes, the Info.plist entry (portable)
//! wrapper/       the plug-in interface (macOS)
//! ```

#![deny(rust_2018_idioms)]

pub mod component;
#[cfg(target_os = "macos")]
pub mod wrapper;

pub use stave_core;
/// The raw Audio Unit ABI (`stave-au-sys`).
pub use stave_au_sys as sys;
/// The design system, for a plugin's face.
pub use stave_face;

use stave_core::runtime::Erased;

/// What the Audio Unit wrapper needs beyond [`Erased`]: the version and
/// the face. `stave::export!` implements it.
pub trait AuExport: Erased {
    /// The version hosts show and the component's version number:
    /// `INFO.version`, or the exporting crate's.
    const VERSION: &'static str = Self::INFO.version;

    /// The plugin's face, if it has one.
    fn face(&self) -> Option<stave_face::Face> {
        None
    }
}

/// The component's factory function and its `Info.plist` entry for
/// `$plugin` (an [`AuExport`]). Once per cdylib; `stave::export!`
/// invokes it. Off macOS it emits nothing.
///
/// The entry lives in the binary's `__DATA,__stave_au` section, built at
/// compile time from the plugin's [`Info`](stave_core::Info), so the
/// bundle step reads the codes from the binary instead of restating
/// them.
#[doc(hidden)]
#[macro_export]
macro_rules! export_au {
    ($plugin:ty) => {
        /// The `factoryFunction` the component's Info.plist names.
        #[cfg(target_os = "macos")]
        #[no_mangle]
        pub extern "C" fn StaveAUFactory(
            desc: *const $crate::sys::AudioComponentDescription,
        ) -> *mut $crate::sys::AudioComponentPlugInInterface {
            $crate::wrapper::factory::<$plugin>(desc)
        }

        #[cfg(target_os = "macos")]
        #[used]
        #[link_section = "__DATA,__stave_au"]
        static STAVE_AU_COMPONENT: [u8; $crate::component::ENTRY_BYTES] =
            $crate::component::entry::<$plugin>().bytes();
    };
}
