//! IPlugView: the Cocoa editor view behind `EditorBox`.

use super::*;

pub(super) unsafe extern "C" fn sv_query_interface(
    this: *mut FUnknown,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: `iid` and `obj` are null or valid.
    let (iid, obj) = unsafe { (iid.as_ref(), out_param(obj, "obj")) };
    let (Some(&requested), Ok(obj)) = (iid, obj) else {
        return K_INVALID_ARGUMENT;
    };
    if requested == IID_FUNKNOWN || requested == IID_IPLUG_VIEW {
        *obj = this.cast();
        K_RESULT_OK
    } else {
        *obj = ptr::null_mut();
        K_NO_INTERFACE
    }
}
pub(super) unsafe extern "C" fn sv_add_ref(_this: *mut FUnknown) -> u32 {
    1
}
pub(super) unsafe extern "C" fn sv_release(_this: *mut FUnknown) -> u32 {
    1
}
pub(super) unsafe extern "C" fn sv_is_platform_type_supported(
    _this: *mut FUnknown,
    ty: FidString,
) -> Tresult {
    if ty.is_null() {
        return K_RESULT_FALSE;
    }
    // SAFETY: a non-null `FIDString` is nul-terminated (SDK).
    if unsafe { core::ffi::CStr::from_ptr(ty) } == c"NSView" {
        K_RESULT_OK
    } else {
        K_RESULT_FALSE
    }
}
pub(super) unsafe extern "C" fn sv_attached(
    this: *mut FUnknown,
    parent: *mut c_void,
    _ty: FidString,
) -> Tresult {
    // SAFETY: `this` is the `EditorBox` `createView` returned; view calls
    // are main-thread only (SDK), so this `&mut` is the only one.
    let editor = unsafe { &mut *(this as *mut EditorBox) };
    // SAFETY: `parent` is the host's NSView for this platform type.
    unsafe { editor.view.attach(parent) };
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_removed(this: *mut FUnknown) -> Tresult {
    // SAFETY: as in `sv_attached`.
    let editor = unsafe { &mut *(this as *mut EditorBox) };
    // SAFETY: the view was attached by `sv_attached`.
    unsafe { editor.view.detach() };
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_on_wheel(_this: *mut FUnknown, _distance: f32) -> Tresult {
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_on_key_down(
    _this: *mut FUnknown,
    _key: i16,
    _key_code: i16,
    _modifiers: i16,
) -> Tresult {
    K_RESULT_FALSE
}
pub(super) unsafe extern "C" fn sv_on_key_up(
    _this: *mut FUnknown,
    _key: i16,
    _key_code: i16,
    _modifiers: i16,
) -> Tresult {
    K_RESULT_FALSE
}
pub(super) unsafe extern "C" fn sv_get_size(this: *mut FUnknown, size: *mut ViewRect) -> Tresult {
    // SAFETY: SDK: null or writable.
    let Ok(size) = (unsafe { out_param(size, "size") }) else {
        return K_INVALID_ARGUMENT;
    };
    // SAFETY: as in `sv_attached`.
    let editor = unsafe { &*(this as *const EditorBox) };
    let rect = if let Some((w, h)) = editor.view.ui_root_dims() {
        ViewRect {
            left: 0,
            top: 0,
            right: w as i32,
            bottom: h as i32,
        }
    } else {
        // No editor tree: a bare container.
        ViewRect {
            left: 0,
            top: 0,
            right: 480,
            bottom: 320,
        }
    };
    *size = rect;
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_on_size(
    _this: *mut FUnknown,
    _new_size: *mut ViewRect,
) -> Tresult {
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_on_focus(_this: *mut FUnknown, _state: TBool) -> Tresult {
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_set_frame(
    _this: *mut FUnknown,
    _frame: *mut FUnknown,
) -> Tresult {
    K_RESULT_OK
}
pub(super) unsafe extern "C" fn sv_can_resize(_this: *mut FUnknown) -> Tresult {
    K_RESULT_FALSE
}
pub(super) unsafe extern "C" fn sv_check_size_constraint(
    _this: *mut FUnknown,
    _rect: *mut ViewRect,
) -> Tresult {
    K_RESULT_OK
}

pub(super) static STUB_VIEW_VTBL: IPlugViewVtbl = IPlugViewVtbl {
    query_interface: sv_query_interface,
    add_ref: sv_add_ref,
    release: sv_release,
    is_platform_type_supported: sv_is_platform_type_supported,
    attached: sv_attached,
    removed: sv_removed,
    on_wheel: sv_on_wheel,
    on_key_down: sv_on_key_down,
    on_key_up: sv_on_key_up,
    get_size: sv_get_size,
    on_size: sv_on_size,
    on_focus: sv_on_focus,
    set_frame: sv_set_frame,
    can_resize: sv_can_resize,
    check_size_constraint: sv_check_size_constraint,
};
