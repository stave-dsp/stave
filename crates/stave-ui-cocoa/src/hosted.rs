//! A face in a view the host asks for and keeps, instead of one attached
//! to a parent the host gives: an Audio Unit's Cocoa view. The host
//! loads a *view factory* class from the plugin's bundle (by name, from
//! `kAudioUnitProperty_CocoaUI`), instantiates it, and asks it
//! `uiViewForAudioUnit:withSize:` (CoreAudioKit's `AUCocoaUIBase`
//! protocol); the answer is an NSView the host retains, adds to its
//! window and releases when it is done.
//!
//! [`view_factory`] registers that class once per binary and returns its
//! name; its method calls the function it was given to make the
//! [`FaceView`], and hands the host an NSView of the face's size that
//! owns it: the face is attached inside, and dropped (its timer stopped,
//! its view removed) when the host releases the container.

use core::ffi::{c_void, CStr};
use std::ffi::CString;
use std::sync::OnceLock;

use crate::cf::{CGRect, CGSize};
use crate::objc::{class, define_class, guard, ivar_offset, send, sel, super_dealloc, ClassRef, MainThread, Obj, Sel};
use crate::view::FaceView;

/// What makes the view for an Audio Unit: the `AudioUnit` the host
/// passes, to the face for it (or `None`).
pub type MakeView = fn(*mut c_void) -> Option<FaceView>;

static MAKE: OnceLock<MakeView> = OnceLock::new();

struct Classes {
    factory_name: CString,
    /// The bundle the runtime attributes the factory to.
    bundle: String,
    container: ClassRef,
    /// The container's `face` ivar: a leaked `Box<FaceView>`.
    face: isize,
}

fn classes() -> &'static Classes {
    static CLASSES: OnceLock<Classes> = OnceLock::new();
    CLASSES.get_or_init(|| {
        let container = define_class("StaveHostedView", c"NSView", |c| {
            c.ivar(c"face");
            c.method(sel!(c"dealloc"), dealloc as extern "C" fn(Obj, Sel));
        });
        let version = (sel!(c"interfaceVersion"), c"I@:");
        let make = (sel!(c"uiViewForAudioUnit:withSize:"), c"@@:^v{CGSize=dd}");
        let factory = define_class("StaveViewFactory", c"NSObject", |c| {
            c.protocol(c"AUCocoaUIBase", &[version, make]);
            c.method(version.0, interface_version as extern "C" fn(Obj, Sel) -> u32);
            c.method(make.0, ui_view as extern "C" fn(Obj, Sel, *mut c_void, CGSize) -> Option<Obj>);
        });
        // SAFETY: `className` returns an NSString whose `UTF8String`
        // lives as long as the (never freed) class.
        let factory_name = unsafe {
            let s: Option<Obj> = send(factory, sel!(c"className"), ());
            let p: *mut c_void = send(s.expect("a class has a name"), sel!(c"UTF8String"), ());
            CStr::from_ptr(p.cast()).to_owned()
        };
        // A class made at run time lies in no image, so Foundation puts
        // it in the main bundle (the host's): the bundle a host loads the
        // class from must be the one `bundleForClass:` names, or its
        // `classNamed:` finds nothing. (JUCE's Audio Units do the same.)
        // SAFETY: NSBundle class methods with their real signatures; the
        // path's `UTF8String` is copied while the bundle lives.
        let bundle = unsafe {
            let b: Option<Obj> = send(class(c"NSBundle"), sel!(c"bundleForClass:"), (factory,));
            let path: Option<Obj> = send(b.expect("every class has a bundle"), sel!(c"bundlePath"), ());
            let p: *mut c_void = send(path.expect("a bundle has a path"), sel!(c"UTF8String"), ());
            CStr::from_ptr(p.cast()).to_string_lossy().into_owned()
        };
        Classes {
            factory_name,
            bundle,
            container: ClassRef(container),
            face: ivar_offset(container, c"face"),
        }
    })
}

/// Where a host finds the view factory: the bundle to load it from and
/// its class name (unique to this binary).
#[derive(Copy, Clone, Debug)]
pub struct ViewFactory {
    /// The bundle's path, as a host passes it to `NSBundle`.
    pub bundle: &'static str,
    pub class: &'static CStr,
}

/// Register the view factory (once per binary; later calls keep the
/// first `make`) and say where a host finds it.
pub fn view_factory(make: MakeView) -> ViewFactory {
    let _ = MAKE.set(make);
    let c = classes();
    ViewFactory {
        bundle: &c.bundle,
        class: &c.factory_name,
    }
}

/// The container's `face` ivar.
///
/// # Safety
/// `view` is a live instance of the container class.
unsafe fn face_slot(view: Obj) -> *mut *mut FaceView {
    // SAFETY: the ivar lies inside the instance (the contract).
    unsafe { view.as_ptr().byte_offset(classes().face).cast() }
}

extern "C" fn interface_version(_this: Obj, _sel: Sel) -> u32 {
    0
}

extern "C" fn ui_view(_this: Obj, _sel: Sel, unit: *mut c_void, _size: CGSize) -> Option<Obj> {
    guard(None, || {
        // SAFETY: AppKit asks for a view on the main thread.
        let main = unsafe { MainThread::assume() };
        let view = MAKE.get()?(unit)?;
        hosted(main, view)
    })
}

/// `view` in a container NSView that owns it, autoreleased (+0, as a
/// method returning an object does); `None` when AppKit refuses the
/// view (the host then shows its generic editor).
fn hosted(_main: MainThread, view: FaceView) -> Option<Obj> {
    let (w, h) = view.size();
    // SAFETY: `alloc`/`initWithFrame:` on an NSView subclass, on the main
    // thread; the +1 is handed to `autorelease` (or released on refusal).
    let container = unsafe {
        let raw: Option<Obj> = send(classes().container.0, sel!(c"alloc"), ());
        send::<_, Option<Obj>>(raw?, sel!(c"initWithFrame:"), (CGRect::new(0.0, 0.0, w, h),))?
    };
    let mut view = Box::new(view);
    // SAFETY: `container` is a live NSView, on the main thread.
    if unsafe { view.attach(container.as_ptr()) }.is_err() {
        drop(view);
        // SAFETY: the one reference `alloc` gave us; the slot is null,
        // so `dealloc` has nothing to drop.
        let _: () = unsafe { send(container, sel!(c"release"), ()) };
        return None;
    }
    // SAFETY: a fresh container; `dealloc` takes the box back.
    unsafe { *face_slot(container) = Box::into_raw(view) };
    // SAFETY: an object we own one reference to.
    let _: Option<Obj> = unsafe { send(container, sel!(c"autorelease"), ()) };
    Some(container)
}

extern "C" fn dealloc(this: Obj, _sel: Sel) {
    guard((), || {
        // SAFETY: `this` is a container being deallocated (on the main
        // thread, where AppKit releases its views); the slot holds the
        // box `hosted` leaked, or null.
        let face = unsafe { core::mem::replace(&mut *face_slot(this), core::ptr::null_mut()) };
        if !face.is_null() {
            // SAFETY: the box `hosted` leaked; dropping it detaches the
            // face's view and stops its timer.
            drop(unsafe { Box::from_raw(face) });
        }
    });
    // SAFETY: `this` is being deallocated and is a direct NSView subclass.
    unsafe { super_dealloc(this, class(c"NSView")) };
}
