//! Latency & tail propagation as PURE arithmetic
//! The executor owns compensation; this
//! module owns the invariants it computes with, snapshot-tested headless:
//!
//!  - serial paths SUM latency,
//!  - parallel paths take the MAX and pad the faster branches
//!    (the pads are what the executor turns into compensation delays
//!    — `stave_dsp::Compensator`),
//!  - a rate region reports its inner latency at the OUTER rate
//!    (inner samples divide by the region factor; the half-band
//!    constructions guarantee integer group delay, and the invariant
//!    asserts it rather than silently rounding a value that integer
//!    delay lines cannot compensate).
//!
//! Tails follow the same shapes with `Infinite` absorbing, except a
//! region tail rounds UP — a tail bound may be conservative, a
//! latency report may not.

use crate::plugin::Tail;

/// Serial composition: latencies sum.
pub fn serial(paths: &[u32]) -> u32 {
    paths.iter().sum()
}

/// Parallel composition of `N` branches: the join's latency is the
/// slowest branch; `pad[i]` is the compensation delay the executor
/// inserts on branch `i` so every branch arrives together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Balanced<const N: usize> {
    pub latency: u32,
    pub pad: [u32; N],
}

pub fn parallel<const N: usize>(paths: [u32; N]) -> Balanced<N> {
    let latency = paths.iter().copied().max().unwrap_or(0);
    let mut pad = [0u32; N];
    let mut i = 0;
    while i < N {
        pad[i] = latency - paths[i];
        i += 1;
    }
    Balanced { latency, pad }
}

/// A rate region's inner-rate latency, reported at the outer rate.
/// Panics (debug) on non-integer group delay: that is a design error
/// in the region's filters, not a value to round.
pub fn region(inner_samples: u32, factor: u32) -> u32 {
    debug_assert!(
        factor > 0 && inner_samples % factor == 0,
        "region group delay must be integer at the outer rate ({inner_samples}/{factor})"
    );
    inner_samples / factor
}

/// Serial tail: upstream ringing still traverses everything after it.
pub fn tail_serial(a: Tail, b: Tail) -> Tail {
    match (a, b) {
        (Tail::Infinite, _) | (_, Tail::Infinite) => Tail::Infinite,
        (Tail::None, t) | (t, Tail::None) => t,
        (Tail::Samples(x), Tail::Samples(y)) => Tail::Samples(x + y),
    }
}

/// Parallel tail: the join rings as long as its longest branch.
pub fn tail_parallel(a: Tail, b: Tail) -> Tail {
    match (a, b) {
        (Tail::Infinite, _) | (_, Tail::Infinite) => Tail::Infinite,
        (Tail::None, t) | (t, Tail::None) => t,
        (Tail::Samples(x), Tail::Samples(y)) => Tail::Samples(x.max(y)),
    }
}

/// A region's inner-rate tail at the outer rate — rounds UP (a tail
/// bound may be conservative).
pub fn tail_region(t: Tail, factor: u32) -> Tail {
    match t {
        Tail::Samples(x) => Tail::Samples(x.div_ceil(factor)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_sums() {
        assert_eq!(serial(&[]), 0);
        assert_eq!(serial(&[66, 18]), 84);
        assert_eq!(serial(&[1, 2, 3]), 6);
    }

    #[test]
    fn parallel_maxes_and_pads() {
        // Dry/wet: dry (0) against a wet path (84) → the join reports
        // 84 and the dry branch is padded by 84 — exactly the delay
        // Forge and the multiband saturation path hand-rolled until now.
        assert_eq!(
            parallel([0, 84]),
            Balanced {
                latency: 84,
                pad: [84, 0]
            }
        );
        assert_eq!(parallel([12, 12, 12]).latency, 12);
        assert_eq!(parallel([3, 7, 5]).pad, [4, 0, 2]);
    }

    #[test]
    fn nested_regions_divide_per_level() {
        // The forge 4x stack: front half-band = 132 inner samples at
        // 2x → 66 outer; nested inner half-band = 72 samples at 4x =
        // 36 at 2x → 18 outer. Serial at the outer rate: 84.
        let front = region(132, 2);
        let inner = region(region(72, 2), 2);
        assert_eq!((front, inner), (66, 18));
        assert_eq!(serial(&[front, inner]), 84);
    }

    #[test]
    #[should_panic(expected = "integer")]
    #[cfg(debug_assertions)]
    fn region_rejects_fractional_group_delay() {
        region(3, 2);
    }

    #[test]
    fn tails_combine() {
        use Tail::*;
        assert_eq!(tail_serial(Samples(100), Samples(50)), Samples(150));
        assert_eq!(tail_serial(None, Samples(50)), Samples(50));
        assert_eq!(tail_serial(Infinite, Samples(50)), Infinite);
        assert_eq!(tail_parallel(Samples(100), Samples(50)), Samples(100));
        assert_eq!(tail_parallel(None, None), None);
        assert_eq!(tail_parallel(Infinite, None), Infinite);
        assert_eq!(tail_region(Samples(85), 4), Samples(22), "rounds up");
        assert_eq!(tail_region(Infinite, 4), Infinite);
    }
}
