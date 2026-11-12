//! COM plumbing for a VST3 HOST: an owning smart pointer for the
//! plugin's objects ([`Com`]) and the interfaces a host implements
//! and hands to the plugin (context, streams, parameter queues,
//! events, connection points).

pub(crate) mod connection;
pub(crate) mod event_list;
pub(crate) mod host_app;
pub(crate) mod param_changes;
pub(crate) mod stream;

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use stave_vst3::sys::{FUnknown, Tresult, Tuid, K_RESULT_OK};

/// The `FUnknown` prefix every VST3 vtable starts with.
#[repr(C)]
pub(crate) struct UnknownVtbl {
    pub query_interface:
        unsafe extern "C" fn(*mut FUnknown, *const Tuid, *mut *mut c_void) -> Tresult,
    pub add_ref: unsafe extern "C" fn(*mut FUnknown) -> u32,
    pub release: unsafe extern "C" fn(*mut FUnknown) -> u32,
}

/// The `IPluginBase` prefix shared by `IComponent` and `IEditController`.
#[repr(C)]
pub(crate) struct PluginBaseVtbl {
    pub unknown: UnknownVtbl,
    pub initialize: unsafe extern "C" fn(*mut FUnknown, *mut FUnknown) -> Tresult,
    pub terminate: unsafe extern "C" fn(*mut FUnknown) -> Tresult,
}

/// One owned reference to a plugin COM object, viewed through vtable
/// type `V`. Dropping it releases the reference — every early return
/// in the load sequence cleans up by construction.
pub(crate) struct Com<V> {
    ptr: NonNull<FUnknown>,
    _vtbl: PhantomData<*const V>,
}

impl<V> Com<V> {
    /// Adopt one reference to `ptr` (null → `None`).
    ///
    /// # Safety
    /// `ptr` is a live COM object whose vtable starts with `V`'s layout,
    /// and the caller transfers one reference to the returned value.
    pub unsafe fn adopt(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr.cast()).map(|ptr| Self {
            ptr,
            _vtbl: PhantomData,
        })
    }

    pub fn as_ptr(&self) -> *mut FUnknown {
        self.ptr.as_ptr()
    }

    /// The object's vtable, typed as `V`.
    pub fn vtbl(&self) -> &V {
        // SAFETY: `adopt`'s contract — the object starts with a pointer
        // to a vtable laid out as `V`, alive as long as the object.
        unsafe { &**(self.ptr.as_ptr() as *const *const V) }
    }

    fn unknown(&self) -> &UnknownVtbl {
        // SAFETY: every COM vtable starts with the FUnknown prefix.
        unsafe { &**(self.ptr.as_ptr() as *const *const UnknownVtbl) }
    }

    /// `queryInterface` for `iid`, viewed as `W`. `None` when the
    /// object doesn't implement it.
    ///
    /// # Safety
    /// `W` must be the vtable layout of the interface `iid` names.
    pub unsafe fn query<W>(&self, iid: &Tuid) -> Option<Com<W>> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: the object's own method, called on the live object.
        let r = unsafe { (self.unknown().query_interface)(self.as_ptr(), iid, &mut out) };
        if r == K_RESULT_OK {
            // SAFETY: a successful `queryInterface` hands over one
            // reference to an object implementing `iid`, whose layout
            // is `W` (this function's contract).
            unsafe { Com::adopt(out) }
        } else {
            None
        }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: we own exactly one reference (adopt's contract).
        unsafe { (self.unknown().release)(self.as_ptr()) };
    }
}

/// An `IPluginBase` object that was `initialize`d: dropping it calls
/// `terminate` before releasing the reference.
pub(crate) struct Initialized<V> {
    com: Com<V>,
}

impl<V> Initialized<V> {
    /// `initialize(context)`; on failure the reference is released and
    /// the tresult returned.
    ///
    /// # Safety
    /// `V`'s layout starts with [`PluginBaseVtbl`].
    pub unsafe fn new(com: Com<V>, context: *mut FUnknown) -> Result<Self, Tresult> {
        // SAFETY: this function's contract.
        let base = unsafe { &*(com.vtbl() as *const V as *const PluginBaseVtbl) };
        // SAFETY: the object's own method, called on the live object.
        match unsafe { (base.initialize)(com.as_ptr(), context) } {
            K_RESULT_OK => Ok(Self { com }),
            r => Err(r),
        }
    }

    pub fn com(&self) -> &Com<V> {
        &self.com
    }
}

impl<V> std::ops::Deref for Initialized<V> {
    type Target = Com<V>;
    fn deref(&self) -> &Com<V> {
        &self.com
    }
}

impl<V> Drop for Initialized<V> {
    fn drop(&mut self) {
        // SAFETY: `new`'s contract — the vtable starts with IPluginBase.
        unsafe {
            let base = &*(self.com.vtbl() as *const V as *const PluginBaseVtbl);
            (base.terminate)(self.com.as_ptr());
        }
    }
}

/// UTF-16, NUL-terminated (a VST3 `String128`) → `String`.
pub(crate) fn utf16_z(units: &[u16]) -> String {
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// C `char`, NUL-terminated → `String`.
pub(crate) fn c_str_z(chars: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_stops_at_nul() {
        let mut s = [0u16; 8];
        for (i, c) in "gain".encode_utf16().enumerate() {
            s[i] = c;
        }
        assert_eq!(utf16_z(&s), "gain");
        assert_eq!(utf16_z(&[0x61, 0x62]), "ab", "no terminator: whole slice");
    }

    #[test]
    fn c_str_stops_at_nul() {
        let s: Vec<std::ffi::c_char> = b"Audio Module Class\0junk"
            .iter()
            .map(|&b| b as std::ffi::c_char)
            .collect();
        assert_eq!(c_str_z(&s), "Audio Module Class");
    }
}
