//! The Audio Unit ABI (`stave-au-sys`, the declarations the stave-au
//! plugin side uses too) and the few host-side helpers on it.

#![allow(non_snake_case, non_upper_case_globals)]

use core::ffi::c_char;

pub use stave_au_sys::*;

/// The most channels a hosted bus carries here.
pub const MAX_CHANNELS: usize = 8;

/// An `AudioBufferList` with room for [`MAX_CHANNELS`] buffers (the C
/// type is a one-element array the caller over-allocates; this is the
/// same layout with the room spelled out).
#[repr(C)]
pub struct BufferList {
    pub mNumberBuffers: u32,
    pub mBuffers: [AuBuffer; MAX_CHANNELS],
}

impl BufferList {
    pub fn as_raw(&mut self) -> *mut AudioBufferList {
        (self as *mut BufferList).cast()
    }
}

/// `s` as UTF-8 (empty for null).
///
/// # Safety
/// `s` is null or a live `CFString`.
pub unsafe fn string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: `s` is a live CFString (the caller's contract); the buffer
    // is sized by CF's own maximum for the encoding, plus the NUL.
    unsafe {
        let n = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0 as c_char; n.max(1) as usize];
        if CFStringGetCString(s, buf.as_mut_ptr(), n, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

/// `s` as a CFString the caller releases.
pub fn cf_string(s: &str) -> CFStringRef {
    // SAFETY: `s` is valid UTF-8 of the length passed; CF copies it.
    unsafe { CFStringCreateWithBytes(core::ptr::null(), s.as_ptr(), s.len() as CFIndex, kCFStringEncodingUTF8, 0) }
}

/// A property list as bytes (binary plist), or `None`.
///
/// # Safety
/// `list` is null or a live CoreFoundation property list.
pub unsafe fn plist_bytes(list: CFPropertyListRef) -> Option<Vec<u8>> {
    if list.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; the data is ours to release, its
    // bytes valid for its length while it lives.
    unsafe {
        let data = CFPropertyListCreateData(core::ptr::null(), list, kCFPropertyListBinaryFormat_v1_0, 0, core::ptr::null_mut());
        if data.is_null() {
            return None;
        }
        let n = CFDataGetLength(data).max(0) as usize;
        let p = CFDataGetBytePtr(data);
        let out = if n == 0 || p.is_null() { Vec::new() } else { std::slice::from_raw_parts(p, n).to_vec() };
        CFRelease(data);
        Some(out)
    }
}

/// Bytes [`plist_bytes`] wrote, as a property list the caller releases.
pub fn plist_from(bytes: &[u8]) -> CFPropertyListRef {
    // SAFETY: `bytes` is readable for its length; CF copies it, and the
    // data object is released once the list is made.
    unsafe {
        let data = CFDataCreate(core::ptr::null(), bytes.as_ptr(), bytes.len() as CFIndex);
        if data.is_null() {
            return core::ptr::null();
        }
        let list = CFPropertyListCreateWithData(core::ptr::null(), data, 0, core::ptr::null_mut(), core::ptr::null_mut());
        CFRelease(data);
        list
    }
}

/// A unit's label for an AudioUnit parameter unit (`custom` for
/// `kAudioUnitParameterUnit_CustomUnit`).
pub fn unit_label(unit: u32, custom: &str) -> String {
    match unit {
        3 => "%",
        4 => "s",
        5 => "samples",
        6 | 15 => "°",
        8 => "Hz",
        9 | 20 => "cents",
        10 => "semitones",
        13 => "dB",
        21 => "octaves",
        22 => "BPM",
        23 => "beats",
        24 => "ms",
        25 => "ratio",
        u if u == kAudioUnitParameterUnit_CustomUnit => return custom.to_string(),
        _ => "",
    }
    .to_string()
}
