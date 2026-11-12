//! A small typed Objective-C core: every message the crate sends goes
//! through [`send`], whose argument and return types pick the C
//! signature it is called with (and the struct-return entry point where
//! the ABI needs one); selectors are cached ([`sel!`]); an owned
//! reference is a [`Retained`]; a [`MainThread`] token proves where AppKit
//! may be touched; [`define_class`] registers a class under a name unique
//! to this binary, with typed method implementations.
//!
//! The unsafe surface is `send` (the caller states the method's real
//! signature), `Retained::from_owned` (the caller hands over a +1) and the
//! raw runtime calls inside this file.

use core::ffi::{c_char, c_void, CStr};
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::cf::{CGPoint, CGRect, CGSize};

#[expect(
    clippy::duplicated_attributes,
    reason = "one #[link] per framework repeats kind"
)]
#[link(name = "objc")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Option<Obj>;
    fn sel_registerName(name: *const c_char) -> Option<Sel>;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_allocateClassPair(superclass: Obj, name: *const c_char, extra: usize) -> Option<Obj>;
    fn objc_registerClassPair(class: Obj);
    fn class_addMethod(class: Obj, name: Sel, imp: *const c_void, types: *const c_char) -> ObjcBool;
    fn class_addIvar(class: Obj, name: *const c_char, size: usize, alignment: u8, types: *const c_char) -> ObjcBool;
    fn class_getInstanceVariable(class: Obj, name: *const c_char) -> Option<NonNull<c_void>>;
    fn ivar_getOffset(ivar: NonNull<c_void>) -> isize;
    fn pthread_main_np() -> i32;
}

// ------------------------------------------------------------ the types

/// A non-nil Objective-C object (an instance or a class).
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Obj(NonNull<c_void>);

impl Obj {
    pub(crate) fn from_ptr(p: *mut c_void) -> Option<Obj> {
        NonNull::new(p).map(Obj)
    }
    pub(crate) fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }
}

/// A registered selector.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sel(NonNull<c_void>);

/// Objective-C's `BOOL`: C `bool` on arm64, `signed char` on x86_64.
#[cfg(target_arch = "aarch64")]
pub(crate) type ObjcBool = bool;
/// Objective-C's `BOOL`: C `bool` on arm64, `signed char` on x86_64.
#[cfg(not(target_arch = "aarch64"))]
pub(crate) type ObjcBool = i8;

/// YES and NO.
#[cfg(target_arch = "aarch64")]
pub(crate) const YES: ObjcBool = true;
#[cfg(target_arch = "aarch64")]
const NO: ObjcBool = false;
#[cfg(not(target_arch = "aarch64"))]
pub(crate) const YES: ObjcBool = 1;
#[cfg(not(target_arch = "aarch64"))]
const NO: ObjcBool = 0;

/// A selector registered once, on first use (see [`sel!`]).
pub(crate) struct CachedSel {
    name: &'static CStr,
    sel: AtomicPtr<c_void>,
}

impl CachedSel {
    pub(crate) const fn new(name: &'static CStr) -> Self {
        CachedSel { name, sel: AtomicPtr::new(core::ptr::null_mut()) }
    }

    pub(crate) fn get(&self) -> Sel {
        if let Some(sel) = NonNull::new(self.sel.load(Ordering::Relaxed)) {
            return Sel(sel);
        }
        // SAFETY: `name` is nul-terminated; registering is thread-safe
        // and idempotent (a race stores the same selector twice).
        let sel = unsafe { sel_registerName(self.name.as_ptr()) }.expect("the runtime registers any selector");
        self.sel.store(sel.0.as_ptr(), Ordering::Relaxed);
        sel
    }
}

/// The selector named by a C-string literal, registered once per call site.
macro_rules! sel {
    ($name:literal) => {{
        static SEL: $crate::objc::CachedSel = $crate::objc::CachedSel::new($name);
        SEL.get()
    }};
}
pub(crate) use sel;

/// The class named `name` (an AppKit or Foundation class this crate
/// links).
pub(crate) fn class(name: &CStr) -> Obj {
    // SAFETY: `name` is nul-terminated; the lookup is thread-safe.
    unsafe { objc_getClass(name.as_ptr()) }.unwrap_or_else(|| panic!("no Objective-C class {name:?}"))
}

// -------------------------------------------------------- typed messages

/// A Rust type that crosses a message as an argument.
///
/// # Safety
/// `Raw` has the C ABI of the Objective-C type it stands for.
pub(crate) unsafe trait Arg: Copy {
    type Raw: Copy;
    fn raw(self) -> Self::Raw;
}

/// A Rust type a message returns.
///
/// # Safety
/// `Raw` has the C ABI of the Objective-C return type it stands for, and
/// `STRET` says whether that type comes back through a hidden pointer on
/// this target (`objc_msgSend_stret`).
pub(crate) unsafe trait Ret {
    type Raw: Copy;
    const STRET: bool = false;
    fn from_raw(raw: Self::Raw) -> Self;
}

macro_rules! same_abi {
    ($($t:ty),*) => {$(
        // SAFETY: the type is its own C type.
        unsafe impl Arg for $t {
            type Raw = $t;
            fn raw(self) -> $t {
                self
            }
        }
        // SAFETY: as above; none of these is returned in memory.
        unsafe impl Ret for $t {
            type Raw = $t;
            fn from_raw(raw: $t) -> $t {
                raw
            }
        }
    )*};
}

same_abi!(f64, f32, i64, u64, u16, Option<Obj>, CGPoint, CGSize);

// SAFETY: a non-null object pointer is an `id`.
unsafe impl Arg for Obj {
    type Raw = Obj;
    fn raw(self) -> Obj {
        self
    }
}

// SAFETY: a `SEL`.
unsafe impl Arg for Sel {
    type Raw = Sel;
    fn raw(self) -> Sel {
        self
    }
}

// SAFETY: a C string pointer.
unsafe impl Arg for *const c_char {
    type Raw = *const c_char;
    fn raw(self) -> Self {
        self
    }
}

// SAFETY: `BOOL` is `ObjcBool` on every target (see there).
unsafe impl Arg for bool {
    type Raw = ObjcBool;
    fn raw(self) -> ObjcBool {
        if self { YES } else { NO }
    }
}

// SAFETY: as above; any `BOOL` but NO is YES.
unsafe impl Ret for bool {
    type Raw = ObjcBool;
    fn from_raw(raw: ObjcBool) -> bool {
        raw != NO
    }
}

// SAFETY: a struct of four doubles; by value as an argument on every
// Apple target.
unsafe impl Arg for CGRect {
    type Raw = CGRect;
    fn raw(self) -> CGRect {
        self
    }
}

// SAFETY: 32 bytes: returned in memory on x86_64 (so through
// `objc_msgSend_stret`), in registers on arm64.
unsafe impl Ret for CGRect {
    type Raw = CGRect;
    const STRET: bool = cfg!(target_arch = "x86_64");
    fn from_raw(raw: CGRect) -> CGRect {
        raw
    }
}

// SAFETY: `void`.
unsafe impl Ret for () {
    type Raw = ();
    fn from_raw(_: ()) {}
}

// SAFETY: a C pointer (an opaque CoreFoundation/CoreGraphics reference).
unsafe impl Ret for *mut c_void {
    type Raw = *mut c_void;
    fn from_raw(raw: *mut c_void) -> Self {
        raw
    }
}

/// The messenger for a return type: `objc_msgSend`, or its struct-return
/// twin where the type comes back in memory.
fn messenger<R: Ret>() -> unsafe extern "C" fn() {
    #[cfg(target_arch = "x86_64")]
    if R::STRET {
        return objc_msgSend_stret;
    }
    assert!(!R::STRET, "no struct-return messenger on this target");
    objc_msgSend
}

/// A message's arguments: a tuple of [`Arg`]s.
pub(crate) trait Args {
    /// # Safety
    /// As [`send`].
    unsafe fn send<R: Ret>(self, receiver: Obj, sel: Sel) -> R;
}

macro_rules! args {
    ($($a:ident),*) => {
        impl<$($a: Arg),*> Args for ($($a,)*) {
            #[allow(non_snake_case)]
            unsafe fn send<R: Ret>(self, receiver: Obj, sel: Sel) -> R {
                let ($($a,)*) = self;
                type Imp<R $(, $a)*> = unsafe extern "C" fn(Obj, Sel $(, <$a as Arg>::Raw)*) -> <R as Ret>::Raw;
                // SAFETY: the messenger jumps to the method with the
                // arguments where the caller put them, so calling it as
                // the method's own C signature is calling the method;
                // `send`'s caller guarantees this is that signature.
                let imp: Imp<R $(, $a)*> = unsafe { core::mem::transmute(messenger::<R>()) };
                // SAFETY: as above.
                R::from_raw(unsafe { imp(receiver, sel $(, $a.raw())*) })
            }
        }
    };
}

args!();
args!(A);
args!(A, B);
args!(A, B, C);
args!(A, B, C, D);
args!(A, B, C, D, E, F, G, H, I);

/// Send `sel` to `receiver` with `args`, the reply typed as `R`.
///
/// # Safety
/// `receiver` is a live object that responds to `sel`, whose method takes
/// exactly `args`' types and returns `R`'s; an AppKit receiver is used on
/// the main thread.
pub(crate) unsafe fn send<A: Args, R: Ret>(receiver: Obj, sel: Sel, args: A) -> R {
    // SAFETY: this function's contract.
    unsafe { args.send(receiver, sel) }
}

// ------------------------------------------------------------ ownership

/// An object this code owns one reference to, released on drop.
#[derive(Debug)]
pub(crate) struct Retained(Obj);

impl Retained {
    /// # Safety
    /// `obj` carries a +1 reference the caller hands over (from `alloc`
    /// + `init…`, `new`, `copy`), and may be released on this thread.
    pub(crate) unsafe fn from_owned(obj: Obj) -> Self {
        Retained(obj)
    }

    pub(crate) fn get(&self) -> Obj {
        self.0
    }
}

impl Drop for Retained {
    fn drop(&mut self) {
        // SAFETY: the reference `from_owned` was handed is ours to give
        // back, once, on the thread it was handed over on (`Retained`
        // is not `Send`).
        unsafe { send::<_, ()>(self.0, sel!(c"release"), ()) }
    }
}

/// Proof the code holding it runs on the main thread, where AppKit may be
/// used. Not `Send`.
#[derive(Copy, Clone, Debug)]
pub(crate) struct MainThread(PhantomData<*mut ()>);

impl MainThread {
    pub(crate) fn new() -> Option<Self> {
        // SAFETY: a plain query of the calling thread.
        (unsafe { pthread_main_np() } != 0).then_some(MainThread(PhantomData))
    }

    /// # Safety
    /// The caller runs on the main thread (an AppKit callback does).
    pub(crate) unsafe fn assume() -> Self {
        MainThread(PhantomData)
    }
}

// ---------------------------------------------------------- classes

/// A method implementation and its type encoding, together.
///
/// # Safety
/// `ENCODING` is the Objective-C type encoding of the function's
/// signature (return, `self`, `_cmd`, arguments).
pub(crate) unsafe trait Method: Copy {
    const ENCODING: &'static CStr;
    fn imp(self) -> *const c_void;
}

macro_rules! method {
    ($ty:ty, $enc:expr) => {
        // SAFETY: the encoding spells the signature, piece by piece.
        unsafe impl Method for $ty {
            const ENCODING: &'static CStr = $enc;
            fn imp(self) -> *const c_void {
                self as *const c_void
            }
        }
    };
}

/// The encodings of `BOOL m(id, SEL)` and `BOOL m(id, SEL, id)`.
#[cfg(target_arch = "aarch64")]
const BOOL_METHODS: [&CStr; 2] = [c"B@:", c"B@:@"];
#[cfg(not(target_arch = "aarch64"))]
const BOOL_METHODS: [&CStr; 2] = [c"c@:", c"c@:@"];

method!(extern "C" fn(Obj, Sel, Option<Obj>), c"v@:@");
method!(extern "C" fn(Obj, Sel, CGRect), c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}");
method!(extern "C" fn(Obj, Sel) -> ObjcBool, BOOL_METHODS[0]);
method!(extern "C" fn(Obj, Sel, Option<Obj>) -> ObjcBool, BOOL_METHODS[1]);

/// A class being declared (see [`define_class`]).
pub(crate) struct ClassDecl(Obj);

impl ClassDecl {
    /// Add a pointer-sized instance variable named `name`.
    pub(crate) fn ivar(&mut self, name: &CStr) {
        let size = core::mem::size_of::<*mut c_void>();
        // SAFETY: the class is allocated and not yet registered; the
        // strings are nul-terminated.
        let added = unsafe { class_addIvar(self.0, name.as_ptr(), size, size.trailing_zeros() as u8, c"^v".as_ptr()) };
        assert!(added != NO, "ivar {name:?} added once");
    }

    /// Implement `sel` with `imp`.
    pub(crate) fn method<M: Method>(&mut self, sel: Sel, imp: M) {
        // SAFETY: the class is not yet registered; `imp` has the
        // signature `ENCODING` spells (`Method`'s contract).
        let added = unsafe { class_addMethod(self.0, sel, imp.imp(), M::ENCODING.as_ptr()) };
        assert!(added != NO, "a method added once");
    }
}

/// Declare and register a subclass of `superclass` named `name` plus a
/// suffix unique to this binary — two plugins built on stave (each with
/// its own copy of this code) loaded into one host never share a class.
/// Call once per class (keep the result).
pub(crate) fn define_class(name: &str, superclass: &CStr, declare: impl FnOnce(&mut ClassDecl)) -> Obj {
    // The address of a static is unique to the loaded image.
    static ANCHOR: u8 = 0;
    let unique = std::ffi::CString::new(format!("{name}_{:x}", core::ptr::from_ref(&ANCHOR) as usize))
        .expect("a class name has no nul");
    // SAFETY: the superclass exists (`class`); the name is nul-terminated.
    let cls = unsafe { objc_allocateClassPair(class(superclass), unique.as_ptr(), 0) }
        .unwrap_or_else(|| panic!("class {unique:?} declared once"));
    let mut decl = ClassDecl(cls);
    declare(&mut decl);
    // SAFETY: allocated above and declared; registered once.
    unsafe { objc_registerClassPair(cls) };
    cls
}

/// The byte offset of instance variable `name` in `class`'s instances.
pub(crate) fn ivar_offset(class: Obj, name: &CStr) -> isize {
    // SAFETY: a registered class; `name` is nul-terminated.
    let ivar = unsafe { class_getInstanceVariable(class, name.as_ptr()) }.expect("the class declares the ivar");
    // SAFETY: `ivar` is the class's.
    unsafe { ivar_getOffset(ivar) }
}

/// A class pointer that may live in a static: classes are immutable once
/// registered and live for the process.
#[derive(Copy, Clone)]
pub(crate) struct ClassRef(pub(crate) Obj);

// SAFETY: a registered class is never mutated or freed, and the runtime's
// class functions are thread-safe.
unsafe impl Send for ClassRef {}
// SAFETY: as above.
unsafe impl Sync for ClassRef {}

/// `f()`, or `fallback` if it panics: the boundary every Objective-C
/// callback keeps, so a bug in a face never unwinds into AppKit (or the
/// host). The panic hook has already reported the panic.
pub(crate) fn guard<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    std::panic::catch_unwind(core::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Foundation (thread-safe, no window) answers through every typed
    /// path: doubles, `BOOL`s, and a struct return (`objc_msgSend_stret`
    /// on x86_64).
    #[test]
    fn typed_sends_round_trip_doubles_bools_and_rects() {
        // SAFETY: NSNumber's and NSValue's constructors and accessors,
        // with their real signatures; Foundation is thread-safe.
        unsafe {
            let n: Option<Obj> = send(class(c"NSNumber"), sel!(c"numberWithDouble:"), (2.5f64,));
            assert_eq!(send::<_, f64>(n.unwrap(), sel!(c"doubleValue"), ()), 2.5);
            for b in [false, true] {
                let n: Option<Obj> = send(class(c"NSNumber"), sel!(c"numberWithBool:"), (b,));
                assert_eq!(send::<_, bool>(n.unwrap(), sel!(c"boolValue"), ()), b);
            }
            let r = CGRect::new(1.0, 2.0, 3.0, 4.0);
            let v: Option<Obj> = send(class(c"NSValue"), sel!(c"valueWithRect:"), (r,));
            assert_eq!(send::<_, CGRect>(v.unwrap(), sel!(c"rectValue"), ()), r);
        }
    }

    /// A declared class is registered under this binary's own name, so
    /// a second copy of this code (another plugin) declares its own.
    #[test]
    fn a_declared_class_carries_a_suffix_unique_to_the_binary() {
        let cls = define_class("StaveObjcTest", c"NSObject", |c| c.ivar(c"slot"));
        // SAFETY: `className` is an NSObject method returning an NSString;
        // `UTF8String` its C string, live while the class is.
        let name = unsafe {
            let s: Option<Obj> = send(cls, sel!(c"className"), ());
            let p: *mut c_void = send(s.unwrap(), sel!(c"UTF8String"), ());
            CStr::from_ptr(p.cast()).to_str().unwrap().to_owned()
        };
        assert!(name.starts_with("StaveObjcTest_") && name.len() > "StaveObjcTest_".len(), "{name}");
        assert!(ivar_offset(cls, c"slot") >= core::mem::size_of::<*mut c_void>() as isize);
    }
}
