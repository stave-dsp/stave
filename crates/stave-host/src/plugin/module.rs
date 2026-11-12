//! A plugin MODULE: the four entry points a VST3 binary exports, and
//! the lifetime around them (`bundleEntry` … `bundleExit`, dlopen …
//! dlclose). Usually loaded from a bundle; tests and embedders can
//! hand in statically linked entry points instead.

use std::ffi::{c_void, CStr};
use std::path::Path;
use std::ptr::NonNull;

use stave_debug::DebugBus;
use stave_vst3_sys::FUnknown;

use super::bundle::Bundle;
use super::HostError;

/// The exports of a VST3 binary (plus stave's optional debug hook).
#[derive(Copy, Clone)]
pub struct EntryPoints {
    pub bundle_entry: unsafe extern "C" fn(*mut c_void) -> bool,
    pub bundle_exit: unsafe extern "C" fn() -> bool,
    pub get_factory: unsafe extern "C" fn() -> *mut FUnknown,
    /// `stave_debug_bus_for` — present only in stave-built plugins.
    pub debug_bus_for: Option<unsafe extern "C" fn(*mut c_void) -> *const DebugBus>,
}

/// The [`EntryPoints`] of a stave plugin crate linked into this binary
/// (the symbols `stave_vst3_export!` defines in it).
#[macro_export]
macro_rules! entry_points {
    ($plugin:path) => {{
        use $plugin as plugin;
        $crate::plugin::EntryPoints {
            bundle_entry: plugin::bundleEntry,
            bundle_exit: plugin::bundleExit,
            get_factory: plugin::GetPluginFactory,
            debug_bus_for: Some(plugin::stave_debug_bus_for),
        }
    }};
}

/// An entered module. Dropping it calls `bundleExit`, then unloads the
/// library (and releases the bundle) — in that order.
pub(crate) struct Module {
    entry: EntryPoints,
    _library: Option<Library>,
    _bundle: Option<Bundle>,
}

impl Module {
    /// dlopen the bundle's executable, resolve its exports, and enter it.
    pub fn load(path: &Path) -> Result<Self, HostError> {
        let bundle = Bundle::open(path);
        let library = Library::open(&bundle.executable())?;
        // SAFETY: the exported symbols have the VST3 SDK's signatures.
        let entry = unsafe {
            EntryPoints {
                bundle_entry: library.require(c"bundleEntry")?,
                bundle_exit: library.require(c"bundleExit")?,
                get_factory: library.require(c"GetPluginFactory")?,
                debug_bus_for: library.symbol(c"stave_debug_bus_for"),
            }
        };
        Self::enter(entry, Some(library), Some(bundle))
    }

    /// Enter statically linked entry points (no bundle, no dlopen).
    pub fn from_entry_points(entry: EntryPoints) -> Result<Self, HostError> {
        Self::enter(entry, None, None)
    }

    fn enter(
        entry: EntryPoints,
        library: Option<Library>,
        bundle: Option<Bundle>,
    ) -> Result<Self, HostError> {
        let cf_ref = bundle.as_ref().map_or(std::ptr::null_mut(), Bundle::cf_ref);
        // SAFETY: called once, before any other export (spec order).
        if !unsafe { (entry.bundle_entry)(cf_ref) } {
            return Err(HostError::BundleEntryFailed);
        }
        Ok(Self {
            entry,
            _library: library,
            _bundle: bundle,
        })
    }

    pub fn entry(&self) -> &EntryPoints {
        &self.entry
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: entered in `enter`; every object from this module has
        // been released by now (field order in LoadedPlugin).
        unsafe { (self.entry.bundle_exit)() };
    }
}

/// A dlopen'd library; dlclose on drop.
struct Library(NonNull<c_void>);

/// Resolve everything up front, and keep each plugin's symbols to
/// itself: macOS defaults to GLOBAL, and every VST3 exports the same
/// names. The values are platform-specific (Linux `0x4` is
/// `RTLD_NOLOAD`, not LOCAL), so they come from libc, never literals.
const DLOPEN_FLAGS: libc::c_int = libc::RTLD_NOW | libc::RTLD_LOCAL;

impl Library {
    fn open(path: &Path) -> Result<Self, HostError> {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| HostError::DlopenFailed(format!("path has a NUL: {}", path.display())))?;
        // SAFETY: plain libc call with a valid C string.
        let handle = unsafe { libc::dlopen(c_path.as_ptr(), DLOPEN_FLAGS) };
        NonNull::new(handle).map(Self).ok_or_else(|| {
            // SAFETY: dlerror returns a static, NUL-terminated message.
            let msg = unsafe { libc::dlerror() };
            let why = if msg.is_null() {
                "unknown error".into()
            } else {
                // SAFETY: as above.
                unsafe { CStr::from_ptr(msg) }
                    .to_string_lossy()
                    .into_owned()
            };
            HostError::DlopenFailed(format!("{}: {why}", path.display()))
        })
    }

    /// Resolve `name` as a `T` (a function pointer type).
    ///
    /// # Safety
    /// `T` must be the symbol's true type.
    unsafe fn symbol<T: Copy>(&self, name: &CStr) -> Option<T> {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
        // SAFETY: a libc call on the open library with a valid C string.
        let p = unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) };
        // SAFETY: pointer-sized (asserted) and the symbol's true type (this
        // function's contract).
        (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
    }

    /// # Safety
    /// As [`symbol`](Self::symbol).
    unsafe fn require<T: Copy>(&self, name: &'static CStr) -> Result<T, HostError> {
        // SAFETY: this function's contract.
        unsafe { self.symbol(name) }.ok_or(HostError::SymbolMissing(name.to_str().unwrap_or("?")))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened in `open`; nothing from it is referenced anymore.
        unsafe { libc::dlclose(self.0.as_ptr()) };
    }
}

// Every `unsafe` block in these tests plays a loader: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    #[test]
    fn dlopen_flags_are_now_and_local_never_noload() {
        assert_eq!(DLOPEN_FLAGS & libc::RTLD_NOW, libc::RTLD_NOW);
        assert_eq!(DLOPEN_FLAGS & libc::RTLD_LOCAL, libc::RTLD_LOCAL);
        assert_eq!(DLOPEN_FLAGS & libc::RTLD_NOLOAD, 0, "NOLOAD would refuse every fresh load");
        assert_eq!(DLOPEN_FLAGS & libc::RTLD_GLOBAL, 0);
    }

    #[test]
    fn missing_library_names_the_path() {
        let Err(HostError::DlopenFailed(msg)) = Library::open(Path::new("/nope/libghost.dylib"))
        else {
            panic!("expected DlopenFailed");
        };
        assert!(msg.contains("/nope/libghost.dylib"), "{msg}");
    }

    #[test]
    fn resolves_and_misses_symbols() {
        let lib = Library::open(Path::new("/usr/lib/libSystem.B.dylib")).unwrap();
        let strlen: Option<unsafe extern "C" fn(*const std::ffi::c_char) -> usize> =
            unsafe { lib.symbol(c"strlen") };
        assert_eq!(unsafe { strlen.unwrap()(c"four".as_ptr()) }, 4);
        let missing = unsafe { lib.require::<unsafe extern "C" fn()>(c"no_such_symbol_xyz") };
        assert!(matches!(
            missing,
            Err(HostError::SymbolMissing("no_such_symbol_xyz"))
        ));
    }

    #[test]
    fn refused_entry_is_an_error_and_never_exits() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static EXITED: AtomicBool = AtomicBool::new(false);
        unsafe extern "C" fn refuse(_: *mut c_void) -> bool {
            false
        }
        unsafe extern "C" fn exit() -> bool {
            EXITED.store(true, Ordering::SeqCst);
            true
        }
        unsafe extern "C" fn factory() -> *mut FUnknown {
            std::ptr::null_mut()
        }
        let entry = EntryPoints {
            bundle_entry: refuse,
            bundle_exit: exit,
            get_factory: factory,
            debug_bus_for: None,
        };
        assert!(matches!(
            Module::from_entry_points(entry),
            Err(HostError::BundleEntryFailed)
        ));
        assert!(!EXITED.load(Ordering::SeqCst));
    }
}
