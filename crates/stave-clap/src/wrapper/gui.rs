//! The `clap.gui` extension: the plugin's face, embedded in the host's
//! window, never floating — an NSView (`cocoa`) on macOS, an HWND
//! (`win32`) on Windows, an X11 window (`x11`) on Linux. `create` makes
//! the platform's `FaceView` over the instance's `controller`;
//! `set_parent` attaches it; `destroy` drops it. The face has its
//! natural size and does not resize. Every call is [main-thread] (clap
//! gui.h), which is what lets the instance keep the view in a `RefCell`.
//!
//! On X11 the view joins the host's run loop: `set_parent` registers
//! the display connection's descriptor (`clap.posix-fd-support`) and a
//! repaint timer (`clap.timer-support`), and both callbacks pump the
//! view; `destroy` unregisters them.

#[cfg(target_os = "macos")]
pub(super) use stave_ui_cocoa::FaceView;
#[cfg(windows)]
pub(super) use stave_ui_win32::FaceView;
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) use stave_ui_x11::FaceView;

use super::*;

/// The one window API a face supports on this platform.
#[cfg(target_os = "macos")]
const API: &CStr = sys::CLAP_WINDOW_API_COCOA;
#[cfg(windows)]
const API: &CStr = sys::CLAP_WINDOW_API_WIN32;
#[cfg(all(unix, not(target_os = "macos")))]
const API: &CStr = sys::CLAP_WINDOW_API_X11;

/// A view's size in the host's unit: whole points on macOS, device
/// pixels on Windows and X11 (clap gui.h).
#[cfg(target_os = "macos")]
fn host_size(view: &FaceView) -> (u32, u32) {
    let (w, h) = view.size();
    (w.ceil() as u32, h.ceil() as u32)
}
#[cfg(not(target_os = "macos"))]
fn host_size(view: &FaceView) -> (u32, u32) {
    view.host_size()
}

/// The one window API a face supports here, embedded.
fn supported<P: ClapExport>(inst: &Instance<P>, api: Result<&CStr, CallError>, is_floating: bool) -> bool {
    inst.face.is_some() && !is_floating && api.is_ok_and(|api| api == API)
}

pub(super) unsafe extern "C" fn gui_is_api_supported<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    // SAFETY: host contract (see `host`); `api` is null or nul-terminated.
    let (inst, api) = unsafe { (Instance::<P>::from_plugin_ptr(plugin), in_text(api, "api")) };
    supported(inst, api, is_floating)
}

pub(super) unsafe extern "C" fn gui_get_preferred_api<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    // SAFETY: host contract (see `host`); the out-params are null or
    // writable (clap.h).
    let (inst, api, is_floating) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(api, "api"),
            out_param(is_floating, "is_floating"),
        )
    };
    if inst.face.is_none() {
        return false;
    }
    inst.call("gui.get_preferred_api", || {
        *api? = API.as_ptr();
        *is_floating? = false;
        Ok(())
    })
}

pub(super) unsafe extern "C" fn gui_create<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    // SAFETY: host contract (see `host`); `api` is null or nul-terminated.
    let (inst, api) = unsafe { (Instance::<P>::from_plugin_ptr(plugin), in_text(api, "api")) };
    inst.guarded(
        "gui.create",
        || false,
        || {
            let Some(face) = inst.face.clone().filter(|_| supported(inst, api, is_floating)) else {
                return false;
            };
            let view = FaceView::new(face, Box::new(controller_of(inst)));
            *inst.editor.borrow_mut() = Some(view);
            true
        },
    )
}

pub(super) unsafe extern "C" fn gui_destroy<P: ClapExport>(plugin: *const sys::ClapPlugin) {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    // Dropping the view detaches it; a panicking destructor must not
    // unwind into the host.
    ffi_entry::<P, _>(
        || (),
        || {
            #[cfg(all(unix, not(target_os = "macos")))]
            run_loop::leave(inst);
            drop(inst.editor.borrow_mut().take())
        },
    );
}

/// Cocoa sizes are in points; the view reads its own backing scale.
/// (Generic only to share the vtable's shape with the other platforms.)
#[cfg(target_os = "macos")]
#[allow(clippy::extra_unused_type_parameters)]
pub(super) unsafe extern "C" fn gui_set_scale<P: ClapExport>(_plugin: *const sys::ClapPlugin, _scale: f64) -> bool {
    false
}

/// Windows and X11 hosts say the scale; the view paints at it.
#[cfg(not(target_os = "macos"))]
pub(super) unsafe extern "C" fn gui_set_scale<P: ClapExport>(plugin: *const sys::ClapPlugin, scale: f64) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guarded(
        "gui.set_scale",
        || false,
        || match inst.editor.borrow_mut().as_mut() {
            Some(view) if scale.is_finite() && scale > 0.0 => {
                view.set_scale(scale);
                true
            }
            _ => false,
        },
    )
}

pub(super) unsafe extern "C" fn gui_get_size<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    // SAFETY: host contract (see `host`); the out-params are null or
    // writable (clap.h).
    let (inst, width, height) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(width, "width"),
            out_param(height, "height"),
        )
    };
    inst.guarded(
        "gui.get_size",
        || false,
        || {
            let Some((w, h)) = inst.editor.borrow().as_ref().map(host_size) else {
                return false;
            };
            let write = || -> Result<(), CallError> {
                *width? = w;
                *height? = h;
                Ok(())
            };
            inst.answer("gui.get_size", write())
        },
    )
}

pub(super) unsafe extern "C" fn gui_can_resize(_plugin: *const sys::ClapPlugin) -> bool {
    false
}

pub(super) unsafe extern "C" fn gui_get_resize_hints(
    _plugin: *const sys::ClapPlugin,
    _hints: *mut sys::ClapGuiResizeHints,
) -> bool {
    false
}

pub(super) unsafe extern "C" fn gui_adjust_size(
    _plugin: *const sys::ClapPlugin,
    _width: *mut u32,
    _height: *mut u32,
) -> bool {
    false
}

pub(super) unsafe extern "C" fn gui_set_size<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    width: u32,
    height: u32,
) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    // Not resizable: only the face's own size is accepted.
    inst.guarded(
        "gui.set_size",
        || false,
        || inst.editor.borrow().as_ref().map(host_size) == Some((width, height)),
    )
}

pub(super) unsafe extern "C" fn gui_set_parent<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    window: *const sys::ClapWindow,
) -> bool {
    // SAFETY: host contract (see `host`); `window` is null or a valid
    // `clap_window` for the call.
    let (inst, window) = unsafe { (Instance::<P>::from_plugin_ptr(plugin), window.as_ref()) };
    inst.guarded(
        "gui.set_parent",
        || false,
        || {
            let Some(window) = window else {
                return inst.answer("gui.set_parent", Err(CallError::NullArgument("window")));
            };
            // SAFETY: `api` is null or nul-terminated (clap.h).
            let api = unsafe { in_text(window.api, "window.api") };
            if !api.is_ok_and(|api| api == API) {
                return false;
            }
            let mut editor = inst.editor.borrow_mut();
            let Some(view) = editor.as_mut() else {
                return false;
            };
            #[cfg(target_os = "macos")]
            // SAFETY: a `cocoa` window carries the host's live NSView
            // (clap gui.h). Off the main thread the view refuses, not
            // panics: a GUI fault never latches the audio instance.
            if let Err(error) = unsafe { view.attach(window.handle.cocoa) } {
                return inst.answer("gui.set_parent", Err(CallError::WindowRefused(error.to_string())));
            }
            #[cfg(windows)]
            // SAFETY: a `win32` window carries the host's live HWND
            // (clap gui.h), on the main (UI) thread.
            unsafe {
                view.attach(window.handle.win32)
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                // SAFETY: an `x11` window carries an X11 window id (clap
                // gui.h); ids fit 29 bits (X11 protocol).
                let parent = unsafe { window.handle.x11 } as u32;
                if let Err(error) = view.attach(parent) {
                    return inst.answer("gui.set_parent", Err(CallError::WindowRefused(error.to_string())));
                }
                drop(editor);
                return run_loop::join(inst);
            }
            #[allow(unreachable_code)]
            true
        },
    )
}

pub(super) unsafe extern "C" fn gui_set_transient(
    _plugin: *const sys::ClapPlugin,
    _window: *const sys::ClapWindow,
) -> bool {
    false
}

pub(super) unsafe extern "C" fn gui_suggest_title(_plugin: *const sys::ClapPlugin, _title: *const c_char) {}

pub(super) unsafe extern "C" fn gui_show<P: ClapExport>(plugin: *const sys::ClapPlugin) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    // Embedded: the view shows with its parent.
    inst.editor.borrow().is_some()
}

pub(super) unsafe extern "C" fn gui_hide<P: ClapExport>(plugin: *const sys::ClapPlugin) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.editor.borrow().is_some()
}

pub(super) const fn gui_vtable<P: ClapExport>() -> sys::ClapPluginGui {
    sys::ClapPluginGui {
        is_api_supported: Some(gui_is_api_supported::<P>),
        get_preferred_api: Some(gui_get_preferred_api::<P>),
        create: Some(gui_create::<P>),
        destroy: Some(gui_destroy::<P>),
        set_scale: Some(gui_set_scale::<P>),
        get_size: Some(gui_get_size::<P>),
        can_resize: Some(gui_can_resize),
        get_resize_hints: Some(gui_get_resize_hints),
        adjust_size: Some(gui_adjust_size),
        set_size: Some(gui_set_size::<P>),
        set_parent: Some(gui_set_parent::<P>),
        set_transient: Some(gui_set_transient),
        suggest_title: Some(gui_suggest_title),
        show: Some(gui_show::<P>),
        hide: Some(gui_hide::<P>),
    }
}

/// The X11 view on the host's run loop (CLAP's `posix-fd-support` and
/// `timer-support`): the host calls back on the main thread, and each
/// callback pumps the view.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) mod run_loop {
    use super::*;

    /// Repaint and input latency: about 60 Hz.
    const PERIOD_MS: u32 = 16;

    /// Register the attached view's descriptor and timer. False (and
    /// nothing registered) when the host offers no timer: without one the
    /// face could not animate or repaint.
    pub(in crate::wrapper) fn join<P: ClapExport>(inst: &Instance<P>) -> bool {
        leave(inst);
        let Some(host) = inst.host else { return false };
        let Some(timer) = host.register_timer(PERIOD_MS) else {
            return inst.answer("gui.set_parent", Err(CallError::WindowRefused("the host has no clap.timer-support".into())));
        };
        let fd = inst.editor.borrow().as_ref().and_then(FaceView::fd);
        let fd = fd.filter(|&fd| host.register_fd(fd, sys::CLAP_POSIX_FD_READ));
        inst.run_loop.set(Some((timer, fd)));
        true
    }

    /// Unregister whatever `join` registered.
    pub(in crate::wrapper) fn leave<P: ClapExport>(inst: &Instance<P>) {
        let (Some((timer, fd)), Some(host)) = (inst.run_loop.take(), inst.host) else { return };
        host.unregister_timer(timer);
        if let Some(fd) = fd {
            host.unregister_fd(fd);
        }
    }

    fn pump<P: ClapExport>(inst: &Instance<P>) {
        // A reentrant callback (from inside the view's own pump) waits
        // for the next one.
        if let Ok(editor) = inst.editor.try_borrow() {
            if let Some(view) = editor.as_ref() {
                view.pump();
            }
        }
    }

    pub(in crate::wrapper) unsafe extern "C" fn on_timer<P: ClapExport>(plugin: *const sys::ClapPlugin, _id: sys::ClapId) {
        // SAFETY: host contract (see `host`).
        let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
        inst.guarded("timer-support.on_timer", || (), || pump(inst));
    }

    pub(in crate::wrapper) unsafe extern "C" fn on_fd<P: ClapExport>(
        plugin: *const sys::ClapPlugin,
        _fd: core::ffi::c_int,
        _flags: u32,
    ) {
        // SAFETY: host contract (see `host`).
        let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
        inst.guarded("posix-fd-support.on_fd", || (), || pump(inst));
    }
}
