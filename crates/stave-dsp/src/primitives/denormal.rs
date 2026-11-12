//! Denormal handling. Subnormal floats are catastrophically slow on x86 —
//! filter feedback loops can sit on tiny tails that take 100x longer to
//! process. Flush small values to zero in filter state to avoid this.

/// If `|x|` is below this, treat as zero. about −400 dB — far below any audible or measurable floor — well below
/// audible.
pub const DENORMAL_FLOOR: f32 = 1.0e-20;

#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < DENORMAL_FLOOR {
        0.0
    } else {
        x
    }
}

/// In-place flush over a slice.
#[inline]
pub fn flush_slice(buf: &mut [f32]) {
    for s in buf.iter_mut() {
        *s = flush(*s);
    }
}

/// f64 twin, for the f64-state LF one-poles: their whole point
/// is precision near unity poles, so they keep their own state class.
/// 1e-30 ≈ −600 dB — far below the f64 recursion's own error floor.
#[inline]
pub fn flush64(x: f64) -> f64 {
    if x.abs() < 1.0e-30 {
        0.0
    } else {
        x
    }
}
