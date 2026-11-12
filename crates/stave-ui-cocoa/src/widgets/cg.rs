// The CoreGraphics FFI surface, shared by the scene renderer and
// the widgets that draw directly.

use core::ffi::c_void;

use crate::{CGPoint, CGRect};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub(crate) fn CGPathCreateMutable() -> *mut c_void;
    pub(crate) fn CGPathMoveToPoint(path: *mut c_void, transform: *const c_void, x: f64, y: f64);
    pub(crate) fn CGPathAddLineToPoint(path: *mut c_void, transform: *const c_void, x: f64, y: f64);
    pub(crate) fn CGPathCloseSubpath(path: *mut c_void);
    pub(crate) fn CGPathRelease(path: *mut c_void);
    pub(crate) fn CGContextSetRGBFillColor(c: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub(crate) fn CGContextSetRGBStrokeColor(c: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub(crate) fn CGContextFillEllipseInRect(c: *mut c_void, rect: CGRect);
    pub(crate) fn CGContextStrokeEllipseInRect(c: *mut c_void, rect: CGRect);
    pub(crate) fn CGContextFillRect(c: *mut c_void, rect: CGRect);
    pub(crate) fn CGContextSetLineWidth(c: *mut c_void, w: f64);
    pub(crate) fn CGContextSetLineCap(c: *mut c_void, cap: i32);
    pub(crate) fn CGContextSetBlendMode(c: *mut c_void, mode: i32);
    pub(crate) fn CGContextClipToRect(c: *mut c_void, rect: CGRect);
    pub(crate) fn CGContextAddArcToPoint(
        c: *mut c_void,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        r: f64,
    );
    pub(crate) fn CGContextClosePath(c: *mut c_void);
    pub(crate) fn CGContextFillPath(c: *mut c_void);
    pub(crate) fn CGContextBeginPath(c: *mut c_void);
    pub(crate) fn CGContextAddArc(
        c: *mut c_void,
        x: f64,
        y: f64,
        r: f64,
        a1: f64,
        a2: f64,
        cw: i32,
    );
    pub(crate) fn CGContextMoveToPoint(c: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGContextAddLineToPoint(c: *mut c_void, x: f64, y: f64);
    pub(crate) fn CGContextStrokePath(c: *mut c_void);
    pub(crate) fn CGContextSaveGState(c: *mut c_void);
    pub(crate) fn CGContextRestoreGState(c: *mut c_void);
    pub(crate) fn CGContextClip(c: *mut c_void);
    pub(crate) fn CGContextDrawLinearGradient(
        c: *mut c_void,
        g: *mut c_void,
        start: CGPoint,
        end: CGPoint,
        options: u32,
    );
    pub(crate) fn CGGradientCreateWithColorComponents(
        space: *mut c_void,
        components: *const f64,
        locations: *const f64,
        count: usize,
    ) -> *mut c_void;
    pub(crate) fn CGGradientRelease(g: *mut c_void);
    pub(crate) fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    pub(crate) fn CGColorSpaceRelease(cs: *mut c_void);
}
