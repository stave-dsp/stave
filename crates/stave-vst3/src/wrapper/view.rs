//! IPlugView: the Cocoa editor view behind `EditorBox`.
//!
//! Lifetime: `createView` returns a fresh `EditorBox` with one reference,
//! which itself holds one reference on the `Instance`; `sv_release` at
//! zero frees the box and then drops that instance reference. Every
//! method that touches the view or the instance runs inside the
//! instance's panic guard.

use super::*;

/// The view box behind a view interface pointer.
///
/// # Safety
/// `this` is a pointer `ec_create_view::<P>` returned whose count has not
/// reached zero.
#[inline]
unsafe fn view_box<'a, P: Vst3Export>(this: *mut FUnknown) -> &'a EditorBox<P> {
    // SAFETY: this function's contract; the box is a live `EditorBox<P>`.
    unsafe { &*this.cast::<EditorBox<P>>() }
}

/// The instance a view holds its reference on.
///
/// # Safety
/// As [`view_box`]: the view's reference keeps the instance alive.
#[inline]
unsafe fn view_instance<'a, P: Vst3Export>(this: *mut FUnknown) -> &'a Instance<P> {
    // SAFETY: this function's contract.
    unsafe { &*view_box::<P>(this).inst }
}

/// A new view on `inst` with one reference (the caller's), holding one
/// reference on `inst`.
pub(super) fn new_view<P: Vst3Export>(
    inst: &Instance<P>,
    inst_ptr: *mut Instance<P>,
) -> *mut FUnknown {
    let mut view = crate::cocoa_view::PluginEditorView::new(
        inst.debug_bus.clone(),
        inst.param_table.clone(),
        Box::new(Vst3EditorHost::<P> { inst: inst_ptr }),
    );
    view.set_ui(inst.editor_ui.clone());
    view.set_face(inst.editor_face.clone());
    view.set_face_presets(inst.programs.presets());
    // Taken only once the view exists, so a panic above leaks nothing.
    inst.ref_count.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(Box::new(EditorBox::<P> {
        // Raw, from the instance's own pointer (not `&inst`), like the
        // other by-value vtables: valid while the instance lives, which
        // the reference just taken guarantees.
        // SAFETY: `inst_ptr` is the live instance `inst` borrows;
        // `addr_of!` reads no memory.
        vtbl: unsafe { ptr::addr_of!((*inst_ptr).view_vtbl) },
        ref_count: AtomicU32::new(1),
        inst: inst_ptr,
        view,
    }))
    .cast()
}

pub(super) unsafe extern "C" fn sv_query_interface<P: Vst3Export>(
    this: *mut FUnknown,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: `iid` and `obj` are null or valid.
    let (iid, obj) = unsafe { (iid.as_ref(), out_param(obj, "obj")) };
    let (Some(&requested), Ok(obj)) = (iid, obj) else {
        return K_INVALID_ARGUMENT;
    };
    // Pointer comparisons and writes only: nothing here can panic.
    if requested == IID_FUNKNOWN || requested == IID_IPLUG_VIEW {
        *obj = this.cast();
        // SAFETY: `this` is a live view (host contract).
        unsafe { sv_add_ref::<P>(this) };
        K_RESULT_OK
    } else {
        *obj = ptr::null_mut();
        K_NO_INTERFACE
    }
}

pub(super) unsafe extern "C" fn sv_add_ref<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // SAFETY: host contract: `this` is a live view.
    let editor = unsafe { view_box::<P>(this) };
    editor
        .ref_count
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1)
}

pub(super) unsafe extern "C" fn sv_release<P: Vst3Export>(this: *mut FUnknown) -> u32 {
    // SAFETY: host contract: `this` is a live view; the caller's own
    // reference keeps it so until this decrement.
    let now = unsafe { view_box::<P>(this) }
        .ref_count
        .fetch_sub(1, Ordering::SeqCst)
        .wrapping_sub(1);
    if now == 0 {
        // SAFETY: the last reference is gone; `new_view` made `this` with
        // `Box::into_raw`, and no `&EditorBox` outlives this call.
        let editor = unsafe { Box::from_raw(this.cast::<EditorBox<P>>()) };
        let inst = editor.inst;
        // Dropping the view detaches it from any host window first; a
        // panicking destructor must not unwind into the host.
        ffi_entry::<P, _>(|| (), || drop(editor));
        // SAFETY: the view's instance reference, taken in `new_view`, is
        // still held (the instance is alive); `controller` is one of its
        // interfaces, and `shared_release` frees it if this was the last.
        unsafe { shared_release::<P>(ptr::addr_of_mut!((*inst).controller).cast()) };
    }
    now
}

pub(super) unsafe extern "C" fn sv_is_platform_type_supported(
    _this: *mut FUnknown,
    ty: FidString,
) -> Tresult {
    if ty.is_null() {
        return K_RESULT_FALSE;
    }
    // SAFETY: a non-null `FIDString` is nul-terminated (SDK). A `CStr`
    // comparison cannot panic.
    if unsafe { core::ffi::CStr::from_ptr(ty) } == c"NSView" {
        K_RESULT_OK
    } else {
        K_RESULT_FALSE
    }
}

pub(super) unsafe extern "C" fn sv_attached<P: Vst3Export>(
    this: *mut FUnknown,
    parent: *mut c_void,
    _ty: FidString,
) -> Tresult {
    // SAFETY: host contract: `this` is a live view.
    let inst = unsafe { view_instance::<P>(this) };
    inst.guarded(
        "IPlugView::attached",
        || K_INTERNAL_ERROR,
        || {
            // SAFETY: view calls are main-thread only (SDK) and nothing
            // else holds a reference into the box during one, so this
            // `&mut` is unique; the box is a different allocation from the
            // instance `inst` borrows.
            let editor = unsafe { &mut *this.cast::<EditorBox<P>>() };
            // SAFETY: `parent` is the host's NSView for this platform type.
            unsafe { editor.view.attach(parent) };
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn sv_removed<P: Vst3Export>(this: *mut FUnknown) -> Tresult {
    // SAFETY: host contract: `this` is a live view.
    let inst = unsafe { view_instance::<P>(this) };
    inst.guarded(
        "IPlugView::removed",
        || K_INTERNAL_ERROR,
        || {
            // SAFETY: as in `sv_attached`.
            let editor = unsafe { &mut *this.cast::<EditorBox<P>>() };
            // SAFETY: main thread; detaching an unattached view is a no-op.
            unsafe { editor.view.detach() };
            K_RESULT_OK
        },
    )
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

pub(super) unsafe extern "C" fn sv_get_size<P: Vst3Export>(
    this: *mut FUnknown,
    size: *mut ViewRect,
) -> Tresult {
    // SAFETY: host contract: `this` is a live view; `size` is null or
    // writable (SDK).
    let (editor, inst, size) = unsafe {
        (
            view_box::<P>(this),
            view_instance::<P>(this),
            out_param(size, "size"),
        )
    };
    inst.guarded(
        "IPlugView::getSize",
        || K_INTERNAL_ERROR,
        || {
            let size = match size {
                Ok(size) => size,
                Err(error) => return inst.answer("IPlugView::getSize", Err(error)),
            };
            // No editor tree: a bare container.
            let (w, h) = editor.view.ui_root_dims().unwrap_or((480.0, 320.0));
            *size = ViewRect {
                left: 0,
                top: 0,
                right: w as i32,
                bottom: h as i32,
            };
            K_RESULT_OK
        },
    )
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

pub(super) const fn build_view_vtbl<P: Vst3Export>() -> IPlugViewVtbl {
    IPlugViewVtbl {
        query_interface: sv_query_interface::<P>,
        add_ref: sv_add_ref::<P>,
        release: sv_release::<P>,
        is_platform_type_supported: sv_is_platform_type_supported,
        attached: sv_attached::<P>,
        removed: sv_removed::<P>,
        on_wheel: sv_on_wheel,
        on_key_down: sv_on_key_down,
        on_key_up: sv_on_key_up,
        get_size: sv_get_size::<P>,
        on_size: sv_on_size,
        on_focus: sv_on_focus,
        set_frame: sv_set_frame,
        can_resize: sv_can_resize,
        check_size_constraint: sv_check_size_constraint,
    }
}
