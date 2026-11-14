//! A `.component` bundle opened as itself: its `Info.plist`'s first
//! `AudioComponents` entry names the codes and the factory function; the
//! bundle's binary is loaded and the factory registered in this process
//! (`AudioComponentRegister`), so the host opens exactly the bundle it
//! was given, installed or not.

use std::path::Path;

use super::ffi::*;
use super::{AuError, Component};

fn fail(path: &Path, reason: impl Into<String>) -> AuError {
    AuError::Bundle {
        path: path.display().to_string(),
        reason: reason.into(),
    }
}

/// `dict[key]`, borrowed from `dict`.
///
/// # Safety
/// `dict` is null or a live CFDictionary.
unsafe fn get(dict: CFTypeRef, key: &str) -> CFTypeRef {
    if dict.is_null() {
        return core::ptr::null();
    }
    let k = cf_string(key);
    // SAFETY: the caller's contract; `k` is ours to release.
    unsafe {
        let v = CFDictionaryGetValue(dict, k);
        CFRelease(k);
        v
    }
}

/// `dict[key]` as text, or empty.
///
/// # Safety
/// As [`get`].
unsafe fn text(dict: CFTypeRef, key: &str) -> String {
    // SAFETY: as `get`; a value of another type reads as empty.
    unsafe {
        let v = get(dict, key);
        if v.is_null() || CFGetTypeID(v) != CFStringGetTypeID() {
            return String::new();
        }
        string(v)
    }
}

fn four(path: &Path, field: &str, s: &str) -> Result<u32, AuError> {
    let b = s.as_bytes();
    if b.len() != 4 {
        return Err(fail(path, format!("AudioComponents {field} '{s}' is not four characters")));
    }
    Ok(fourcc(&[b[0], b[1], b[2], b[3]]))
}

/// The component the bundle at `path` describes, registered once per
/// process: registering the same codes again would replace the first
/// registration and invalidate its handle.
pub(super) fn register(path: &Path) -> Result<Component, AuError> {
    use std::collections::HashMap;
    use std::sync::{Mutex, PoisonError};
    static REGISTERED: Mutex<Option<HashMap<std::path::PathBuf, Component>>> = Mutex::new(None);
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut registered = REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(c) = registered.get_or_insert_with(HashMap::new).get(&key) {
        return Ok(c.clone());
    }
    let c = register_new(path)?;
    registered.get_or_insert_with(HashMap::new).insert(key, c.clone());
    Ok(c)
}

fn register_new(path: &Path) -> Result<Component, AuError> {
    let plist = std::fs::read(path.join("Contents/Info.plist")).map_err(|e| fail(path, format!("Info.plist: {e}")))?;
    let list = plist_from(&plist);
    if list.is_null() {
        return Err(fail(path, "Info.plist is not a property list"));
    }
    // SAFETY: `list` is the live dictionary just parsed (released below);
    // every value read is type-checked.
    let (fields, version) = unsafe {
        let components = get(list, "AudioComponents");
        let first = if !components.is_null() && CFArrayGetCount(components) > 0 {
            CFArrayGetValueAtIndex(components, 0)
        } else {
            core::ptr::null()
        };
        let version = get(first, "version");
        let mut v = 0i32;
        if !version.is_null() && CFGetTypeID(version) == CFNumberGetTypeID() {
            CFNumberGetValue(version, kCFNumberSInt32Type, (&mut v as *mut i32).cast());
        }
        let fields = [
            text(first, "type"),
            text(first, "subtype"),
            text(first, "manufacturer"),
            text(first, "name"),
            text(first, "factoryFunction"),
            text(list, "CFBundleExecutable"),
        ];
        CFRelease(list);
        (fields, v as u32)
    };
    let [kind, subtype, maker, name, factory, binary] = fields;
    if factory.is_empty() || binary.is_empty() {
        return Err(fail(path, "no AudioComponents entry with a factoryFunction"));
    }
    let desc = AudioComponentDescription {
        componentType: four(path, "type", &kind)?,
        componentSubType: four(path, "subtype", &subtype)?,
        componentManufacturer: four(path, "manufacturer", &maker)?,
        componentFlags: 0,
        componentFlagsMask: 0,
    };
    let exe = path.join("Contents/MacOS").join(&binary);
    let exe = std::ffi::CString::new(exe.as_os_str().as_encoded_bytes()).map_err(|_| fail(path, "binary path holds a NUL"))?;
    let symbol = std::ffi::CString::new(factory.as_str()).map_err(|_| fail(path, "factory name holds a NUL"))?;
    // SAFETY: loading the bundle's own binary (kept loaded for the
    // process: the registered factory lives in it) and looking up the
    // factory its Info.plist names; `dlerror` after a failed `dlopen` is
    // a C string the loader owns.
    let f = unsafe {
        let handle = libc::dlopen(exe.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let why = std::ffi::CStr::from_ptr(libc::dlerror()).to_string_lossy().into_owned();
            return Err(fail(path, format!("dlopen: {why}")));
        }
        libc::dlsym(handle, symbol.as_ptr())
    };
    if f.is_null() {
        return Err(fail(path, format!("no factory function {factory}")));
    }
    // SAFETY: the symbol the bundle declares as its AudioComponent
    // factory, which has that C signature.
    let factory = unsafe { core::mem::transmute::<*mut core::ffi::c_void, AudioComponentFactoryFunction>(f) };
    Ok(Component::register(desc, &name, version, factory))
}
