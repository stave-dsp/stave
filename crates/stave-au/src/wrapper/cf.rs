//! The CoreFoundation values the wrapper makes and reads: owned
//! references ([`Cf`]) released on drop, and the reads of borrowed
//! ones, each `unsafe` block's invariant stated once here.

use core::ffi::{c_char, c_void};
use core::ptr;

use crate::sys;

/// A CoreFoundation object this code owns one reference to.
pub(crate) struct Cf(sys::CFTypeRef);

impl Cf {
    /// # Safety
    /// `p` is null or a +1 reference the caller hands over.
    pub(crate) unsafe fn from_owned(p: sys::CFTypeRef) -> Option<Cf> {
        (!p.is_null()).then_some(Cf(p))
    }

    pub(crate) fn get(&self) -> sys::CFTypeRef {
        self.0
    }

    /// Hand the reference to the caller (a property's "caller releases").
    pub(crate) fn into_raw(self) -> sys::CFTypeRef {
        let p = self.0;
        core::mem::forget(self);
        p
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: the one reference `from_owned` was handed.
        unsafe { sys::CFRelease(self.0) }
    }
}

/// `s` as a CFString.
pub(crate) fn string(s: &str) -> Cf {
    // SAFETY: `s` is valid UTF-8 of the length passed; CF copies it.
    let p = unsafe {
        sys::CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as sys::CFIndex, sys::kCFStringEncodingUTF8, 0)
    };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes a string of valid UTF-8")
}

/// The text of a CFString, or `None` for null or another type.
///
/// # Safety
/// `s` is null or a live CoreFoundation object.
pub(crate) unsafe fn read_string(s: sys::CFStringRef) -> Option<String> {
    // SAFETY: the caller's contract; the type is checked before use and
    // the buffer is CF's own maximum for the encoding, plus the NUL.
    unsafe {
        if s.is_null() || sys::CFGetTypeID(s) != sys::CFStringGetTypeID() {
            return None;
        }
        let n = sys::CFStringGetMaximumSizeForEncoding(sys::CFStringGetLength(s), sys::kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0 as c_char; n.max(1) as usize];
        if sys::CFStringGetCString(s, buf.as_mut_ptr(), n, sys::kCFStringEncodingUTF8) == 0 {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

pub(crate) fn number(v: i32) -> Cf {
    // SAFETY: `v` is an SInt32, the type named.
    let p = unsafe { sys::CFNumberCreate(ptr::null(), sys::kCFNumberSInt32Type, (&v as *const i32).cast()) };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes a number")
}

/// A CFNumber's value as an SInt32.
///
/// # Safety
/// `n` is null or a live CoreFoundation object.
pub(crate) unsafe fn read_number(n: sys::CFNumberRef) -> Option<i32> {
    let mut v = 0i32;
    // SAFETY: the caller's contract; the type is checked first; `v` is
    // the SInt32 the call writes.
    unsafe {
        if n.is_null() || sys::CFGetTypeID(n) != sys::CFNumberGetTypeID() {
            return None;
        }
        sys::CFNumberGetValue(n, sys::kCFNumberSInt32Type, (&mut v as *mut i32).cast());
    }
    Some(v)
}

pub(crate) fn data(bytes: &[u8]) -> Cf {
    // SAFETY: `bytes` is readable for its length; CF copies it.
    let p = unsafe { sys::CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as sys::CFIndex) };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes a data object")
}

/// A CFData's bytes, copied.
///
/// # Safety
/// `d` is null or a live CoreFoundation object.
pub(crate) unsafe fn read_data(d: sys::CFDataRef) -> Option<Vec<u8>> {
    // SAFETY: the caller's contract; the type is checked first; the byte
    // pointer is valid for the reported length while `d` lives.
    unsafe {
        if d.is_null() || sys::CFGetTypeID(d) != sys::CFDataGetTypeID() {
            return None;
        }
        let n = sys::CFDataGetLength(d).max(0) as usize;
        let p = sys::CFDataGetBytePtr(d);
        Some(if n == 0 || p.is_null() { Vec::new() } else { std::slice::from_raw_parts(p, n).to_vec() })
    }
}

/// A new, empty mutable CFDictionary with CF-typed keys and values.
pub(crate) fn dictionary() -> Cf {
    // SAFETY: the standard callbacks, by address.
    let p = unsafe {
        sys::CFDictionaryCreateMutable(ptr::null(), 0, &sys::kCFTypeDictionaryKeyCallBacks, &sys::kCFTypeDictionaryValueCallBacks)
    };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes a dictionary")
}

/// `dict[key] = value` (the dictionary retains both).
pub(crate) fn insert(dict: &Cf, key: &str, value: &Cf) {
    let key = string(key);
    // SAFETY: `dict` was made by `dictionary` (mutable, CF-typed
    // callbacks); key and value are live CF objects.
    unsafe { sys::CFDictionarySetValue(dict.get().cast_mut(), key.get(), value.get()) }
}

/// `dict[key]`, borrowed (valid while `dict` is), or null.
///
/// # Safety
/// `dict` is null or a live CoreFoundation object.
pub(crate) unsafe fn lookup(dict: sys::CFDictionaryRef, key: &str) -> sys::CFTypeRef {
    // SAFETY: the caller's contract; the type is checked first.
    unsafe {
        if dict.is_null() || sys::CFGetTypeID(dict) != sys::CFDictionaryGetTypeID() {
            return ptr::null();
        }
        let key = string(key);
        sys::CFDictionaryGetValue(dict, key.get())
    }
}

/// A CFArray of CFStrings.
pub(crate) fn strings(items: &[&str]) -> Cf {
    let owned: Vec<Cf> = items.iter().map(|s| string(s)).collect();
    let values: Vec<*const c_void> = owned.iter().map(Cf::get).collect();
    // SAFETY: `values` holds live CF objects, retained by the array
    // (the standard callbacks) before `owned` releases its references.
    let p = unsafe { sys::CFArrayCreate(ptr::null(), values.as_ptr(), values.len() as sys::CFIndex, &sys::kCFTypeArrayCallBacks) };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes an array")
}

/// A CFArray of raw pointers the array neither retains nor releases
/// (`FactoryPresets`' `AUPreset`s, which outlive it).
pub(crate) fn pointers(items: &[*const c_void]) -> Cf {
    // SAFETY: `items` is readable for its length; null callbacks: the
    // array stores the pointers as they are.
    let p = unsafe { sys::CFArrayCreate(ptr::null(), items.as_ptr(), items.len() as sys::CFIndex, ptr::null()) };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }.expect("CoreFoundation makes an array")
}

/// A file URL for the directory at `path`.
pub(crate) fn directory_url(path: &[u8]) -> Option<Cf> {
    // SAFETY: `path` is readable for its length.
    let p = unsafe { sys::CFURLCreateFromFileSystemRepresentation(ptr::null(), path.as_ptr(), path.len() as sys::CFIndex, 1) };
    // SAFETY: a Create function's +1 result.
    unsafe { Cf::from_owned(p) }
}
