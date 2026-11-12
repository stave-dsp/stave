//! Host `IParameterChanges` / `IParamValueQueue` — how controller
//! edits reach the processor of a split-architecture plugin
//! (`setParamNormalized` alone only moves the controller's copy; the
//! processor hears changes via `ProcessData.inputParameterChanges`).
//!
//! Allocation-free on the audio thread: every queue and point slot is
//! reserved at load time, and `clear`/`push` only move lengths. A push
//! past capacity is refused and reported, never silently dropped.

use std::ffi::c_void;
use std::ptr;

use stave_vst3::sys::{
    IParamValueQueue, IParamValueQueueVtbl, IParameterChanges, IParameterChangesVtbl, ParamId,
    ParamValue, Tresult, Tuid, K_NO_INTERFACE, K_RESULT_FALSE, K_RESULT_OK,
};

/// Points one parameter can carry per block.
pub(crate) const MAX_POINTS: usize = 16;

#[repr(C)]
struct ParamQueue {
    vtbl: *const IParamValueQueueVtbl,
    id: ParamId,
    len: usize,
    points: [(i32, ParamValue); MAX_POINTS],
}

impl ParamQueue {
    /// Append a point; its index, or `None` when full.
    fn add(&mut self, offset: i32, value: ParamValue) -> Option<usize> {
        let slot = self.points.get_mut(self.len)?;
        *slot = (offset, value);
        self.len += 1;
        Some(self.len - 1)
    }
}

/// One block's parameter changes, with stable addresses (the plugin
/// holds queue pointers for the duration of `process`).
#[repr(C)]
pub(crate) struct ParamChanges {
    vtbl: *const IParameterChangesVtbl,
    len: usize,
    queues: Box<[ParamQueue]>,
}

impl ParamChanges {
    /// Room for `capacity` distinct parameters per block.
    pub fn with_capacity(capacity: usize) -> Box<Self> {
        let queues = (0..capacity.max(1))
            .map(|_| ParamQueue {
                vtbl: &QUEUE_VTBL,
                id: 0,
                len: 0,
                points: [(0, 0.0); MAX_POINTS],
            })
            .collect();
        Box::new(Self {
            vtbl: &CHANGES_VTBL,
            len: 0,
            queues,
        })
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Queue `value` for `id` at `offset`. `false` when out of room.
    pub fn push(&mut self, id: ParamId, offset: i32, value: ParamValue) -> bool {
        self.slot_for(id)
            .is_some_and(|i| self.queues[i].add(offset, value).is_some())
    }

    /// The queue for `id`, opening a fresh one if there's room.
    fn slot_for(&mut self, id: ParamId) -> Option<usize> {
        if let Some(i) = self.queues[..self.len].iter().position(|q| q.id == id) {
            return Some(i);
        }
        let q = self.queues.get_mut(self.len)?;
        q.id = id;
        q.len = 0;
        self.len += 1;
        Some(self.len - 1)
    }

    pub fn as_raw(&mut self) -> *mut IParameterChanges {
        (self as *mut Self).cast()
    }
}

/// # Safety
/// `this` is a `ParamChanges` this module handed to `process`, with no
/// other reference to it in use (the plugin calls it synchronously).
unsafe fn changes<'a>(this: *mut IParameterChanges) -> &'a mut ParamChanges {
    // SAFETY: this function's contract.
    unsafe { &mut *(this as *mut ParamChanges) }
}

/// # Safety
/// As [`changes`], for a queue `c_get`/`c_add` handed out.
unsafe fn queue<'a>(this: *mut IParamValueQueue) -> &'a mut ParamQueue {
    // SAFETY: this function's contract.
    unsafe { &mut *(this as *mut ParamQueue) }
}

// The callbacks below are called by the plugin with an object this module
// handed out (`this`), under the SDK's contract for their other pointers.

unsafe extern "C" fn no_qi<T>(_this: *mut T, _iid: *const Tuid, obj: *mut *mut c_void) -> Tresult {
    // Borrowed for one process call: nothing to hand out a reference to.
    // SAFETY: SDK: null or writable.
    if let Some(obj) = unsafe { obj.as_mut() } {
        *obj = ptr::null_mut();
    }
    K_NO_INTERFACE
}
unsafe extern "C" fn static_ref<T>(_this: *mut T) -> u32 {
    1
}

static QUEUE_VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
    query_interface: no_qi::<IParamValueQueue>,
    add_ref: static_ref::<IParamValueQueue>,
    release: static_ref::<IParamValueQueue>,
    get_parameter_id: q_id,
    get_point_count: q_count,
    get_point: q_get,
    add_point: q_add,
};

unsafe extern "C" fn q_id(this: *mut IParamValueQueue) -> ParamId {
    // SAFETY: `this` is ours (see above).
    unsafe { queue(this) }.id
}
unsafe extern "C" fn q_count(this: *mut IParamValueQueue) -> i32 {
    // SAFETY: `this` is ours (see above).
    unsafe { queue(this) }.len as i32
}
unsafe extern "C" fn q_get(
    this: *mut IParamValueQueue,
    index: i32,
    sample_offset: *mut i32,
    value: *mut ParamValue,
) -> Tresult {
    // SAFETY: `this` is ours (see above).
    let q = unsafe { queue(this) };
    let Some(&(off, v)) = usize::try_from(index)
        .ok()
        .and_then(|i| q.points[..q.len].get(i))
    else {
        return K_RESULT_FALSE;
    };
    // SAFETY: SDK: each null or writable.
    let (offset_out, value_out) = unsafe { (sample_offset.as_mut(), value.as_mut()) };
    if let Some(out) = offset_out {
        *out = off;
    }
    if let Some(out) = value_out {
        *out = v;
    }
    K_RESULT_OK
}
unsafe extern "C" fn q_add(
    this: *mut IParamValueQueue,
    sample_offset: i32,
    value: ParamValue,
    index: *mut i32,
) -> Tresult {
    // SAFETY: `this` is ours (see above).
    let Some(i) = unsafe { queue(this) }.add(sample_offset, value) else {
        return K_RESULT_FALSE;
    };
    // SAFETY: SDK: null or writable.
    if let Some(out) = unsafe { index.as_mut() } {
        *out = i as i32;
    }
    K_RESULT_OK
}

static CHANGES_VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
    query_interface: no_qi::<IParameterChanges>,
    add_ref: static_ref::<IParameterChanges>,
    release: static_ref::<IParameterChanges>,
    get_parameter_count: c_count,
    get_parameter_data: c_get,
    add_parameter_data: c_add,
};

unsafe extern "C" fn c_count(this: *mut IParameterChanges) -> i32 {
    // SAFETY: `this` is ours (see above).
    unsafe { changes(this) }.len as i32
}
unsafe extern "C" fn c_get(this: *mut IParameterChanges, index: i32) -> *mut IParamValueQueue {
    // SAFETY: `this` is ours (see above).
    let c = unsafe { changes(this) };
    match usize::try_from(index) {
        Ok(i) if i < c.len => (&mut c.queues[i] as *mut ParamQueue).cast(),
        _ => ptr::null_mut(),
    }
}
unsafe extern "C" fn c_add(
    this: *mut IParameterChanges,
    id: *const ParamId,
    index: *mut i32,
) -> *mut IParamValueQueue {
    // SAFETY: SDK: null or readable; `this` is ours (see above).
    let (Some(&id), c) = (unsafe { id.as_ref() }, unsafe { changes(this) }) else {
        return ptr::null_mut();
    };
    let Some(slot) = c.slot_for(id) else {
        return ptr::null_mut();
    };
    // SAFETY: SDK: null or writable.
    if let Some(out) = unsafe { index.as_mut() } {
        *out = slot as i32;
    }
    (&mut c.queues[slot] as *mut ParamQueue).cast()
}

// Every `unsafe` block in these tests plays the plugin: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    fn read_all(c: &mut ParamChanges) -> Vec<(ParamId, Vec<(i32, f64)>)> {
        let raw = c.as_raw();
        let vt = unsafe { &**(raw as *const *const IParameterChangesVtbl) };
        let mut out = vec![];
        for i in 0..unsafe { (vt.get_parameter_count)(raw) } {
            let q = unsafe { (vt.get_parameter_data)(raw, i) };
            let qv = unsafe { &**(q as *const *const IParamValueQueueVtbl) };
            let mut pts = vec![];
            for k in 0..unsafe { (qv.get_point_count)(q) } {
                let (mut off, mut v) = (0, 0.0);
                unsafe { (qv.get_point)(q, k, &mut off, &mut v) };
                pts.push((off, v));
            }
            out.push((unsafe { (qv.get_parameter_id)(q) }, pts));
        }
        out
    }

    #[test]
    fn groups_points_by_parameter() {
        let mut c = ParamChanges::with_capacity(4);
        assert!(c.push(7, 0, 0.25));
        assert!(c.push(9, 0, 1.0));
        assert!(c.push(7, 32, 0.5));
        assert_eq!(
            read_all(&mut c),
            vec![(7, vec![(0, 0.25), (32, 0.5)]), (9, vec![(0, 1.0)])]
        );
    }

    #[test]
    fn clear_reuses_slots_without_stale_points() {
        let mut c = ParamChanges::with_capacity(1);
        c.push(1, 0, 0.1);
        c.clear();
        assert!(c.is_empty());
        c.push(2, 0, 0.2);
        assert_eq!(read_all(&mut c), vec![(2, vec![(0, 0.2)])]);
    }

    #[test]
    fn refuses_past_capacity() {
        let mut c = ParamChanges::with_capacity(1);
        assert!(c.push(1, 0, 0.0));
        assert!(!c.push(2, 0, 0.0), "second parameter has no slot");
        for k in 1..MAX_POINTS {
            assert!(c.push(1, k as i32, 0.0));
        }
        assert!(!c.push(1, 99, 0.0), "point slots are bounded too");
    }

    #[test]
    fn out_of_range_reads_are_refused() {
        let mut c = ParamChanges::with_capacity(2);
        c.push(3, 0, 0.5);
        let raw = c.as_raw();
        let vt = unsafe { &**(raw as *const *const IParameterChangesVtbl) };
        assert!(unsafe { (vt.get_parameter_data)(raw, 1) }.is_null());
        assert!(unsafe { (vt.get_parameter_data)(raw, -1) }.is_null());
        let q = unsafe { (vt.get_parameter_data)(raw, 0) };
        let qv = unsafe { &**(q as *const *const IParamValueQueueVtbl) };
        assert_eq!(
            unsafe { (qv.get_point)(q, 5, ptr::null_mut(), ptr::null_mut()) },
            K_RESULT_FALSE
        );
    }
}
