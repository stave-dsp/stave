//! Denormal handling. Subnormal floats are catastrophically slow on x86 —
//! filter feedback loops can sit on tiny tails that take 100x longer to
//! process. Flush small values to zero in filter state to avoid this.
//!
//! The same flush also maps NaN/±inf to zero: every call site is
//! recursive state, where one non-finite sample would otherwise poison
//! the state forever (until `reset`). Finite values are untouched
//! bit-for-bit — the check is the same two compares, arranged so a NaN
//! fails them.

/// If `|x|` is below this, treat as zero. about −400 dB — far below any audible or measurable floor — well below
/// audible.
pub const DENORMAL_FLOOR: f32 = 1.0e-20;

/// `0.0` for tiny (`|x| < DENORMAL_FLOOR`) or non-finite `x`, else `x`.
#[inline]
pub fn flush(x: f32) -> f32 {
    let a = x.abs();
    // NaN fails both compares; ±inf fails the second.
    if (DENORMAL_FLOOR..=f32::MAX).contains(&a) {
        x
    } else {
        0.0
    }
}

/// `x` if finite, else `0.0` — the guard for recursive paths that must
/// stay bit-exact for tiny finite values (Loop/Chain feedback), where
/// [`flush`]'s denormal floor would change results.
#[inline]
pub fn finite_or_zero(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        0.0
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
/// Non-finite maps to `0.0`, as in [`flush`].
#[inline]
pub fn flush64(x: f64) -> f64 {
    let a = x.abs();
    if (1.0e-30..=f64::MAX).contains(&a) {
        x
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_zeroes_tiny_and_non_finite_and_keeps_the_rest_bit_exact() {
        for x in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            1e-30,
            -1e-25,
            0.0,
        ] {
            assert_eq!(flush(x).to_bits(), 0.0f32.to_bits(), "{x}");
        }
        for x in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e-31,
            -1e-35,
            0.0,
        ] {
            assert_eq!(flush64(x).to_bits(), 0.0f64.to_bits(), "{x}");
        }
        for x in [
            DENORMAL_FLOOR,
            -DENORMAL_FLOOR,
            1.0,
            -0.5,
            f32::MAX,
            f32::MIN,
            1e-19,
        ] {
            assert_eq!(flush(x).to_bits(), x.to_bits(), "{x}");
        }
        for x in [1.0e-30f64, 1.0, f64::MAX, f64::MIN] {
            assert_eq!(flush64(x).to_bits(), x.to_bits(), "{x}");
        }
        assert_eq!(finite_or_zero(f32::NAN), 0.0);
        assert_eq!(finite_or_zero(1e-40).to_bits(), 1e-40f32.to_bits());
    }
}
