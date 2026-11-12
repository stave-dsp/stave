//! Block metrics for the editor's rails and load meter: peak dBFS
//! scanning (read-only). Audio-thread safe throughout — no allocation,
//! no locks.

/// Peak levels in dBFS (−120 floor for silence): all channels, the
/// first, and the second (the first again for mono).
pub(crate) struct Peaks {
    pub all: f64,
    pub left: f64,
    pub right: f64,
}

/// # Safety
/// Every non-null pointer is valid for `frames` reads.
pub(crate) unsafe fn peaks(ptrs: &[*mut f32], frames: usize) -> Peaks {
    let channel = |p: *mut f32| -> f32 {
        if p.is_null() {
            return 0.0;
        }
        // SAFETY: this function's contract.
        let s = unsafe { core::slice::from_raw_parts(p, frames) };
        s.iter().fold(0.0, |pk: f32, v| pk.max(v.abs()))
    };
    let db = |pk: f32| {
        if pk > 1e-6 {
            20.0 * f64::from(pk).log10()
        } else {
            -120.0
        }
    };
    let left = ptrs.first().map_or(0.0, |&p| channel(p));
    let right = ptrs.get(1).map_or(left, |&p| channel(p));
    let rest = ptrs
        .iter()
        .skip(2)
        .fold(0.0f32, |pk, &p| pk.max(channel(p)));
    Peaks {
        all: db(left.max(right).max(rest)),
        left: db(left),
        right: db(right),
    }
}
