//! A plugin MODULE: the entry points a VST3 binary exports, and the
//! lifetime around them (`bundleEntry` … `bundleExit` on macOS,
//! `ModuleEntry` … `ModuleExit` on Linux, `InitDll` … `ExitDll` on
//! Windows; load … unload). Usually loaded from a bundle; tests and embedders can
//! hand in statically linked entry points instead.

use std::ffi::{c_void, CStr};
use std::path::Path;
use std::ptr::NonNull;

use stave_telemetry::Telemetry;
use stave_vst3_sys::FUnknown;

use super::bundle::Bundle;
use super::HostError;

/// The exports of a VST3 binary (plus stave's optional telemetry hook).
#[derive(Copy, Clone)]
pub struct EntryPoints {
    pub bundle_entry: unsafe extern "C" fn(*mut c_void) -> bool,
    pub bundle_exit: unsafe extern "C" fn() -> bool,
    pub get_factory: unsafe extern "C" fn() -> *mut FUnknown,
    /// `stave_telemetry_for` — present only in stave-built plugins.
    pub telemetry_for: Option<unsafe extern "C" fn(*mut c_void) -> *const Telemetry>,
}

/// The [`EntryPoints`] of a stave plugin crate linked into this binary
/// (the symbols `stave::export!` defines in it).
#[macro_export]
macro_rules! entry_points {
    ($plugin:path) => {{
        use $plugin as plugin;
        $crate::plugin::EntryPoints {
            bundle_entry: plugin::bundleEntry,
            bundle_exit: plugin::bundleExit,
            get_factory: plugin::GetPluginFactory,
            telemetry_for: Some(plugin::stave_telemetry_for),
        }
    }};
}

/// How a module is entered: `bundleEntry(CFBundleRef)` (macOS),
/// `ModuleEntry(dlopen handle)` (Linux), `InitDll()` (Windows, optional).
#[derive(Copy, Clone)]
#[cfg_attr(not(windows), allow(dead_code))]
enum Enter {
    With(unsafe extern "C" fn(*mut c_void) -> bool, *mut c_void),
    Plain(unsafe extern "C" fn() -> bool),
    Nothing,
}

/// An entered module. Dropping it calls the module's exit
/// (`bundleExit`, `ModuleExit` or `ExitDll`), then unloads the library
/// (and releases the bundle) — in that order.
pub(crate) struct Module {
    entry: EntryPoints,
    exit: unsafe extern "C" fn() -> bool,
    _library: Option<Library>,
    _bundle: Option<Bundle>,
}

/// Stands in for the bundle entry and exit a Windows module does not
/// export (it is entered through [`Enter`]; these are never called).
#[cfg(windows)]
unsafe extern "C" fn no_entry(_: *mut c_void) -> bool {
    true
}
#[cfg(windows)]
unsafe extern "C" fn no_exit() -> bool {
    true
}

impl Module {
    /// Load the bundle's binary, resolve its exports, and enter it.
    pub fn load(path: &Path) -> Result<Self, HostError> {
        let bundle = Bundle::open(path);
        let library = Library::open(&bundle.executable())?;
        // SAFETY: the exported symbols have the VST3 SDK's signatures.
        let (entry, enter, exit) = unsafe {
            let get_factory = library.require(c"GetPluginFactory")?;
            let telemetry_for = library.symbol(c"stave_telemetry_for");
            #[cfg(target_os = "macos")]
            let (bundle_entry, bundle_exit, enter) = {
                let e = library.require(c"bundleEntry")?;
                (e, library.require(c"bundleExit")?, Enter::With(e, bundle.cf_ref()))
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            let (bundle_entry, bundle_exit, enter) = {
                let e = library.require(c"ModuleEntry")?;
                (e, library.require(c"ModuleExit")?, Enter::With(e, library.handle()))
            };
            #[cfg(windows)]
            let (bundle_entry, bundle_exit, enter) = {
                // InitDll/ExitDll are optional on Windows (SDK).
                let init: Option<unsafe extern "C" fn() -> bool> = library.symbol(c"InitDll");
                let exit: Option<unsafe extern "C" fn() -> bool> = library.symbol(c"ExitDll");
                let enter = init.map_or(Enter::Nothing, Enter::Plain);
                (no_entry as unsafe extern "C" fn(*mut c_void) -> bool, exit.unwrap_or(no_exit), enter)
            };
            let entry = EntryPoints {
                bundle_entry,
                bundle_exit,
                get_factory,
                telemetry_for,
            };
            (entry, enter, bundle_exit)
        };
        Self::enter(entry, enter, exit, Some(library), Some(bundle))
    }

    /// Enter statically linked entry points (no bundle, no library).
    pub fn from_entry_points(entry: EntryPoints) -> Result<Self, HostError> {
        let enter = Enter::With(entry.bundle_entry, std::ptr::null_mut());
        Self::enter(entry, enter, entry.bundle_exit, None, None)
    }

    fn enter(
        entry: EntryPoints,
        enter: Enter,
        exit: unsafe extern "C" fn() -> bool,
        library: Option<Library>,
        bundle: Option<Bundle>,
    ) -> Result<Self, HostError> {
        // SAFETY: called once, before any other export (spec order), with
        // the argument its platform's entry takes.
        let entered = unsafe {
            match enter {
                Enter::With(f, arg) => f(arg),
                Enter::Plain(f) => f(),
                Enter::Nothing => true,
            }
        };
        if !entered {
            return Err(HostError::BundleEntryFailed);
        }
        Ok(Self {
            entry,
            exit,
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
        unsafe { (self.exit)() };
    }
}

/// A loaded shared library (dlopen / LoadLibraryW); unloaded on drop.
struct Library(NonNull<c_void>);

/// Resolve everything up front, and keep each plugin's symbols to
/// itself: macOS defaults to GLOBAL, and every VST3 exports the same
/// names. The values are platform-specific (Linux `0x4` is
/// `RTLD_NOLOAD`, not LOCAL), so they come from libc, never literals.
#[cfg(unix)]
const DLOPEN_FLAGS: libc::c_int = libc::RTLD_NOW | libc::RTLD_LOCAL;

#[cfg(unix)]
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

    /// The handle `ModuleEntry` receives (Linux).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    fn handle(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    fn lookup(&self, name: &CStr) -> *mut c_void {
        // SAFETY: a libc call on the open library with a valid C string.
        unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) }
    }
}

#[cfg(unix)]
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened in `open`; nothing from it is referenced anymore.
        unsafe { libc::dlclose(self.0.as_ptr()) };
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const core::ffi::c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

#[cfg(windows)]
impl Library {
    fn open(path: &Path) -> Result<Self, HostError> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        if wide[..wide.len() - 1].contains(&0) {
            return Err(HostError::DlopenFailed(format!("path has a NUL: {}", path.display())));
        }
        // SAFETY: a nul-terminated UTF-16 path.
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        NonNull::new(handle).map(Self).ok_or_else(|| {
            // SAFETY: no arguments; reads this thread's last error.
            let code = unsafe { GetLastError() };
            HostError::DlopenFailed(format!("{}: LoadLibraryW error {code}", path.display()))
        })
    }

    fn lookup(&self, name: &CStr) -> *mut c_void {
        // SAFETY: the open module, a nul-terminated name.
        unsafe { GetProcAddress(self.0.as_ptr(), name.as_ptr()) }
    }
}

#[cfg(windows)]
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: loaded in `open`; nothing from it is referenced anymore.
        unsafe { FreeLibrary(self.0.as_ptr()) };
    }
}

impl Library {
    /// Resolve `name` as a `T` (a function pointer type).
    ///
    /// # Safety
    /// `T` must be the symbol's true type.
    unsafe fn symbol<T: Copy>(&self, name: &CStr) -> Option<T> {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
        let p = self.lookup(name);
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

// Every `unsafe` block in these tests plays a loader: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    #[cfg(unix)]
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
        #[cfg(target_os = "macos")]
        let (lib, name) = ("/usr/lib/libSystem.B.dylib", c"strlen");
        #[cfg(all(unix, not(target_os = "macos")))]
        let (lib, name) = ("libc.so.6", c"strlen");
        #[cfg(windows)]
        let (lib, name) = ("ucrtbase.dll", c"strlen");
        let lib = Library::open(Path::new(lib)).unwrap();
        let strlen: Option<unsafe extern "C" fn(*const std::ffi::c_char) -> usize> =
            unsafe { lib.symbol(name) };
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
            telemetry_for: None,
        };
        assert!(matches!(
            Module::from_entry_points(entry),
            Err(HostError::BundleEntryFailed)
        ));
        assert!(!EXITED.load(Ordering::SeqCst));
    }
}
