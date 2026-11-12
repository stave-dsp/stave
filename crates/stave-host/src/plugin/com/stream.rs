//! `MemStream` — a host-side `IBStream` over a `Vec<u8>`: the stream
//! `getState`/`setState`/`setComponentState` read and write. The plugin
//! sees a refcounted, spec-shaped stream; the host holds a
//! [`StreamHandle`] and sees plain bytes.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU32, Ordering};

use stave_vst3_sys::{
    FUnknown, IBStream, IBStreamVtbl, Tresult, Tuid, IID_FUNKNOWN, IID_IBSTREAM,
    K_INVALID_ARGUMENT, K_NO_INTERFACE, K_RESULT_OK, SEEK_CUR, SEEK_END, SEEK_SET,
};

#[repr(C)]
struct MemStream {
    vtbl: *const IBStreamVtbl,
    refs: AtomicU32,
    data: Vec<u8>,
    pos: usize,
}

static VTBL: IBStreamVtbl = IBStreamVtbl {
    query_interface: qi,
    add_ref,
    release,
    read,
    write,
    seek,
    tell,
};

/// The host's reference to a [`MemStream`]. Dropping it releases that
/// reference; the plugin may hold its own (refcounted) for longer.
pub(crate) struct StreamHandle(NonNull<MemStream>);

impl StreamHandle {
    /// A fresh stream positioned at 0 over `data`.
    pub fn new(data: Vec<u8>) -> Self {
        let boxed = Box::new(MemStream {
            vtbl: &VTBL,
            refs: AtomicU32::new(1),
            data,
            pos: 0,
        });
        Self(NonNull::from(Box::leak(boxed)))
    }

    /// The stream as the `FUnknown` the state calls take.
    pub fn as_funknown(&self) -> *mut FUnknown {
        self.0.as_ptr().cast()
    }

    /// Seek back to the start (write, then hand to a reader).
    pub fn rewind(&mut self) {
        // SAFETY: the handle's reference keeps the stream alive; state
        // calls are synchronous, so no plugin call is in flight.
        unsafe { (*self.0.as_ptr()).pos = 0 };
    }

    /// A copy of the bytes written so far.
    pub fn bytes(&self) -> Vec<u8> {
        // SAFETY: as in `rewind`.
        unsafe { (*self.0.as_ptr()).data.clone() }
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        // SAFETY: the handle owns one reference.
        unsafe { release(self.0.as_ptr().cast()) };
    }
}

/// # Safety
/// `this` is a live `MemStream` this module handed out, and no other
/// reference to it is in use (the plugin calls a stream synchronously
/// from one thread).
unsafe fn stream<'a>(this: *mut IBStream) -> &'a mut MemStream {
    // SAFETY: this function's contract.
    unsafe { &mut *(this as *mut MemStream) }
}

// The callbacks below are called by the plugin with a stream this module
// handed out (`this`), under the SDK's contract for their other pointers.

unsafe extern "C" fn qi(this: *mut IBStream, iid: *const Tuid, obj: *mut *mut c_void) -> Tresult {
    // SAFETY: SDK: null or valid.
    let (Some(iid), Some(obj)) = (unsafe { iid.as_ref() }, unsafe { obj.as_mut() }) else {
        return K_INVALID_ARGUMENT;
    };
    if *iid == IID_FUNKNOWN || *iid == IID_IBSTREAM {
        // SAFETY: `this` is ours (see above).
        unsafe { add_ref(this) };
        *obj = this.cast();
        K_RESULT_OK
    } else {
        *obj = ptr::null_mut();
        K_NO_INTERFACE
    }
}

unsafe extern "C" fn add_ref(this: *mut IBStream) -> u32 {
    // SAFETY: `this` is ours (see above).
    unsafe { stream(this) }.refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "C" fn release(this: *mut IBStream) -> u32 {
    // SAFETY: `this` is ours (see above).
    let left = unsafe { stream(this) }.refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        // SAFETY: the last reference; `StreamHandle::new` leaked the box.
        drop(unsafe { Box::from_raw(this as *mut MemStream) });
    }
    left
}

unsafe extern "C" fn read(
    this: *mut IBStream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_read: *mut i32,
) -> Tresult {
    // SAFETY: `this` is ours (see above).
    let s = unsafe { stream(this) };
    let avail = s.data.get(s.pos..).unwrap_or_default();
    let n = avail.len().min(num_bytes.max(0) as usize);
    if n > 0 {
        if buffer.is_null() {
            return K_INVALID_ARGUMENT;
        }
        // SAFETY: SDK: `buffer` is writable for `num_bytes` ≥ `n`; our
        // `data` is a separate allocation.
        unsafe { ptr::copy_nonoverlapping(avail.as_ptr(), buffer.cast(), n) };
    }
    s.pos += n;
    // SAFETY: SDK: null or writable.
    if let Some(out) = unsafe { num_read.as_mut() } {
        *out = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "C" fn write(
    this: *mut IBStream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_written: *mut i32,
) -> Tresult {
    // SAFETY: `this` is ours (see above).
    let s = unsafe { stream(this) };
    let n = num_bytes.max(0) as usize;
    if n > 0 {
        if buffer.is_null() {
            return K_INVALID_ARGUMENT;
        }
        // SAFETY: SDK: `buffer` is readable for `num_bytes`.
        let src = unsafe { std::slice::from_raw_parts(buffer as *const u8, n) };
        let end = s.pos + n;
        if end > s.data.len() {
            s.data.resize(end, 0);
        }
        s.data[s.pos..end].copy_from_slice(src);
        s.pos = end;
    }
    // SAFETY: SDK: null or writable.
    if let Some(out) = unsafe { num_written.as_mut() } {
        *out = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "C" fn seek(this: *mut IBStream, pos: i64, mode: i32, result: *mut i64) -> Tresult {
    // SAFETY: `this` is ours (see above).
    let s = unsafe { stream(this) };
    let target = match mode {
        SEEK_SET => pos,
        SEEK_CUR => s.pos as i64 + pos,
        SEEK_END => s.data.len() as i64 + pos,
        _ => return K_INVALID_ARGUMENT,
    };
    if target < 0 {
        return K_INVALID_ARGUMENT;
    }
    s.pos = target as usize;
    // SAFETY: SDK: null or writable.
    if let Some(out) = unsafe { result.as_mut() } {
        *out = target;
    }
    K_RESULT_OK
}

unsafe extern "C" fn tell(this: *mut IBStream, pos: *mut i64) -> Tresult {
    // SAFETY: SDK: null or writable; `this` is ours (see above).
    let (Some(out), s) = (unsafe { pos.as_mut() }, unsafe { stream(this) }) else {
        return K_INVALID_ARGUMENT;
    };
    *out = s.pos as i64;
    K_RESULT_OK
}

// Every `unsafe` block in these tests plays the plugin: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    fn vt(h: &StreamHandle) -> (&'static IBStreamVtbl, *mut IBStream) {
        let p = h.as_funknown() as *mut IBStream;
        (unsafe { &**(p as *const *const IBStreamVtbl) }, p)
    }

    #[test]
    fn write_then_read_round_trips() {
        let mut h = StreamHandle::new(Vec::new());
        let (v, p) = vt(&h);
        let mut n = 0;
        let src = *b"hello";
        unsafe { (v.write)(p, src.as_ptr() as *mut c_void, 5, &mut n) };
        assert_eq!(n, 5);
        h.rewind();
        let mut dst = [0u8; 8];
        unsafe { (v.read)(p, dst.as_mut_ptr().cast(), 8, &mut n) };
        assert_eq!(
            (n, &dst[..5]),
            (5, &b"hello"[..]),
            "short read reports what it got"
        );
        assert_eq!(h.bytes(), b"hello");
    }

    #[test]
    fn read_past_end_is_empty_not_ub() {
        let h = StreamHandle::new(vec![1, 2]);
        let (v, p) = vt(&h);
        let mut at = 0;
        unsafe { (v.seek)(p, 10, SEEK_SET, &mut at) };
        assert_eq!(at, 10);
        let (mut dst, mut n) = ([9u8; 4], -1);
        unsafe { (v.read)(p, dst.as_mut_ptr().cast(), 4, &mut n) };
        assert_eq!((n, dst), (0, [9; 4]));
    }

    #[test]
    fn overwrite_in_the_middle_keeps_the_tail() {
        let h = StreamHandle::new(b"abcdef".to_vec());
        let (v, p) = vt(&h);
        unsafe {
            (v.seek)(p, 2, SEEK_SET, ptr::null_mut());
            (v.write)(p, b"XY".as_ptr() as *mut c_void, 2, ptr::null_mut());
        }
        assert_eq!(h.bytes(), b"abXYef");
    }

    #[test]
    fn seek_modes_and_tell() {
        let h = StreamHandle::new(vec![0; 10]);
        let (v, p) = vt(&h);
        let mut at = 0;
        unsafe {
            (v.seek)(p, -3, SEEK_END, &mut at);
            assert_eq!(at, 7);
            (v.seek)(p, 2, SEEK_CUR, &mut at);
            assert_eq!(at, 9);
            assert_eq!((v.seek)(p, -100, SEEK_CUR, &mut at), K_INVALID_ARGUMENT);
            assert_eq!((v.seek)(p, 0, 99, &mut at), K_INVALID_ARGUMENT);
            (v.tell)(p, &mut at);
        }
        assert_eq!(at, 9, "failed seeks leave the position alone");
    }

    #[test]
    fn plugin_reference_outlives_host_handle() {
        let h = StreamHandle::new(b"kept".to_vec());
        let (v, p) = vt(&h);
        let mut obj: *mut c_void = ptr::null_mut();
        assert_eq!(
            unsafe { (v.query_interface)(p, &IID_IBSTREAM, &mut obj) },
            K_RESULT_OK
        );
        drop(h);
        // The plugin's reference is still valid; releasing it frees.
        let mut n = 0;
        let mut dst = [0u8; 4];
        unsafe {
            (v.seek)(obj.cast(), 0, SEEK_SET, ptr::null_mut());
            (v.read)(obj.cast(), dst.as_mut_ptr().cast(), 4, &mut n);
            assert_eq!(&dst, b"kept");
            assert_eq!((v.release)(obj.cast()), 0);
        }
    }

    #[test]
    fn unknown_interface_is_refused() {
        let h = StreamHandle::new(Vec::new());
        let (v, p) = vt(&h);
        let mut obj: *mut c_void = ptr::dangling_mut::<c_void>();
        let r = unsafe { (v.query_interface)(p, &[7; 16], &mut obj) };
        assert_eq!((r, obj), (K_NO_INTERFACE, ptr::null_mut()));
    }
}
