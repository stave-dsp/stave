//! IPlugView: the plugin's face, in the platform's `FaceView`, behind
//! `EditorBox` — an NSView on macOS (`NSView`), a child HWND on Windows
//! (`HWND`), an X11 window on Linux (`X11EmbedWindowID`). On Linux the
//! view joins the host's run loop: `attached` asks the frame
//! (`setFrame`) for its `IRunLoop` and registers the display
//! connection's descriptor and a repaint timer, each a small COM object
//! inside the box that pumps the view; `removed` unregisters them.
//!
//! Lifetime: `createView` returns a fresh `EditorBox` with one reference,
//! which itself holds one reference on the `Instance`; `sv_release` at
//! zero frees the box and then drops that instance reference. Every
//! method that touches the view or the instance runs inside the
//! instance's panic guard.

use core::cell::Cell;

use stave_core::PluginController;
#[cfg(target_os = "macos")]
use stave_ui_cocoa::FaceView;
#[cfg(windows)]
use stave_ui_win32::FaceView;
#[cfg(all(unix, not(target_os = "macos")))]
use stave_ui_x11::FaceView;

use super::*;
use crate::gestures::Vst3Gestures;

/// One `IPlugView`, returned by `createView` with one reference (the
/// SDK's `CPluginView` pattern). While alive it holds a reference on its
/// `Instance`, so a host that releases the controller first still
/// reaches a live instance through the view; the last `release` drops
/// the view (which detaches it from any host window) and then that
/// instance reference.
///
/// No `Send`/`Sync`: the box lives on the heap behind the host's raw
/// pointer, never inside a value that crosses threads, and view calls
/// are main-thread only (SDK).
#[repr(C)]
pub struct EditorBox<P: Vst3Export> {
    /// Points at the owning instance's `view_vtbl`.
    pub vtbl: *const IPlugViewVtbl,
    pub(crate) ref_count: AtomicU32,
    pub(crate) inst: *mut Instance<P>,
    pub view: FaceView,
    /// The host's `IPlugFrame` (`setFrame`), unowned: valid until the
    /// next `setFrame` (SDK).
    frame: Cell<*mut FUnknown>,
    #[cfg(all(unix, not(target_os = "macos")))]
    run_loop: run_loop::Link,
}

/// The platform type a view attaches to (`isPlatformTypeSupported`).
#[cfg(target_os = "macos")]
const PLATFORM: &core::ffi::CStr = c"NSView";
#[cfg(windows)]
const PLATFORM: &core::ffi::CStr = c"HWND";
#[cfg(all(unix, not(target_os = "macos")))]
const PLATFORM: &core::ffi::CStr = c"X11EmbedWindowID";

/// The view's size in the host's unit: points on macOS, device pixels
/// on Windows and Linux.
#[cfg(target_os = "macos")]
fn host_size(view: &FaceView) -> (i32, i32) {
    let (w, h) = view.size();
    (w as i32, h as i32)
}
#[cfg(not(target_os = "macos"))]
fn host_size(view: &FaceView) -> (i32, i32) {
    let (w, h) = view.host_size();
    (w as i32, h as i32)
}


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
/// reference on `inst`; null for a plugin without a face (the host shows
/// its own editor).
pub(super) fn new_view<P: Vst3Export>(
    inst: &Instance<P>,
    inst_ptr: *mut Instance<P>,
) -> *mut FUnknown {
    let Some(face) = inst.face.clone() else {
        return ptr::null_mut();
    };
    let gestures = Vst3Gestures {
        handler: inst.handler.clone(),
        table: inst.params.clone(),
    };
    let controller = PluginController::new(
        inst.params.clone(),
        inst.telemetry.clone(),
        inst.programs.presets().clone(),
        Box::new(gestures),
    );
    let view = FaceView::new(face, Box::new(controller));
    // Taken only once the view exists, so a panic above leaks nothing.
    inst.ref_count.fetch_add(1, Ordering::SeqCst);
    let raw = Box::into_raw(Box::new(EditorBox::<P> {
        // Raw, from the instance's own pointer (not `&inst`), like the
        // other by-value vtables: valid while the instance lives, which
        // the reference just taken guarantees.
        // SAFETY: `inst_ptr` is the live instance `inst` borrows;
        // `addr_of!` reads no memory.
        vtbl: unsafe { ptr::addr_of!((*inst_ptr).view_vtbl) },
        ref_count: AtomicU32::new(1),
        inst: inst_ptr,
        view,
        frame: Cell::new(ptr::null_mut()),
        #[cfg(all(unix, not(target_os = "macos")))]
        run_loop: run_loop::Link::new::<P>(),
    }));
    #[cfg(all(unix, not(target_os = "macos")))]
    // SAFETY: `raw` is the live box just made; its handlers point back at
    // it, and the box never moves.
    unsafe {
        run_loop::Link::bind(raw)
    };
    raw.cast()
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
    if unsafe { core::ffi::CStr::from_ptr(ty) } == PLATFORM {
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
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            {
                // SAFETY: `parent` is the host's NSView or HWND for this
                // platform's type (the only one `isPlatformTypeSupported`
                // accepts), on the UI thread.
                unsafe { editor.view.attach(parent) };
                K_RESULT_OK
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                // `X11EmbedWindowID`: the parent is a window id, passed
                // as the pointer's value.
                if let Err(error) = editor.view.attach(parent as usize as u32) {
                    let error = stave_core::diagnostics::CallError::WindowRefused(error.to_string());
                    return inst.answer("IPlugView::attached", Err(error));
                }
                // SAFETY: `this` is the live box on the UI thread.
                if unsafe { run_loop::join::<P>(this.cast()) } {
                    K_RESULT_OK
                } else {
                    editor.view.detach();
                    let error = stave_core::diagnostics::CallError::WindowRefused("the frame offers no IRunLoop".into());
                    inst.answer("IPlugView::attached", Err(error))
                }
            }
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
            #[cfg(all(unix, not(target_os = "macos")))]
            // SAFETY: `this` is the live box on the UI thread.
            unsafe {
                run_loop::leave::<P>(this.cast())
            };
            editor.view.detach();
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
            let (w, h) = host_size(&editor.view);
            *size = ViewRect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
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
pub(super) unsafe extern "C" fn sv_set_frame<P: Vst3Export>(
    this: *mut FUnknown,
    frame: *mut FUnknown,
) -> Tresult {
    // SAFETY: host contract: `this` is a live view. A pointer store.
    unsafe { view_box::<P>(this) }.frame.set(frame);
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
        set_frame: sv_set_frame::<P>,
        can_resize: sv_can_resize,
        check_size_constraint: sv_check_size_constraint,
    }
}

/// The Linux view on the host's `IRunLoop`: two COM objects inside the
/// view's box — an `IEventHandler` for the display connection's
/// descriptor and an `ITimerHandler` for repaints — whose references are
/// the view's own (a host holding a handler holds the view).
#[cfg(all(unix, not(target_os = "macos")))]
mod run_loop {
    use super::*;
    use stave_vst3_sys::{
        IEventHandlerVtbl, IRunLoopVtbl, ITimerHandlerVtbl, IID_IEVENT_HANDLER, IID_IRUN_LOOP, IID_ITIMER_HANDLER,
    };

    /// Repaint and input latency: about 60 Hz.
    const PERIOD_MS: u64 = 16;

    /// One handler object: its vtable, and the box it belongs to.
    #[repr(C)]
    pub(super) struct Handler {
        vtbl: *const c_void,
        owner: *mut FUnknown,
    }

    /// The handlers, their vtables (by value: no per-plugin statics), and
    /// the run loop while joined (one reference, from queryInterface).
    pub(in crate::wrapper) struct Link {
        events: Handler,
        timer: Handler,
        events_vtbl: IEventHandlerVtbl,
        timer_vtbl: ITimerHandlerVtbl,
        joined: Cell<*mut FUnknown>,
    }

    impl Link {
        pub(super) fn new<P: Vst3Export>() -> Self {
            Link {
                events: Handler { vtbl: ptr::null(), owner: ptr::null_mut() },
                timer: Handler { vtbl: ptr::null(), owner: ptr::null_mut() },
                events_vtbl: IEventHandlerVtbl {
                    query_interface: handler_qi::<P>,
                    add_ref: handler_add_ref::<P>,
                    release: handler_release::<P>,
                    on_fd_is_set: on_fd_is_set::<P>,
                },
                timer_vtbl: ITimerHandlerVtbl {
                    query_interface: handler_qi::<P>,
                    add_ref: handler_add_ref::<P>,
                    release: handler_release::<P>,
                    on_timer: on_timer::<P>,
                },
                joined: Cell::new(ptr::null_mut()),
            }
        }

        /// Point the handlers at their vtables and their box.
        ///
        /// # Safety
        /// `editor` is a live box, not yet shared with the host.
        pub(super) unsafe fn bind<P: Vst3Export>(editor: *mut EditorBox<P>) {
            // SAFETY: this function's contract: we hold the only pointer.
            let link = unsafe { &mut (*editor).run_loop };
            link.events.vtbl = ptr::addr_of!(link.events_vtbl).cast();
            link.timer.vtbl = ptr::addr_of!(link.timer_vtbl).cast();
            link.events.owner = editor.cast();
            link.timer.owner = editor.cast();
        }
    }

    /// The box a handler belongs to.
    ///
    /// # Safety
    /// `this` is one of a live box's handlers.
    unsafe fn owner<'a, P: Vst3Export>(this: *mut FUnknown) -> &'a EditorBox<P> {
        // SAFETY: this function's contract; `owner` is the box (`bind`).
        unsafe { &*(*this.cast::<Handler>()).owner.cast::<EditorBox<P>>() }
    }

    unsafe extern "C" fn handler_qi<P: Vst3Export>(
        this: *mut FUnknown,
        iid: *const Tuid,
        obj: *mut *mut c_void,
    ) -> Tresult {
        // SAFETY: SDK: `iid` and `obj` are null or valid.
        let (iid, obj) = unsafe { (iid.as_ref(), out_param(obj, "obj")) };
        let (Some(&requested), Ok(obj)) = (iid, obj) else {
            return K_INVALID_ARGUMENT;
        };
        // SAFETY: `this` is one of a live box's handlers (host contract).
        let link = unsafe { &owner::<P>(this).run_loop };
        let is_events = ptr::eq(this.cast::<Handler>(), &link.events);
        let offered = if is_events { IID_IEVENT_HANDLER } else { IID_ITIMER_HANDLER };
        if requested == IID_FUNKNOWN || requested == offered {
            *obj = this.cast();
            // SAFETY: as above.
            unsafe { handler_add_ref::<P>(this) };
            K_RESULT_OK
        } else {
            *obj = ptr::null_mut();
            K_NO_INTERFACE
        }
    }

    unsafe extern "C" fn handler_add_ref<P: Vst3Export>(this: *mut FUnknown) -> u32 {
        // SAFETY: host contract: `this` is a live handler; its references
        // are its view's.
        unsafe { sv_add_ref::<P>((*this.cast::<Handler>()).owner) }
    }

    unsafe extern "C" fn handler_release<P: Vst3Export>(this: *mut FUnknown) -> u32 {
        // SAFETY: as in `handler_add_ref`.
        unsafe { sv_release::<P>((*this.cast::<Handler>()).owner) }
    }

    fn pump<P: Vst3Export>(editor: &EditorBox<P>, call: &str) {
        // SAFETY: the box's instance reference keeps it alive.
        let inst = unsafe { &*editor.inst };
        inst.guarded(call, || (), || {
            editor.view.pump();
        });
    }

    unsafe extern "C" fn on_fd_is_set<P: Vst3Export>(this: *mut FUnknown, _fd: i32) {
        // SAFETY: host contract: a registered (live) handler, on the UI thread.
        pump(unsafe { owner::<P>(this) }, "IEventHandler::onFDIsSet");
    }

    unsafe extern "C" fn on_timer<P: Vst3Export>(this: *mut FUnknown) {
        // SAFETY: as in `on_fd_is_set`.
        pump(unsafe { owner::<P>(this) }, "ITimerHandler::onTimer");
    }

    /// The run loop's vtable.
    ///
    /// # Safety
    /// `run_loop` is a live `IRunLoop`.
    unsafe fn vtbl<'a>(run_loop: *mut FUnknown) -> &'a IRunLoopVtbl {
        // SAFETY: this function's contract.
        unsafe { &*(*run_loop).vtbl.cast::<IRunLoopVtbl>() }
    }

    /// Register the attached view's descriptor and timer on the frame's
    /// run loop. False (nothing registered) without one.
    ///
    /// # Safety
    /// `editor` is a live box, on the UI thread.
    pub(super) unsafe fn join<P: Vst3Export>(editor: *mut EditorBox<P>) -> bool {
        // SAFETY: this function's contract.
        unsafe { leave(editor) };
        // SAFETY: as above; only shared references from here on.
        let editor = unsafe { &*editor };
        let frame = editor.frame.get();
        if frame.is_null() {
            return false;
        }
        let mut run_loop: *mut c_void = ptr::null_mut();
        // SAFETY: the host's live frame (until the next `setFrame`); its
        // vtable starts with FUnknown's.
        let found = unsafe {
            let unknown = &*(*frame).vtbl.cast::<IRunLoopVtbl>();
            (unknown.query_interface)(frame, &IID_IRUN_LOOP, &mut run_loop)
        };
        if found != K_RESULT_OK || run_loop.is_null() {
            return false;
        }
        let run_loop: *mut FUnknown = run_loop.cast();
        let link = &editor.run_loop;
        let events = ptr::addr_of!(link.events).cast_mut().cast::<FUnknown>();
        let timer = ptr::addr_of!(link.timer).cast_mut().cast::<FUnknown>();
        // SAFETY: the run loop queryInterface returned (one reference,
        // ours); the handlers live as long as the box, which the host
        // keeps alive through the references it takes on them.
        unsafe {
            let v = vtbl(run_loop);
            if let Some(fd) = editor.view.fd() {
                (v.register_event_handler)(run_loop, events, fd);
            }
            (v.register_timer)(run_loop, timer, PERIOD_MS);
        }
        link.joined.set(run_loop);
        true
    }

    /// Unregister whatever `join` registered, and release the run loop.
    ///
    /// # Safety
    /// As [`join`].
    pub(super) unsafe fn leave<P: Vst3Export>(editor: *mut EditorBox<P>) {
        // SAFETY: this function's contract.
        let link = unsafe { &(*editor).run_loop };
        let run_loop = link.joined.replace(ptr::null_mut());
        if run_loop.is_null() {
            return;
        }
        let events = ptr::addr_of!(link.events).cast_mut().cast::<FUnknown>();
        let timer = ptr::addr_of!(link.timer).cast_mut().cast::<FUnknown>();
        // SAFETY: the run loop `join` holds a reference on; the handlers
        // it registered.
        unsafe {
            let v = vtbl(run_loop);
            (v.unregister_event_handler)(run_loop, events);
            (v.unregister_timer)(run_loop, timer);
            (v.release)(run_loop);
        }
    }
}
