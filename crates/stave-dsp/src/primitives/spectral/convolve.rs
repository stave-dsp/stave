//! **Convolver** — convolution with an [`ImpulseResponse`] of any
//! length, by partitioned overlap-save.
//!
//! [`Convolver::uniform`] splits the response into partitions of one
//! block `B` (a power of two), transforms each once with a real
//! [`Fft`] of `2B`, and keeps the spectra of the last `P` input blocks
//! in a frequency-domain delay line. Every `B` samples it transforms the
//! newest block, multiplies and sums the delay line against the
//! partitions, and transforms back: one forward and one inverse FFT per
//! lane per block, plus `P·(B+1)` complex multiply-adds per path.
//!
//! [`Convolver::non_uniform`] keeps the latency of a `B` partition but
//! covers the later response with larger partitions: power-of-two sizes
//! from `B` up to a largest the caller chooses (Gardner's doubling),
//! with the count at each size chosen to minimise a measured cost model
//! (García's optimal partitioning). Each size is a uniform run of its
//! own, so a two-second response at a 64-sample latency costs one long
//! run of `B` and a few dozen large partitions instead of fifteen
//! hundred `B`-sized products.
//!
//! The first partition costs `B` samples of latency, reported exactly.
//! [`Head::Direct`] removes it: the first `B` taps run in direct form,
//! sample by sample, and the partitioned part starts at tap `B`, where
//! its one-block delay lines up with the response — zero latency for
//! `B` multiply-adds per path per sample.

use super::fft::{Complex, Fft};
use super::impulse_response::ImpulseResponse;
use crate::kernel::node::{Stage, Tail};
use crate::units::SampleRate;

mod swap;

/// How a [`Convolver`] computes the first block of its response.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Head {
    /// In the partitioned engine like the rest: the convolver's
    /// latency is one block.
    #[default]
    Partitioned,
    /// In direct form, per sample: zero latency, `B` more
    /// multiply-adds per path per sample.
    Direct,
}

/// One input → output path's direct-form head: the first `B` taps,
/// reversed.
#[derive(Clone, Debug)]
struct HeadPath {
    input: usize,
    output: usize,
    taps: Vec<f32>,
}

/// One input → output path's partitions within a [`Segment`].
#[derive(Clone, Debug)]
struct SegmentPath {
    input: usize,
    output: usize,
    /// `parts` partition spectra of `size + 1` bins, scaled by
    /// `1/2·size` so the unnormalised inverse lands at unity.
    spectra: Split,
}

/// Spectra stored as separate real and imaginary arrays, so the
/// multiply-accumulate over the delay line runs on four flat `f32`
/// slices (which the compiler vectorises).
#[derive(Clone, Debug)]
struct Split {
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Split {
    fn zeroed(len: usize) -> Self {
        Self {
            re: vec![0.0; len],
            im: vec![0.0; len],
        }
    }

    fn store(&mut self, at: usize, bins: &[Complex<f32>]) {
        let (re, im) = (
            &mut self.re[at..at + bins.len()],
            &mut self.im[at..at + bins.len()],
        );
        for ((r, i), z) in re.iter_mut().zip(im.iter_mut()).zip(bins) {
            *r = z.re;
            *i = z.im;
        }
    }

    fn fill_zero(&mut self) {
        self.re.fill(0.0);
        self.im.fill(0.0);
    }
}

/// `acc += x · h` over one spectrum, split form.
#[inline]
fn multiply_add(acc: (&mut [f32], &mut [f32]), x: (&[f32], &[f32]), h: (&[f32], &[f32])) {
    let n = acc.0.len();
    let (ar, ai) = (&mut acc.0[..n], &mut acc.1[..n]);
    let (xr, xi, hr, hi) = (&x.0[..n], &x.1[..n], &h.0[..n], &h.1[..n]);
    for k in 0..n {
        ar[k] += xr[k] * hr[k] - xi[k] * hi[k];
        ai[k] += xr[k] * hi[k] + xi[k] * hr[k];
    }
}

/// `Σ a[k]·b[k]` in eight running sums (a fixed order, so the tick and
/// block paths agree bit for bit, and one the compiler vectorises).
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sums = [0.0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            sums[k] += x[k] * y[k];
        }
    }
    let tail: f32 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
    ((sums[0] + sums[4]) + (sums[1] + sums[5])) + ((sums[2] + sums[6]) + (sums[3] + sums[7])) + tail
}

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
pub(crate) fn schedule(
    ir_len: usize,
    block: usize,
    largest: usize,
    head: Head,
    lanes: usize,
    paths: usize,
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
}

/// One uniform partitioned run of a [`Span`]: its transform, input
/// frames, delay line and output.
#[derive(Clone, Debug)]
struct Segment {
    size: usize,
    /// Where this run's partitions start in the response, in taps.
    offset: usize,
    skip: usize,
    parts: usize,
    paths: Vec<SegmentPath>,
    fft: Fft<f32>,
    /// Per input lane, `2L` samples: the previous block, then the one
    /// filling (overlap-save's frame).
    frames: Vec<f32>,
    /// Per input lane, `skip + parts` spectra of `L + 1` bins (the delay
    /// line).
    fdl: Split,
    /// The delay-line slot holding the newest spectrum.
    newest: usize,
    /// Per output lane, `2L` samples of the last inverse transform; the
    /// block being played out is the second half.
    out: Vec<f32>,
    /// One spectrum of scratch: the forward transform's output, then the
    /// sum before the inverse transform.
    spectrum: Vec<Complex<f32>>,
    /// The sum over the delay line, split form.
    acc: Split,
    /// Position in the current block, `0..L`.
    pos: usize,
    /// A response swap ([`Convolver::swap_response`]): empty until
    /// [`Convolver::reserve_swaps`].
    swap: swap::SegmentSwap,
}

impl Segment {
    fn new(ir: &ImpulseResponse, span: Span) -> Self {
        let Span {
            size,
            offset,
            skip,
            parts,
        } = span;
        let lanes = ir.layout().lanes();
        let bins = size + 1;
        let fft = Fft::new(2 * size);
        let scale = 1.0 / (2 * size) as f32;
        let mut frame = vec![0.0f32; 2 * size];
        let mut spectrum = vec![Complex::default(); bins];
        let mut paths = Vec::new();
        for input in 0..lanes {
            for output in 0..lanes {
                let Some(h) = ir.path(input, output) else {
                    continue;
                };
                let mut spectra = Split::zeroed(parts * bins);
                for p in 0..parts {
                    frame.fill(0.0);
                    let start = (offset + p * size).min(h.len());
                    let end = (start + size).min(h.len());
                    for (f, &v) in frame.iter_mut().zip(&h[start..end]) {
                        *f = v * scale;
                    }
                    fft.forward_real(&frame, &mut spectrum);
                    spectra.store(p * bins, &spectrum);
                }
                paths.push(SegmentPath {
                    input,
                    output,
                    spectra,
                });
            }
        }
        Self {
            size,
            offset,
            skip,
            parts,
            paths,
            fft,
            frames: vec![0.0; lanes * 2 * size],
            fdl: Split::zeroed(lanes * (skip + parts) * bins),
            newest: 0,
            out: vec![0.0; lanes * 2 * size],
            spectrum,
            acc: Split::zeroed(bins),
            pos: 0,
            swap: swap::SegmentSwap::default(),
        }
    }

    fn reset(&mut self) {
        self.frames.fill(0.0);
        self.fdl.fill_zero();
        self.out.fill(0.0);
        self.newest = 0;
        self.pos = 0;
        self.swap.finish(&mut self.paths);
    }

    /// One sample in on every lane: adds the block being played out to
    /// `y`, then steps when the block fills.
    #[inline]
    fn advance(&mut self, x: [f32; 2], y: &mut [f32; 2], lanes: usize) {
        let l = self.size;
        let pos = self.pos;
        for lane in 0..lanes {
            let at = lane * 2 * l + l + pos;
            y[lane] += self.swap.blend(&self.out, at, pos, l);
            self.frames[lane * 2 * l + l + pos] = x[lane];
        }
        self.pos += 1;
        if self.pos == l {
            self.pos = 0;
            self.step(lanes);
        }
    }

    /// The block step: transform each lane's frame into the delay line,
    /// sum the products per output, transform back.
    fn step(&mut self, lanes: usize) {
        let l = self.size;
        let bins = l + 1;
        let slots = self.skip + self.parts;
        self.newest = (self.newest + 1) % slots;
        for lane in 0..lanes {
            let frame = &mut self.frames[lane * 2 * l..(lane + 1) * 2 * l];
            let at = (lane * slots + self.newest) * bins;
            self.fft.forward_real(frame, &mut self.spectrum);
            self.fdl.store(at, &self.spectrum);
            frame.copy_within(l.., 0);
        }
        if self.swap.begins_fade() {
            // The old response's block for the fade's start, then the
            // new one's, which stays.
            self.convolve(lanes);
            self.swap.keep_old_block(&mut self.out);
            self.swap.install(&mut self.paths);
        }
        self.convolve(lanes);
    }

    /// Sums the delay line against the partitions into `out`, per
    /// output lane.
    fn convolve(&mut self, lanes: usize) {
        let l = self.size;
        let bins = l + 1;
        let slots = self.skip + self.parts;
        for output in 0..lanes {
            self.acc.fill_zero();
            for path in self.paths.iter().filter(|p| p.output == output) {
                let line = path.input * slots * bins;
                for p in 0..self.parts {
                    let x = line + fdl_slot(self.newest, self.skip + p, slots) * bins;
                    let h = p * bins;
                    multiply_add(
                        (&mut self.acc.re, &mut self.acc.im),
                        (&self.fdl.re[x..x + bins], &self.fdl.im[x..x + bins]),
                        (&path.spectra.re[h..h + bins], &path.spectra.im[h..h + bins]),
                    );
                }
            }
            for ((z, &re), &im) in self.spectrum.iter_mut().zip(&self.acc.re).zip(&self.acc.im) {
                *z = Complex::new(re, im);
            }
            let out = &mut self.out[output * 2 * l..(output + 1) * 2 * l];
            self.fft.inverse_real(&mut self.spectrum, out);
        }
    }
}

/// Convolution with an [`ImpulseResponse`] of any length, in blocks of a
/// power-of-two partition, with no allocation after construction.
///
/// A mono response is a [`Stage`]; stereo and true-stereo responses run
/// through [`tick_stereo`](Self::tick_stereo) /
/// [`process_stereo`](Self::process_stereo).
///
/// # Models
///
/// Linear convolution, `y[t] = Σ h[j]·x[t − j]`, for every path of the
/// response, delayed by [`latency`](Stage::latency): one partition with
/// [`Head::Partitioned`], none with [`Head::Direct`].
///
/// # Derivation
///
/// Textbook uniform partitioned overlap-save with a frequency-domain
/// delay line (Wefers, *Partitioned convolution algorithms for
/// real-time auralization*, 2015, ch. 5), on the real [`Fft`]. The
/// non-uniform schedule: power-of-two sizes as in Gardner (*Efficient
/// convolution without input-output delay*, JAES 1995), each a uniform
/// run, counts chosen by dynamic programming over a cost model as in
/// García (*Optimal filter partition for efficient convolution with
/// short input/output delay*, AES 2002) and Wefers ch. 6.
///
/// # Verified
///
/// Against direct-form convolution in `f64` for random responses and
/// partitions, ragged blocks (blocks of one included), responses shorter
/// and longer than a partition, mono, stereo and true stereo, both heads,
/// uniform and non-uniform: worst error 2e-6 `‖h‖₂·max|x|` (budget
/// 2e-5), `crates/stave-dsp/tests/convolver_properties.rs`; an impulse
/// returns the response at every host rate,
/// `crates/stave-dsp/tests/sample_rate_range.rs`. The delay line's index maths is a Kani
/// proof (`delay_line_slots_are_distinct_and_in_range`); every schedule
/// covers the response once and lines up with the latency
/// (`schedules_tile_the_response_and_line_up`), and costs no more under
/// the model than the uniform or Gardner's fixed schedule
/// (`the_schedule_costs_no_more_than_uniform_or_gardner`).
///
/// # Cost
///
/// `tests/datasheet.rs`, "Convolver", per sample of mono audio on an
/// Apple M-series core: at a 256-sample partition about 15 ns for 1 k
/// taps, 34 for 16 k and 146 for 96 k (two seconds at 48 kHz); the
/// direct head at a 64-sample partition about 30 ns for 1 k taps and 149
/// for 16 k. Non-uniform with the direct head at a 64-sample partition:
/// about 53 ns for 16 k taps (largest 4096) and 68 for 96 k (largest
/// 8192), with a worst 64-sample block of about 0.36 and 1.2 µs per
/// sample. A true-stereo response costs four paths, two
/// transforms each way. The non-uniform schedule runs every size's step
/// at its block boundary on the calling thread, so the block where a
/// large partition steps costs more than the average (the datasheet
/// prints the worst 64-sample block); moving those steps to a worker
/// thread is DSP.md F3b.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// Partitions `1..=2^30`, powers of two; any response length. A
/// response with a recorded rate ([`ImpulseResponse::from_channels`],
/// [`ImpulseResponse::recorded_at`]) is resampled to the rate
/// [`prepare`](Stage::prepare) is given, and re-planned there (prepare
/// allocates when the rate changes); one without is taken at the
/// prepared rate as given.
///
/// ```
/// use stave_dsp::primitives::spectral::Head;
/// use stave_dsp::{Convolver, ImpulseResponse, Stage};
///
/// // An echo: the dry signal, then half of it 3 samples later.
/// let ir = ImpulseResponse::mono(&[1.0, 0.0, 0.0, 0.5]);
/// let mut conv = Convolver::uniform(&ir, 4, Head::Direct);
/// conv.prepare(48_000.0);
/// assert_eq!(conv.latency(), 0);
/// let out: Vec<f32> = [1.0, 0.0, 0.0, 0.0, 0.0].iter().map(|&x| conv.tick(x)).collect();
/// assert!(out.iter().zip([1.0, 0.0, 0.0, 0.5, 0.0]).all(|(y, h)| (y - h).abs() < 1e-6));
/// ```
#[derive(Clone, Debug)]
pub struct Convolver {
    block: usize,
    lanes: usize,
    head: Head,
    ir_len: usize,
    heads: Vec<HeadPath>,
    /// Per input lane, `2B` samples of history for the direct head
    /// (each sample written twice, so a window is one slice).
    history: Vec<f32>,
    /// Position in the head's block, `0..B`.
    pos: usize,
    segments: Vec<Segment>,
    /// The response as given, when it has a recorded rate: `prepare`
    /// re-plans from it at the prepared rate.
    source: Option<Box<ImpulseResponse>>,
    /// The largest partition the schedule may use.
    largest: usize,
    /// The rate the current plan's response is at, when known.
    planned: Option<SampleRate>,
    /// The direct head's side of a response swap; empty until
    /// [`reserve_swaps`](Self::reserve_swaps).
    head_swap: swap::HeadSwap,
}

/// The delay-line slot of the spectrum `age` blocks older than the one
/// in slot `newest`, in a line of `parts` slots.
#[inline]
pub(crate) fn fdl_slot(newest: usize, age: usize, parts: usize) -> usize {
    debug_assert!(newest < parts && age < parts);
    if age <= newest {
        newest - age
    } else {
        newest + parts - age
    }
}

impl Convolver {
    /// A convolver with `ir`, in uniform partitions of `block` samples.
    /// Plans everything here (this allocates); nothing allocates after.
    ///
    /// # Panics
    ///
    /// If `block` is not a power of two.
    pub fn uniform(ir: &ImpulseResponse, block: usize, head: Head) -> Self {
        Self::non_uniform(ir, block, block, head)
    }

    /// A convolver with `ir` at the latency of a `block`-sample
    /// partition (or none, with [`Head::Direct`]), its later taps in
    /// power-of-two partitions of up to `largest` samples, the count at
    /// each size chosen for the least modelled cost (see
    /// [`schedule`](Self::schedule)). `largest == block` is
    /// [`uniform`](Self::uniform).
    ///
    /// The larger partitions step at their own block boundaries on the
    /// calling thread, so the worst block costs more than the average:
    /// `largest` bounds that spike (one forward and one inverse
    /// transform of `2·largest` per lane, plus its products).
    ///
    /// ```
    /// use stave_dsp::primitives::spectral::Head;
    /// use stave_dsp::{Convolver, ImpulseResponse, Stage};
    ///
    /// let hall = vec![0.01_f32; 96_000]; // two seconds at 48 kHz
    /// let conv = Convolver::non_uniform(&ImpulseResponse::mono(&hall), 64, 8_192, Head::Direct);
    /// assert_eq!(conv.latency(), 0);
    /// // 31 partitions of 64, then 46 of 2048: ~1/10 of the products a
    /// // uniform 64-sample schedule would run.
    /// assert_eq!(conv.schedule().collect::<Vec<_>>(), [(64, 31), (2_048, 46)]);
    /// ```
    ///
    /// # Panics
    ///
    /// If `block` or `largest` is not a power of two, or `largest <
    /// block`.
    pub fn non_uniform(ir: &ImpulseResponse, block: usize, largest: usize, head: Head) -> Self {
        assert!(
            block.is_power_of_two(),
            "Convolver partition must be a power of two, got {block}"
        );
        assert!(
            largest.is_power_of_two() && largest >= block,
            "Convolver largest partition must be a power of two of at least {block}, got {largest}"
        );
        let mut conv = Self::plan(ir, block, largest, head);
        if ir.sample_rate().is_some() {
            conv.source = Some(Box::new(ir.clone()));
        }
        conv
    }

    /// The plan for `ir` as given (no source kept).
    fn plan(ir: &ImpulseResponse, block: usize, largest: usize, head: Head) -> Self {
        let lanes = ir.layout().lanes();
        let ir_len = ir.len();
        let mut heads = Vec::new();
        if head == Head::Direct {
            for input in 0..lanes {
                for output in 0..lanes {
                    if let Some(h) = ir.path(input, output) {
                        let taps = (0..block)
                            .map(|k| h.get(block - 1 - k).copied().unwrap_or(0.0))
                            .collect();
                        heads.push(HeadPath {
                            input,
                            output,
                            taps,
                        });
                    }
                }
            }
        }
        let paths = (0..lanes)
            .flat_map(|i| (0..lanes).map(move |o| (i, o)))
            .filter(|&(i, o)| ir.path(i, o).is_some())
            .count();
        let segments = schedule(ir_len, block, largest, head, lanes, paths)
            .into_iter()
            .map(|span| Segment::new(ir, span))
            .collect();
        Self {
            block,
            lanes,
            head,
            ir_len,
            heads,
            history: match head {
                Head::Partitioned => Vec::new(),
                Head::Direct => vec![0.0; lanes * 2 * block],
            },
            pos: 0,
            segments,
            source: None,
            largest,
            planned: ir.sample_rate(),
            head_swap: swap::HeadSwap::default(),
        }
    }

    /// The smallest partition `B`, in samples: the one that sets the
    /// latency.
    pub const fn block(&self) -> usize {
        self.block
    }

    /// The partitions the frequency-domain part runs, over every size.
    pub fn partitions(&self) -> usize {
        self.segments.iter().map(|s| s.parts).sum()
    }

    /// The schedule: `(partition size, count)` per run, smallest first.
    /// A uniform convolver has one run (none for a response no longer
    /// than its direct head).
    pub fn schedule(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.segments.iter().map(|s| (s.size, s.parts))
    }

    /// The lanes this convolver runs: 1 for a mono response, 2 for
    /// stereo and true stereo.
    pub const fn lanes(&self) -> usize {
        self.lanes
    }

    /// One stereo sample through a stereo or true-stereo response.
    ///
    /// Realtime: never allocates. A mono convolver passes `right`
    /// through unchanged (`debug_assert!`s that it is stereo); for dual
    /// mono, build it from [`ImpulseResponse::stereo`] with the same
    /// response twice.
    #[inline]
    pub fn tick_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        debug_assert_eq!(self.lanes, 2, "tick_stereo on a mono Convolver");
        let [l, r] = self.advance([left, right]);
        if self.lanes == 2 {
            (l, r)
        } else {
            (l, right)
        }
    }

    /// Two lanes of a block through a stereo or true-stereo response, in
    /// place; bit-identical to [`tick_stereo`](Self::tick_stereo) per
    /// sample. The lanes must be the same length (the shorter bounds the
    /// block).
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            (*l, *r) = self.tick_stereo(*l, *r);
        }
    }

    /// One sample in on every lane, one out: the output of every run's
    /// block being played out, plus the direct head, then each run's
    /// block step when its block fills.
    #[inline]
    fn advance(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut y = [0.0f32; 2];
        for segment in &mut self.segments {
            segment.advance(x, &mut y, self.lanes);
        }
        if self.head == Head::Direct {
            let b = self.block;
            let pos = self.pos;
            for lane in 0..self.lanes {
                let h = &mut self.history[lane * 2 * b..(lane + 1) * 2 * b];
                h[pos] = x[lane];
                h[pos + b] = x[lane];
            }
            for path in &self.heads {
                let h = &self.history[path.input * 2 * b..(path.input + 1) * 2 * b];
                let window = &h[pos + 1..pos + 1 + b];
                y[path.output] += self.head_swap.dot(window, &path.taps);
            }
            let run_fading = self.segments.first().is_some_and(Segment::is_fading);
            self.head_swap.advance(&mut self.heads, run_fading);
            self.pos = (pos + 1) & (b - 1);
        }
        y
    }
}

impl Stage for Convolver {
    /// Re-plans from a response with a recorded rate when the rate
    /// differs from the current plan's (this allocates: the response is
    /// resampled, its partitions transformed again), then clears the
    /// state. A response without a recorded rate is taken as given.
    fn prepare(&mut self, sample_rate: f32) {
        if self.source.is_some() && self.planned != Some(SampleRate::new(sample_rate)) {
            if let Some(source) = self.source.take() {
                let ir = source.resampled(SampleRate::new(sample_rate));
                let swappable = self.head_swap.is_reserved()
                    || self.segments.iter().any(|s| s.swap.is_reserved());
                *self = Self::plan(&ir, self.block, self.largest, self.head);
                self.source = Some(source);
                if swappable {
                    self.reserve_swaps();
                }
            }
        }
        self.reset();
    }

    fn reset(&mut self) {
        for segment in &mut self.segments {
            segment.reset();
        }
        self.history.fill(0.0);
        self.pos = 0;
        self.head_swap.finish(&mut self.heads);
    }

    /// One sample through a mono response (a stereo one `debug_assert!`s
    /// and runs its left input to its left output).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        debug_assert_eq!(self.lanes, 1, "tick on a stereo Convolver");
        self.advance([x, 0.0])[0]
    }

    fn latency(&self) -> u32 {
        match self.head {
            Head::Partitioned => self.block as u32,
            Head::Direct => 0,
        }
    }

    /// The response's length after the latency: the last tap sounds
    /// `len − 1` samples after the input that struck it.
    fn tail(&self) -> Tail {
        Tail::samples(self.latency() + self.ir_len.saturating_sub(1) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::spectral::IrLayout;

    #[test]
    fn delay_line_slots_cover_the_line_newest_first() {
        for parts in 1..9 {
            for newest in 0..parts {
                let mut seen = vec![false; parts];
                for age in 0..parts {
                    let s = fdl_slot(newest, age, parts);
                    assert!(s < parts && !seen[s]);
                    seen[s] = true;
                }
                assert_eq!(fdl_slot(newest, 0, parts), newest);
            }
        }
    }

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
                        let spans = schedule(ir_len, block, largest, head, 1, 1);
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
            let parts = if size >= largest { fits } else { parts.min(fits) };
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
                let best = schedule(ir_len, block, largest, head, 1, 1);
                let uniform = schedule(ir_len, block, block, head, 1, 1);
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
        let hall: Vec<_> = schedule(96_000, 64, 8_192, Head::Direct, 1, 1)
            .iter()
            .map(|s| (s.size, s.parts, s.skip))
            .collect();
        assert_eq!(hall, [(64, 31, 0), (2_048, 46, 0)]);
    }

    #[test]
    fn a_layout_reports_its_lanes() {
        assert_eq!(IrLayout::Mono.lanes(), 1);
        assert_eq!(IrLayout::TrueStereo.lanes(), 2);
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// The delay line's index maths: every age of every line up to 64
    /// slots lands inside the line, age 0 is the newest slot, and two
    /// different ages never share a slot.
    #[kani::proof]
    fn delay_line_slots_are_distinct_and_in_range() {
        let parts: usize = kani::any_where(|p: &usize| (1..=64).contains(p));
        let newest: usize = kani::any_where(|n: &usize| *n < parts);
        let a: usize = kani::any_where(|a: &usize| *a < parts);
        let b: usize = kani::any_where(|b: &usize| *b < parts);
        let (sa, sb) = (fdl_slot(newest, a, parts), fdl_slot(newest, b, parts));
        assert!(sa < parts);
        assert_eq!(fdl_slot(newest, 0, parts), newest);
        if a != b {
            assert_ne!(sa, sb);
        }
    }
}
