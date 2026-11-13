//! The partition schedule: which sizes, how many of each.

use super::Head;

/// One run of a partition schedule: `parts` partitions of `size` taps,
/// starting at tap `offset`, whose first partition multiplies the input
/// block `skip` blocks older than the newest.
///
/// A run of size `L` hears its input one block late (overlap-save) and
/// `skip` more blocks later through its delay line, so its taps sound
/// `L·(1 + skip)` samples after the input; it lines up with the
/// convolver's latency `base` when `offset + base == L·(1 + skip)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) size: usize,
    pub(crate) offset: usize,
    pub(crate) skip: usize,
    pub(crate) parts: usize,
}

/// The modelled cost of one run, per sample: a forward and an inverse
/// real transform of `2L` per lane every `L` samples, `parts·(L + 1)`
/// complex multiply-adds per path, and the per-sample bookkeeping per
/// lane. The weights are relative ns on an Apple M-series core
/// (`tests/datasheet.rs`: the real `Fft` at about 0.3 ns per point per
/// `log₂ n`, a split multiply-add about 0.31 ns per bin); only their
/// ratios steer the schedule.
fn run_cost(size: usize, parts: usize, lanes: usize, paths: usize) -> f64 {
    const TRANSFORM: f64 = 0.30;
    const PRODUCT: f64 = 0.31;
    const BOOKKEEPING: f64 = 1.0;
    if parts == 0 {
        return 0.0;
    }
    let (l, n) = (size as f64, (2 * size) as f64);
    (lanes as f64 * 2.0 * TRANSFORM * n * n.log2() + (paths * parts) as f64 * (l + 1.0) * PRODUCT)
        / l
        + lanes as f64 * BOOKKEEPING
}

/// The most partitions the schedule search tries at one size below the
/// largest: past it, doubling the size is always cheaper under
/// [`run_cost`] (the products alone outweigh the next size's
/// transforms). A bound on the search, not on the schedule: the largest
/// size takes as many partitions as the response needs.
const SEARCH_PARTS: usize = 128;

/// The partition schedule for a response of `ir_len` taps: sizes that
/// double from `block` (Gardner, 1995), at most `largest`, with the
/// count at each size (zero skips it) chosen to minimise [`run_cost`]
/// summed over the runs (García, 2002), by dynamic programming over
/// `(size, alignment)`. `largest == block` is the uniform schedule.
///
/// A size `2L` can start only where it lines up with the latency
/// (`offset + base` a multiple of `2L`), which constrains the count at
/// `L` by parity; the search walks exactly the aligned choices.
///
/// Runs of `defer` samples or more are computed a block ahead, off the
/// audio thread (DSP.md F3b): each must keep one block of slack, `skip ≥
/// 1`, so its next output never needs the input block that has just
/// arrived (Battenberg & Avizienis's deadline of one period). The search
/// treats a slack-less start at such a size as infeasible. `usize::MAX`
/// defers nothing.
pub(crate) fn schedule(
    ir_len: usize,
    block: usize,
    largest: usize,
    head: Head,
    lanes: usize,
    paths: usize,
    defer: usize,
) -> Vec<Span> {
    let base = match head {
        Head::Partitioned => block,
        Head::Direct => 0,
    };
    if block - base >= ir_len {
        return Vec::new();
    }
    let levels = (largest / block).trailing_zeros() as usize + 1;
    // best[k][a]: the cheapest (cost, count) from size `block << k` with
    // `offset + base == a · size`; a count equal to the partitions that
    // fit finishes there.
    let mut best: Vec<Vec<Option<(f64, usize)>>> = vec![vec![None; SEARCH_PARTS + 2]; levels];
    fn solve(
        k: usize,
        a: usize,
        g: &Search,
        best: &mut [Vec<Option<(f64, usize)>>],
    ) -> (f64, usize) {
        if let Some(hit) = best[k][a] {
            return hit;
        }
        let size = g.block << k;
        if size >= g.defer && a < 2 {
            // A deferred run needs `skip = a − 1 ≥ 1`.
            best[k][a] = Some((f64::INFINITY, 0));
            return (f64::INFINITY, 0);
        }
        let offset = a * size - g.base;
        let fits = g.ir_len.saturating_sub(offset).div_ceil(size);
        let mut pick = (run_cost(size, fits, g.lanes, g.paths), fits);
        if k + 1 < best.len() && fits > 0 {
            for parts in (a % 2..fits.min(SEARCH_PARTS)).step_by(2) {
                let rest = solve(k + 1, (a + parts) / 2, g, best).0;
                let cost = run_cost(size, parts, g.lanes, g.paths) + rest;
                if cost < pick.0 {
                    pick = (cost, parts);
                }
            }
        }
        best[k][a] = Some(pick);
        pick
    }
    let search = Search {
        ir_len,
        block,
        base,
        lanes,
        paths,
        defer,
    };
    let mut spans = Vec::new();
    let (mut k, mut a) = (0, 1);
    loop {
        let parts = solve(k, a, &search, &mut best).1;
        let size = block << k;
        let offset = a * size - base;
        if parts > 0 {
            spans.push(Span {
                size,
                offset,
                skip: a - 1,
                parts,
            });
        }
        if offset + parts * size >= ir_len {
            return spans;
        }
        (k, a) = (k + 1, (a + parts) / 2);
    }
}

/// The fixed inputs of a [`schedule`] search.
struct Search {
    ir_len: usize,
    block: usize,
    base: usize,
    lanes: usize,
    paths: usize,
    defer: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every schedule covers the response once, in order, with
    /// power-of-two sizes from the block to at most the largest, and every run
    /// lines up with the latency: `offset + base == size·(1 + skip)`.
    #[test]
    fn schedules_tile_the_response_and_line_up() {
        for block in [1usize, 2, 8, 64] {
            for largest in [block, 2 * block, 16 * block, 1024 * block] {
                for head in [Head::Partitioned, Head::Direct] {
                    let base = if head == Head::Direct { 0 } else { block };
                    for ir_len in (0..40).chain([200, 1_000, 5_000, 70_001]) {
                        let spans = schedule(ir_len, block, largest, head, 1, 1, usize::MAX);
                        let mut at = block - base;
                        let mut size = block;
                        for s in &spans {
                            assert_eq!(s.offset, at);
                            assert!(s.size.is_power_of_two() && s.size >= size);
                            assert!(at > block - base || s.size == block);
                            assert!(s.size <= largest && s.parts > 0);
                            assert_eq!(s.offset + base, s.size * (1 + s.skip));
                            size = s.size;
                            at += s.parts * s.size;
                        }
                        assert!(at >= ir_len, "the schedule covers the response");
                        if let Some(last) = spans.last() {
                            assert!(at - ir_len < last.size, "no idle partition");
                        } else {
                            assert!(ir_len <= block - base);
                        }
                    }
                }
            }
        }
    }

    fn cost(spans: &[Span]) -> f64 {
        spans.iter().map(|s| run_cost(s.size, s.parts, 1, 1)).sum()
    }

    /// Gardner's fixed doubling (three of `B`, then two of each size),
    /// the schedule the search must never lose to.
    fn gardner(ir_len: usize, block: usize, largest: usize, head: Head) -> Vec<Span> {
        let base = if head == Head::Direct { 0 } else { block };
        let (mut offset, mut size, mut skip) = (block - base, block, 0);
        let mut spans = Vec::new();
        while offset < ir_len {
            let fits = (ir_len - offset).div_ceil(size);
            let parts = if skip % 2 == 1 { 2 } else { 3 };
            let parts = if size >= largest {
                fits
            } else {
                parts.min(fits)
            };
            spans.push(Span {
                size,
                offset,
                skip,
                parts,
            });
            offset += parts * size;
            skip = (skip + 1 + parts) / 2 - 1;
            size *= 2;
        }
        spans
    }

    #[test]
    fn the_schedule_costs_no_more_than_uniform_or_gardner() {
        for (ir_len, block, largest) in [
            (16_384, 64, 4_096),
            (96_000, 64, 8_192),
            (96_000, 256, 1 << 16),
            (1_000, 1, 1 << 10),
            (300_000, 32, 1 << 14),
        ] {
            for head in [Head::Partitioned, Head::Direct] {
                let best = schedule(ir_len, block, largest, head, 1, 1, usize::MAX);
                let uniform = schedule(ir_len, block, block, head, 1, 1, usize::MAX);
                let fixed = gardner(ir_len, block, largest, head);
                assert!(cost(&best) <= cost(&uniform) + 1e-9);
                assert!(
                    cost(&best) <= cost(&fixed) + 1e-9,
                    "{ir_len} {block} {largest}"
                );
            }
        }
        // Two seconds at 48 kHz, zero latency: one long run of the
        // block, then one jump to a large size (García's typical shape).
        let hall: Vec<_> = schedule(96_000, 64, 8_192, Head::Direct, 1, 1, usize::MAX)
            .iter()
            .map(|s| (s.size, s.parts, s.skip))
            .collect();
        assert_eq!(hall, [(64, 31, 0), (2_048, 46, 0)]);
    }

    /// With deferral, every run of the deferred sizes keeps a block of
    /// slack (`skip ≥ 1`), the schedule still tiles the response and lines
    /// up, and it never beats the unconstrained schedule's cost.
    #[test]
    fn deferred_runs_keep_a_block_of_slack() {
        for block in [8usize, 32, 64] {
            for largest in [16 * block, 128 * block] {
                for from in [2 * block, 8 * block] {
                    for head in [Head::Partitioned, Head::Direct] {
                        let base = if head == Head::Direct { 0 } else { block };
                        for ir_len in [10usize, 200, 5_000, 96_000] {
                            let spans = schedule(ir_len, block, largest, head, 1, 1, from);
                            let free = schedule(ir_len, block, largest, head, 1, 1, usize::MAX);
                            let mut at = block - base;
                            for s in &spans {
                                assert_eq!(s.offset, at);
                                assert_eq!(s.offset + base, s.size * (1 + s.skip));
                                if s.size >= from {
                                    assert!(
                                        s.skip >= 1,
                                        "{s:?} for {ir_len} {block}..{largest} from {from}"
                                    );
                                }
                                at += s.parts * s.size;
                            }
                            assert!(at >= ir_len);
                            assert!(cost(&spans) + 1e-9 >= cost(&free));
                        }
                    }
                }
            }
        }
        // The hall at zero latency, deferring from 512 (twice a 256-sample
        // host buffer): the 64s stay on the audio thread, everything
        // else runs a block behind its input.
        let hall: Vec<_> = schedule(96_000, 64, 8_192, Head::Direct, 1, 1, 512)
            .iter()
            .map(|s| (s.size, s.parts, s.skip))
            .collect();
        println!("96 k taps, B = 64, deferred from 512: {hall:?}");
        assert!(hall.iter().all(|&(size, _, skip)| size < 512 || skip >= 1));
    }
}
