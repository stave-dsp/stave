use core::ffi::{c_char, c_void};

extern crate alloc;

use super::*;

// ============================================================================
// Raw objc runtime FFI
// ============================================================================

pub(crate) type Id = *mut c_void;
pub(crate) type Sel = *mut c_void;
pub(crate) type Class = *mut c_void;

#[expect(
    clippy::duplicated_attributes,
    reason = "one #[link] per framework repeats kind"
)]
#[link(name = "objc")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub(crate) fn objc_getClass(name: *const c_char) -> Class;
    pub(crate) fn sel_registerName(name: *const c_char) -> Sel;
    pub(crate) fn objc_msgSend();
    pub(crate) fn objc_allocateClassPair(
        superclass: Class,
        name: *const c_char,
        extra: usize,
    ) -> Class;
    pub(crate) fn class_addMethod(
        cls: Class,
        name: Sel,
        imp: *const c_void,
        types: *const c_char,
    ) -> bool;
    pub(crate) fn class_addIvar(
        cls: Class,
        name: *const c_char,
        size: usize,
        alignment: u8,
        types: *const c_char,
    ) -> bool;
    pub(crate) fn class_getInstanceVariable(cls: Class, name: *const c_char) -> *mut c_void;
    pub(crate) fn object_setIvar(obj: Id, ivar: *mut c_void, value: Id);
    pub(crate) fn object_getIvar(obj: Id, ivar: *mut c_void) -> Id;
    pub(crate) fn objc_registerClassPair(cls: Class);
    pub(crate) fn objc_lookUpClass(name: *const c_char) -> Class;
    pub(crate) fn object_getClass(obj: Id) -> Class;
}

pub(crate) unsafe extern "C" fn is_flipped_yes(_this: Id, _cmd: Sel) -> bool {
    true
}

/// Mouse drag handler for the spectrum view: maps view-local (x, y)
/// to (band, dB) and writes the band's threshold into the bus. Used
/// by both mouseDown: and mouseDragged: so a click sets the threshold
/// immediately and a drag updates it continuously.
pub(crate) unsafe fn spectrum_drag_apply(this: Id, event: Id) {
    let ivar = unsafe {
        class_getInstanceVariable(
            objc_lookUpClass(c"StaveSpectrumView".as_ptr()),
            c"editor_ptr".as_ptr(),
        )
    };
    if ivar.is_null() {
        return;
    }
    let editor_ptr_id = unsafe { object_getIvar(this, ivar) };
    let editor_ptr = editor_ptr_id as *const PluginEditorView;
    if editor_ptr.is_null() {
        return;
    }
    let editor = unsafe { &*editor_ptr };

    // Get event.locationInWindow → convert to view local.
    let win_loc: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(event, sel(c"locationInWindow"))
    };
    let view_loc: CGPoint = unsafe {
        let f: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        f(
            this,
            sel(c"convertPoint:fromView:"),
            win_loc,
            core::ptr::null_mut(),
        )
    };
    // Spectrum is 720×64 logical points; convert to dB.
    let h: f64 = 64.0;
    let w: f64 = 720.0;
    let db_min: f64 = -78.0;
    let db_max: f64 = -6.0;
    let db_range = db_max - db_min;
    let y = view_loc.y.clamp(0.0, h - 1.0);
    let x = view_loc.x.clamp(0.0, w - 1.0);
    let frac = 1.0 - y / (h - 1.0);
    let db = db_min + frac * db_range;

    // Determine band: compare hz at this x to crossovers.
    let log_min = 20.0_f64.log10();
    let log_max = 24_000.0_f64.log10();
    let t = x / (w - 1.0);
    let hz = 10_f64.powf(log_min + t * (log_max - log_min));
    let low_xover = editor
        .spectrum_low_xover_ctrl
        .map(|ci| editor.plain(ci))
        .unwrap_or(250.0);
    let high_xover = editor
        .spectrum_high_xover_ctrl
        .map(|ci| editor.plain(ci))
        .unwrap_or(2500.0);
    let thresh_ci = if hz < low_xover {
        editor.spectrum_lo_thresh_ctrl
    } else if hz < high_xover {
        editor.spectrum_mid_thresh_ctrl
    } else {
        editor.spectrum_hi_thresh_ctrl
    };
    if let Some(ci) = thresh_ci {
        // Spectrum threshold drag registers no mouseUp; bracket per
        // apply (same note as StavePad).
        editor.write_control_plain(ci, db, true);
    }
}

pub(crate) unsafe extern "C" fn spectrum_mouse_down(this: Id, _cmd: Sel, event: Id) {
    unsafe { spectrum_drag_apply(this, event) };
}

pub(crate) unsafe extern "C" fn spectrum_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    unsafe { spectrum_drag_apply(this, event) };
}

/// Register the StaveSpectrumView class (subclass of NSImageView) with
/// isFlipped + mouseDown:/mouseDragged: forwarded into the drag
/// handler above, plus an editor_ptr ivar so the handler can locate
/// the bus.
pub(crate) unsafe fn spectrum_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveSpectrumView".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsimageview = class(c"NSImageView");
    let cls = unsafe { objc_allocateClassPair(nsimageview, c"StaveSpectrumView".as_ptr(), 0) };
    let _ = unsafe {
        class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        )
    };
    // isFlipped: returns BOOL.
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"isFlipped"),
            is_flipped_yes as *const c_void,
            c"B@:".as_ptr(),
        )
    };
    // mouseDown:/mouseDragged: have signature "v@:@" (void, id, SEL, id event).
    let v_at_sel_at = c"v@:@";
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDown:"),
            spectrum_mouse_down as *const c_void,
            v_at_sel_at.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"mouseDragged:"),
            spectrum_mouse_dragged as *const c_void,
            v_at_sel_at.as_ptr() as *const c_char,
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}

// ============================================================================
// CoreFoundation runloop FFI — sole way to schedule a periodic tick without
// owning an NSObject method (we don't dynamically create an objc class here).
// ============================================================================

pub(crate) type CFRunLoopRef = *mut c_void;
pub(crate) type CFRunLoopTimerRef = *mut c_void;
pub(crate) type CFTypeRef = *const c_void;
pub(crate) type CFAbsoluteTime = f64;
pub(crate) type CFTimeInterval = f64;
pub(crate) type CFOptionFlags = u64;
pub(crate) type CFIndex = isize;
pub(crate) type CFRunLoopTimerCallBack =
    unsafe extern "C" fn(timer: CFRunLoopTimerRef, info: *mut c_void);

#[repr(C)]
pub(crate) struct CFRunLoopTimerContext {
    pub(crate) version: CFIndex,
    pub(crate) info: *mut c_void,
    pub(crate) retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    pub(crate) release: Option<unsafe extern "C" fn(*const c_void)>,
    pub(crate) copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFTypeRef>,
}

extern "C" {
    pub(crate) fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub(crate) fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFTypeRef);
    pub(crate) fn CFRunLoopTimerCreate(
        allocator: CFTypeRef,
        fire_date: CFAbsoluteTime,
        interval: CFTimeInterval,
        flags: CFOptionFlags,
        order: CFIndex,
        callout: CFRunLoopTimerCallBack,
        context: *mut CFRunLoopTimerContext,
    ) -> CFRunLoopTimerRef;
    pub(crate) fn CFAbsoluteTimeGetCurrent() -> CFAbsoluteTime;
    pub(crate) static kCFRunLoopCommonModes: CFTypeRef;
}

pub(crate) unsafe extern "C" fn tick_callback(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    if info.is_null() {
        return;
    }
    let view = unsafe { &mut *(info as *mut PluginEditorView) };
    view.tick();
}
