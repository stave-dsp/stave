//! `kAudioUnitProperty_CocoaUI`: where the host finds the face. The
//! answer names the view-factory class stave-ui-cocoa registers and the
//! bundle Foundation attributes it to; the factory, asked for a view of
//! a unit, finds the instance through a private property and shows its face over
//! the instance's controller (the same `FaceView` every format shows).

use core::ffi::c_void;

use stave_ui_cocoa::FaceView;

use super::properties::{ANCHOR, INSTANCE_PROPERTY};
use super::*;

/// The view info (bundle URL and class name, both the caller's to
/// release).
pub(crate) fn cocoa_view_info<P: AuExport>() -> Option<sys::AudioUnitCocoaViewInfo> {
    let found = stave_ui_cocoa::view_factory(make_view::<P>);
    let url = cf::directory_url(found.bundle.as_bytes())?;
    let name = cf::string(found.class.to_str().ok()?);
    Some(sys::AudioUnitCocoaViewInfo {
        mCocoaAUViewBundleLocation: url.into_raw(),
        mCocoaAUViewClass: [name.into_raw()],
    })
}

/// The face of the unit the host names, over its instance's controller.
fn make_view<P: AuExport>(unit: *mut c_void) -> Option<FaceView> {
    if unit.is_null() {
        return None;
    }
    let mut found = [0usize; 2];
    let mut size = core::mem::size_of_val(&found) as u32;
    // SAFETY: a unit the host owns; the private property's payload is
    // two pointer-sized words, the size passed.
    let status = unsafe {
        sys::AudioUnitGetProperty(unit, INSTANCE_PROPERTY, sys::kAudioUnitScope_Global, 0, found.as_mut_ptr().cast(), &mut size)
    };
    // Another binary's instance answers with another anchor.
    if status != sys::noErr || found[0] != core::ptr::from_ref(&ANCHOR) as usize || found[1] == 0 {
        return None;
    }
    // SAFETY: this binary's anchor came back, so the pointer is an
    // `Instance<P>` this binary's factory made (one plugin type per
    // binary), alive while the host holds its unit.
    let inst = unsafe { &*(found[1] as *const Instance<P>) };
    let face = inst.face.clone()?;
    Some(FaceView::new(face, Box::new(params::controller_of(inst))))
}
