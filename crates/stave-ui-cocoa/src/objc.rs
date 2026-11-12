use core::ffi::{c_char, c_void};

extern crate alloc;

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

// ============================================================================
// CoreFoundation runloop FFI — the face view's repaint timer.
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

// ============================================================================
// CoreGraphics geometry and typed message sends
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}

pub(crate) fn sel(s: &core::ffi::CStr) -> Sel {
    unsafe { sel_registerName(s.as_ptr()) }
}

#[inline]
pub(crate) fn class(s: &core::ffi::CStr) -> Class {
    unsafe { objc_getClass(s.as_ptr()) }
}

#[inline]
pub(crate) unsafe fn msg_id(receiver: Id, selector: Sel) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector) }
}
#[inline]
pub(crate) unsafe fn msg_id_rect(receiver: Id, selector: Sel, rect: CGRect) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, CGRect) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, rect) }
}
#[inline]
pub(crate) unsafe fn msg_void_id(receiver: Id, selector: Sel, arg: Id) {
    let f: unsafe extern "C" fn(Id, Sel, Id) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, arg) };
}
#[inline]
pub(crate) unsafe fn msg_void_int(receiver: Id, selector: Sel, v: i64) {
    let f: unsafe extern "C" fn(Id, Sel, i64) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, v) };
}
