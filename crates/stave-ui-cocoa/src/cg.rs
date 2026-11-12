//! CoreGraphics image plumbing shared by the pixel-rendered
//! components (spectrum strip, transfer plots) — Step 7.2 pure move
//! from view.rs.

use core::ffi::c_void;
use core::ptr;

extern crate alloc;

use super::*;

/// Release callback for CGDataProvider: reconstructs the Box<[u8]> we
/// leaked into the provider and drops it. Signature matches
/// `CGDataProviderReleaseDataCallback` (info, data, size).
pub(crate) unsafe extern "C" fn cg_provider_release_box(
    _info: *mut c_void,
    data: *const u8,
    size: usize,
) {
    if data.is_null() || size == 0 {
        return;
    }
    // Reclaiming a Box we leaked to the CGDataProvider; the allocation
    // was mutable at birth, CF just carries it as *const.
    let slice_ptr = core::ptr::slice_from_raw_parts_mut(data.cast_mut(), size);
    drop(unsafe { alloc::boxed::Box::from_raw(slice_ptr) });
}

/// Build a CGImage from RGBA8 pixels and assign as the NSImageView's
/// image. `pixel_w`/`pixel_h` are the raw buffer dimensions; `point_w`
/// /`point_h` are the logical size NSImage should report. Passing
/// pixel = 2× point gives a Retina-quality image that displays crisp
/// on @2x screens.
pub(crate) unsafe fn set_image_view_pixels(
    view: Id,
    pixels: &[u8],
    pixel_w: usize,
    pixel_h: usize,
    point_w: f64,
    point_h: f64,
) {
    let w = pixel_w;
    let h = pixel_h;
    // CGColorSpaceCreateDeviceRGB → CGDataProviderCreateWithData →
    // CGImageCreate → NSImage initWithCGImage:size: → NSImageView
    // setImage:.
    extern "C" {
        pub(crate) fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        pub(crate) fn CGColorSpaceRelease(cs: *mut c_void);
        pub(crate) fn CGDataProviderCreateWithData(
            info: *mut c_void,
            data: *const u8,
            size: usize,
            release: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize)>,
        ) -> *mut c_void;
        pub(crate) fn CGDataProviderRelease(p: *mut c_void);
        pub(crate) fn CGImageCreate(
            width: usize,
            height: usize,
            bits_per_component: usize,
            bits_per_pixel: usize,
            bytes_per_row: usize,
            colorspace: *mut c_void,
            bitmap_info: u32,
            provider: *mut c_void,
            decode: *const f32,
            should_interpolate: bool,
            intent: i32,
        ) -> *mut c_void;
        pub(crate) fn CGImageRelease(image: *mut c_void);
    }
    let cs = unsafe { CGColorSpaceCreateDeviceRGB() };
    // Hand ownership of an owned copy to the CGDataProvider so the
    // CGImage's backing memory outlives this function. Released via
    // `cg_provider_release_box` when CG decides it's done with the image.
    let owned: alloc::boxed::Box<[u8]> = pixels.to_vec().into_boxed_slice();
    let len = owned.len();
    let owned_ptr = alloc::boxed::Box::into_raw(owned) as *mut u8;
    let provider = unsafe {
        CGDataProviderCreateWithData(
            owned_ptr as *mut c_void,
            owned_ptr,
            len,
            Some(cg_provider_release_box),
        )
    };
    // kCGImageAlphaPremultipliedLast = 1, kCGBitmapByteOrderDefault = 0.
    let bitmap_info: u32 = 1;
    let image = unsafe {
        CGImageCreate(
            w,
            h,
            8,
            32,
            w * 4,
            cs,
            bitmap_info,
            provider,
            ptr::null(),
            false,
            0,
        )
    };
    // NSImage initWithCGImage:size:
    let ns_image_class = class(c"NSImage");
    let alloc = unsafe { msg_id(ns_image_class, sel(c"alloc")) };
    let init_with_cg: unsafe extern "C" fn(Id, Sel, *mut c_void, CGSize) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    let ns_image = unsafe {
        init_with_cg(
            alloc,
            sel(c"initWithCGImage:size:"),
            image,
            CGSize {
                width: point_w,
                height: point_h,
            },
        )
    };
    unsafe { msg_void_id(view, sel(c"setImage:"), ns_image) };
    unsafe {
        CGImageRelease(image);
        CGDataProviderRelease(provider);
        CGColorSpaceRelease(cs);
    }
}
