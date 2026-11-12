//! The `clap.gui` extension: the plugin's face, embedded in the host's
//! NSView (`cocoa`, never floating). `create` makes a `FaceView` over the
//! instance's `controller`; `set_parent` attaches it; `destroy` drops
//! it. The face has its natural size and does not resize. Every call is
//! [main-thread] (clap gui.h), which is what lets the instance keep the
//! view in a `RefCell`.

use stave_ui_cocoa::FaceView;

use super::*;

/// A view's size in whole points (CLAP's GUI unit on macOS).
fn points(view: &FaceView) -> (u32, u32) {
    let (w, h) = view.size();
    (w.ceil() as u32, h.ceil() as u32)
}

/// The one window API a face supports here: an embedded NSView.
fn supported<P: ClapExport>(inst: &Instance<P>, api: Result<&CStr, CallError>, is_floating: bool) -> bool {
    inst.face.is_some() && !is_floating && api.is_ok_and(|api| api == sys::CLAP_WINDOW_API_COCOA)
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
        *api? = sys::CLAP_WINDOW_API_COCOA.as_ptr();
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
    ffi_entry::<P, _>(|| (), || drop(inst.editor.borrow_mut().take()));
}

pub(super) unsafe extern "C" fn gui_set_scale(_plugin: *const sys::ClapPlugin, _scale: f64) -> bool {
    // Cocoa sizes are in points; the view reads its own backing scale.
    false
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
            let Some((w, h)) = inst.editor.borrow().as_ref().map(points) else {
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
        || inst.editor.borrow().as_ref().map(points) == Some((width, height)),
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
            if !api.is_ok_and(|api| api == sys::CLAP_WINDOW_API_COCOA) {
                return false;
            }
            let mut editor = inst.editor.borrow_mut();
            let Some(view) = editor.as_mut() else {
                return false;
            };
            // SAFETY: a `cocoa` window carries the host's live NSView
            // (clap gui.h), on the main thread.
            unsafe { view.attach(window.handle.cocoa) };
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
        set_scale: Some(gui_set_scale),
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
