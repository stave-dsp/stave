//! CoreFoundation and CoreGraphics basics: the geometry structs messages
//! pass by value, and [`CfRef`], an owned CF reference.

use core::ffi::c_void;
use core::ptr::NonNull;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CGSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}

impl CGRect {
    pub(crate) fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        CGRect {
            origin: CGPoint { x, y },
            size: CGSize { width, height },
        }
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: NonNull<c_void>);
}

/// One owned reference to a CoreFoundation object (CoreGraphics and
/// CoreText objects are CF objects too), released on drop.
#[derive(Debug)]
pub(crate) struct CfRef(NonNull<c_void>);

impl CfRef {
    /// Take over `p`: `None` when a create function failed (returned
    /// null).
    ///
    /// # Safety
    /// `p` is null or a reference the caller owns (from a `…Create…` or
    /// `…Copy…` function, or retained), handed over.
    pub(crate) unsafe fn from_create<T>(p: *const T) -> Option<CfRef> {
        NonNull::new(p.cast_mut().cast()).map(CfRef)
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for CfRef {
    fn drop(&mut self) {
        // SAFETY: the one reference `from_create` was handed.
        unsafe { CFRelease(self.0) };
    }
}
