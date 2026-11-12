//! `IConnectionPoint` — the component <-> controller wiring the VST3
//! spec asks hosts to make for split-architecture plugins (some
//! controllers stay inert without a peer).

use stave_vst3_sys::{IConnectionPointVtbl, IID_ICONNECTION_POINT, K_RESULT_OK};

use super::Com;

/// Two connection points wired to each other; dropping disconnects both
/// (before either object is terminated — the spec's teardown order).
pub(crate) struct Connection {
    a: Com<IConnectionPointVtbl>,
    b: Com<IConnectionPointVtbl>,
}

impl Connection {
    /// Wire `a` and `b` together if both expose `IConnectionPoint`.
    /// `None` when either doesn't (the plugin talks some other way) or
    /// a `connect` call is refused.
    pub fn wire<A, B>(a: &Com<A>, b: &Com<B>) -> Option<Self> {
        // SAFETY: IConnectionPointVtbl is the layout of IID_ICONNECTION_POINT.
        let a = unsafe { a.query::<IConnectionPointVtbl>(&IID_ICONNECTION_POINT)? };
        // SAFETY: as above.
        let b = unsafe { b.query::<IConnectionPointVtbl>(&IID_ICONNECTION_POINT)? };
        // SAFETY: both are live connection points we hold references to.
        let ab = unsafe { (a.vtbl().connect)(a.as_ptr(), b.as_ptr()) };
        if ab != K_RESULT_OK {
            return None;
        }
        // SAFETY: as above.
        let ba = unsafe { (b.vtbl().connect)(b.as_ptr(), a.as_ptr()) };
        if ba != K_RESULT_OK {
            // SAFETY: as above; undoes the one-way connection.
            unsafe { (a.vtbl().disconnect)(a.as_ptr(), b.as_ptr()) };
            return None;
        }
        Some(Self { a, b })
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: wired in `wire`; both references still held.
        unsafe {
            (self.a.vtbl().disconnect)(self.a.as_ptr(), self.b.as_ptr());
            (self.b.vtbl().disconnect)(self.b.as_ptr(), self.a.as_ptr());
        }
    }
}
