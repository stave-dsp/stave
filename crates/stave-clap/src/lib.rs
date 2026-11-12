//! CLAP backend for stave. Implements the CLAP 1.x C ABI from scratch and
//! adapts it to the host-agnostic `stave_core::Plugin` trait.
//!
//! Users invoke `stave_clap_export!(MyPlugin)` once in their cdylib to expose
//! the `clap_entry` symbol that DAWs look for.

#![deny(rust_2018_idioms)]

pub(crate) mod ports;
/// The raw CLAP ABI (`stave-clap-sys`), re-exported for the export macros.
pub use stave_clap_sys as sys;
pub mod wrapper;

pub use stave_core;
/// The design system, for a plugin's face (`FaceSpec`, `HasFace`).
pub use stave_face;

/// Exports a type implementing `stave_core::Plugin` as a CLAP plugin.
///
/// Emits all the per-type statics CLAP requires (descriptor, features array,
/// factory) plus the `clap_entry` symbol that the host dynamically loads.
/// Invoke exactly once per `cdylib` crate.
#[macro_export]
macro_rules! stave_clap_export {
    ($engine_ty:ty) => {
        const _STAVE_DESC: $crate::stave_core::PluginDescriptor =
            <$crate::stave_core::StavePlugin<$engine_ty> as $crate::stave_core::Plugin>::DESCRIPTOR;

        // 17 = 16 features + trailing NULL. FeaturesArray::new asserts the
        // declared feature count fits.
        static _STAVE_FEATURES: $crate::wrapper::FeaturesArray<17> =
            $crate::wrapper::FeaturesArray::new(_STAVE_DESC.features);

        static _STAVE_CLAP_DESC: $crate::sys::ClapPluginDescriptor =
            $crate::sys::ClapPluginDescriptor {
                clap_version: $crate::sys::CLAP_VERSION,
                id: _STAVE_DESC.id.as_ptr(),
                name: _STAVE_DESC.name.as_ptr(),
                vendor: _STAVE_DESC.vendor.as_ptr(),
                url: _STAVE_DESC.url.as_ptr(),
                manual_url: ::core::ptr::null(),
                support_url: ::core::ptr::null(),
                version: _STAVE_DESC.version.as_ptr(),
                description: _STAVE_DESC.description.as_ptr(),
                features: _STAVE_FEATURES.as_ptr(),
            };

        static _STAVE_CLAP_FACTORY: $crate::sys::ClapPluginFactory =
            $crate::sys::ClapPluginFactory {
                get_plugin_count: ::core::option::Option::Some(
                    $crate::wrapper::factory_get_plugin_count::<
                        $crate::stave_core::StavePlugin<$engine_ty>,
                    >,
                ),
                get_plugin_descriptor: ::core::option::Option::Some(
                    $crate::wrapper::factory_get_plugin_descriptor::<
                        $crate::stave_core::StavePlugin<$engine_ty>,
                    >,
                ),
                create_plugin: ::core::option::Option::Some(
                    $crate::wrapper::factory_create_plugin::<
                        $crate::stave_core::StavePlugin<$engine_ty>,
                    >,
                ),
            };

        impl $crate::wrapper::ClapEntry for $engine_ty {
            fn clap_descriptor() -> &'static $crate::sys::ClapPluginDescriptor {
                &_STAVE_CLAP_DESC
            }
            fn clap_factory() -> &'static $crate::sys::ClapPluginFactory {
                &_STAVE_CLAP_FACTORY
            }
        }

        #[no_mangle]
        pub static clap_entry: $crate::sys::ClapPluginEntry = $crate::sys::ClapPluginEntry {
            clap_version: $crate::sys::CLAP_VERSION,
            init: ::core::option::Option::Some($crate::wrapper::entry_init),
            deinit: ::core::option::Option::Some($crate::wrapper::entry_deinit),
            get_factory: ::core::option::Option::Some(
                $crate::wrapper::entry_get_factory::<$crate::stave_core::StavePlugin<$engine_ty>>,
            ),
        };
    };

    (raw $plugin_ty:ty) => {
        const _STAVE_DESC: $crate::stave_core::PluginDescriptor =
            <$plugin_ty as $crate::stave_core::Plugin>::DESCRIPTOR;

        // 17 = 16 features + trailing NULL. FeaturesArray::new asserts the
        // declared feature count fits.
        static _STAVE_FEATURES: $crate::wrapper::FeaturesArray<17> =
            $crate::wrapper::FeaturesArray::new(_STAVE_DESC.features);

        static _STAVE_CLAP_DESC: $crate::sys::ClapPluginDescriptor =
            $crate::sys::ClapPluginDescriptor {
                clap_version: $crate::sys::CLAP_VERSION,
                id: _STAVE_DESC.id.as_ptr(),
                name: _STAVE_DESC.name.as_ptr(),
                vendor: _STAVE_DESC.vendor.as_ptr(),
                url: _STAVE_DESC.url.as_ptr(),
                manual_url: ::core::ptr::null(),
                support_url: ::core::ptr::null(),
                version: _STAVE_DESC.version.as_ptr(),
                description: _STAVE_DESC.description.as_ptr(),
                features: _STAVE_FEATURES.as_ptr(),
            };

        static _STAVE_CLAP_FACTORY: $crate::sys::ClapPluginFactory =
            $crate::sys::ClapPluginFactory {
                get_plugin_count: ::core::option::Option::Some(
                    $crate::wrapper::factory_get_plugin_count::<$plugin_ty>,
                ),
                get_plugin_descriptor: ::core::option::Option::Some(
                    $crate::wrapper::factory_get_plugin_descriptor::<$plugin_ty>,
                ),
                create_plugin: ::core::option::Option::Some(
                    $crate::wrapper::factory_create_plugin::<$plugin_ty>,
                ),
            };

        impl $crate::wrapper::ClapExport for $plugin_ty {
            fn clap_descriptor() -> &'static $crate::sys::ClapPluginDescriptor {
                &_STAVE_CLAP_DESC
            }
            fn clap_factory() -> &'static $crate::sys::ClapPluginFactory {
                &_STAVE_CLAP_FACTORY
            }
        }

        #[no_mangle]
        pub static clap_entry: $crate::sys::ClapPluginEntry = $crate::sys::ClapPluginEntry {
            clap_version: $crate::sys::CLAP_VERSION,
            init: ::core::option::Option::Some($crate::wrapper::entry_init),
            deinit: ::core::option::Option::Some($crate::wrapper::entry_deinit),
            get_factory: ::core::option::Option::Some(
                $crate::wrapper::entry_get_factory::<$plugin_ty>,
            ),
        };
    };
}
