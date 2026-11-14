//! The `clap.gui` extension: the plugin's face, embedded in the host's
//! window, never floating — an NSView (`cocoa`) on macOS, an HWND
//! (`win32`) on Windows, an X11 window (`x11`) on Linux. `create` makes
//! the platform's `FaceView` over the instance's `controller`;
//! `set_parent` attaches it; `destroy` drops it. The face has its
//! natural size and does not resize. Every call is [main-thread] (clap
//! gui.h), which is what lets the instance keep the view in a
//! [`MainThreadCell`]. A host that calls back into `clap.gui` from inside
//! another gui call (say while the view attaches to its parent) is
//! refused — logged, `false` — rather than panicking on the cell.
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

/// The open editor, exclusively, for a gui call; `None` (the refusal
/// logged) while another gui call holds it.
fn editor_mut<'a, P: ClapExport>(inst: &'a Instance<P>, call: &str) -> Option<core::cell::RefMut<'a, Option<FaceView>>> {
    inst.editor.try_borrow_mut().inspect_err(|error| inst.report(call, error)).ok()
}

/// The open editor, shared, for a gui call; `None` (the refusal logged)
/// while another gui call holds it exclusively.
fn editor<'a, P: ClapExport>(inst: &'a Instance<P>, call: &str) -> Option<core::cell::Ref<'a, Option<FaceView>>> {
    inst.editor.try_borrow().inspect_err(|error| inst.report(call, error)).ok()
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
            let Some(mut editor) = editor_mut(inst, "gui.create") else {
                return false;
            };
            // A view the host never destroyed drops after the borrow ends.
            let stale = editor.replace(view);
            drop(editor);
            drop(stale);
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
            // Taken under the borrow, dropped after it: the view's
            // destructor may call back into the host.
            let Some(view) = editor_mut(inst, "gui.destroy").map(|mut editor| editor.take()) else {
                return;
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            run_loop::leave(inst);
            drop(view)
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
        || match editor_mut(inst, "gui.set_scale").as_deref_mut().and_then(Option::as_mut) {
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
            let Some((w, h)) = editor(inst, "gui.get_size").and_then(|editor| editor.as_ref().map(host_size)) else {
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
        || {
            editor(inst, "gui.set_size").and_then(|editor| editor.as_ref().map(host_size)) == Some((width, height))
        },
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
            // Held across `attach` (which needs the view exclusively): a
            // gui call the host makes from inside it is refused, not a
            // panic. Released before anything else calls the host (the
            // X11 run-loop registration below).
            let Some(mut editor) = editor_mut(inst, "gui.set_parent") else {
                return false;
            };
            let Some(view) = editor.as_mut() else {
                return false;
            };
            #[cfg(target_os = "macos")]
            // SAFETY: a `cocoa` window carries the host's live NSView
            // (clap gui.h), on the main thread.
            unsafe {
                view.attach(window.handle.cocoa)
            };
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
    inst.guarded("gui.show", || false, || editor(inst, "gui.show").is_some_and(|editor| editor.is_some()))
}

pub(super) unsafe extern "C" fn gui_hide<P: ClapExport>(plugin: *const sys::ClapPlugin) -> bool {
    // SAFETY: host contract (see `host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    inst.guarded("gui.hide", || false, || editor(inst, "gui.hide").is_some_and(|editor| editor.is_some()))
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
        let fd = editor(inst, "gui.set_parent").and_then(|editor| editor.as_ref().and_then(FaceView::fd));
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
        // A reentrant callback (from inside the view's own pump, or while
        // a gui call holds the editor) waits for the next one, unlogged.
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

#[cfg(test)]
mod tests {
    use stave_core::{Block, Info, Telemetry};

    use super::*;

    struct Faceless;
    impl Erased for Faceless {
        const INFO: Info = Info {
            id: "com.stave.test.gui-reentry",
            name: "GUI re-entry",
            vendor: "stave tests",
            ..Info::EFFECT
        };
        fn new(_: &Telemetry) -> Self {
            Faceless
        }
        fn process(&mut self, _block: &mut Block<'_>) {}
    }
    impl ClapExport for Faceless {}

    /// A host that calls back into `clap.gui` while another gui call
    /// holds the editor (here: the view attaching to its parent) is
    /// refused; the instance is not disabled and nothing unwinds into
    /// the host.
    #[test]
    fn a_reentrant_gui_call_is_refused_and_leaves_the_instance_alive() {
        let factory = factory::<Faceless>();
        // SAFETY: plays the host under clap.h: a live factory, a null
        // host, the plugin's own id; `plugin` lives until `destroy`.
        unsafe {
            let plugin = (factory.create_plugin.unwrap())(factory, ptr::null(), c"com.stave.test.gui-reentry".as_ptr());
            assert!(((*plugin).init.unwrap())(plugin));
            let inst = Instance::<Faceless>::from_plugin_ptr(plugin);
            let gui = gui_vtable::<Faceless>();
            let window = sys::ClapWindow { api: API.as_ptr(), handle: core::mem::zeroed() };
            let (mut w, mut h) = (0, 0);
            {
                // What `set_parent` holds while the view attaches.
                let _attaching = inst.editor.try_borrow_mut().unwrap();
                assert!(!(gui.show.unwrap())(plugin));
                assert!(!(gui.hide.unwrap())(plugin));
                assert!(!(gui.get_size.unwrap())(plugin, &mut w, &mut h));
                assert!(!(gui.set_size.unwrap())(plugin, 1, 1));
                assert!(!(gui.set_scale.unwrap())(plugin, 2.0));
                assert!(!(gui.set_parent.unwrap())(plugin, &window));
                (gui.destroy.unwrap())(plugin);
            }
            assert!(!inst.guard.disabled(), "a re-entrant gui call disabled the instance");
            ((*plugin).destroy.unwrap())(plugin);
        }
    }
}
