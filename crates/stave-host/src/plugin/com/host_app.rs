//! `IHostApplication` — the host context handed to `initialize`.
//! Commercial plugins expect one; some controllers register zero
//! parameters without it.

use std::ffi::c_void;
use std::ptr;

use stave_vst3::sys::{
    FUnknown, Tresult, Tuid, IID_FUNKNOWN, K_INVALID_ARGUMENT, K_NO_INTERFACE, K_RESULT_FALSE,
    K_RESULT_OK,
};

pub(crate) const IID_IHOST_APPLICATION: Tuid =
    stave_vst3::sys::make_tuid(0x58E595CC, 0xDB2D4969, 0x8B6AAF8C, 0x36A664E5);

/// The name plugins see for their host.
pub(crate) const HOST_NAME: &str = "stave-host";

#[repr(C)]
struct HostApp {
    vtbl: *const HostAppVtbl,
}

#[repr(C)]
struct HostAppVtbl {
    query_interface: unsafe extern "C" fn(*mut FUnknown, *const Tuid, *mut *mut c_void) -> Tresult,
    add_ref: unsafe extern "C" fn(*mut FUnknown) -> u32,
    release: unsafe extern "C" fn(*mut FUnknown) -> u32,
    get_name: unsafe extern "C" fn(*mut FUnknown, *mut u16) -> Tresult,
    create_instance:
        unsafe extern "C" fn(*mut FUnknown, *mut Tuid, *mut Tuid, *mut *mut c_void) -> Tresult,
}

static VTBL: HostAppVtbl = HostAppVtbl {
    query_interface,
    add_ref,
    release,
    get_name,
    create_instance,
};

static HOST_APP: HostApp = HostApp { vtbl: &VTBL };
// SAFETY: immutable static; its callbacks touch no shared state.
unsafe impl Sync for HostApp {}

/// The process-wide host context (static: refcounting is a no-op).
pub(crate) fn host_context() -> *mut FUnknown {
    &HOST_APP as *const HostApp as *mut FUnknown
}

unsafe extern "C" fn add_ref(_this: *mut FUnknown) -> u32 {
    1
}
unsafe extern "C" fn release(_this: *mut FUnknown) -> u32 {
    1
}

unsafe extern "C" fn query_interface(
    this: *mut FUnknown,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: null or valid.
    let (Some(&iid), Some(obj)) = (unsafe { iid.as_ref() }, unsafe { obj.as_mut() }) else {
        return K_INVALID_ARGUMENT;
    };
    let hit = iid == IID_IHOST_APPLICATION || iid == IID_FUNKNOWN;
    *obj = if hit { this.cast() } else { ptr::null_mut() };
    if hit {
        K_RESULT_OK
    } else {
        K_NO_INTERFACE
    }
}

/// Writes into a `String128` (128 UTF-16 units including the NUL).
unsafe extern "C" fn get_name(_this: *mut FUnknown, name: *mut u16) -> Tresult {
    if name.is_null() {
        return K_INVALID_ARGUMENT;
    }
    let units: Vec<u16> = HOST_NAME.encode_utf16().take(127).chain([0]).collect();
    // SAFETY: SDK: a `String128`, writable for 128 ≥ `units.len()` units.
    unsafe { ptr::copy_nonoverlapping(units.as_ptr(), name, units.len()) };
    K_RESULT_OK
}

/// We create no host objects (IMessage/IAttributeList) for plugins.
unsafe extern "C" fn create_instance(
    _this: *mut FUnknown,
    _cid: *mut Tuid,
    _iid: *mut Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: null or writable.
    if let Some(obj) = unsafe { obj.as_mut() } {
        *obj = ptr::null_mut();
    }
    K_RESULT_FALSE
}

// Every `unsafe` block in these tests plays the plugin: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    #[test]
    fn answers_its_name_and_interfaces() {
        let ctx = host_context();
        let vt = unsafe { &**(ctx as *const *const HostAppVtbl) };
        let mut buf = [0xFFFFu16; 128];
        assert_eq!(unsafe { (vt.get_name)(ctx, buf.as_mut_ptr()) }, K_RESULT_OK);
        assert_eq!(crate::plugin::com::utf16_z(&buf), HOST_NAME);

        let mut obj = ptr::null_mut();
        let r = unsafe { (vt.query_interface)(ctx, &IID_IHOST_APPLICATION, &mut obj) };
        assert_eq!((r, obj), (K_RESULT_OK, ctx.cast()));
        let r = unsafe { (vt.query_interface)(ctx, &[3; 16], &mut obj) };
        assert_eq!((r, obj), (K_NO_INTERFACE, ptr::null_mut()));
    }
}
