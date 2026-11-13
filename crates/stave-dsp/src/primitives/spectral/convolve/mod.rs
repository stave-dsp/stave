//! **Convolver** — convolution with an [`ImpulseResponse`] of any
//! length, by partitioned overlap-save.
//!
//! [`Convolver::uniform`] splits the response into partitions of one
//! block `B` (a power of two), transforms each once with a real
//! [`Fft`](super::Fft) of `2B`, and keeps the spectra of the last `P` input blocks
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

mod crossfade;
mod schedule;
mod segment;
mod swap;
mod worker;

pub use crossfade::Crossfade;
pub use segment::Run;
pub use worker::{Inline, Worker};

use super::impulse_response::ImpulseResponse;
use crate::kernel::node::{Stage, Tail};
use crate::units::SampleRate;
use schedule::schedule;
use segment::{Segment, SegmentPath, Split};

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
/// real-time auralization*, 2015, ch. 5), on the real [`Fft`](super::Fft). The
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
/// prints the worst 64-sample block); [`Convolver::deferred`] moves
/// those steps to a [`Worker`] (DSP.md F3b), and with stave-core's
/// threaded one the worst 64-sample block of 96 k taps costs what the
/// average does (stave-core's `convolution_workers` test prints both).
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
pub struct Convolver<W = Inline> {
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
    /// The runs stepped on the calling thread.
    segments: Vec<Segment>,
    /// The deferred runs' audio-thread side, smallest first.
    ports: Vec<Port>,
    /// Who computes the deferred runs.
    worker: W,
    /// Deferred blocks the worker did not deliver in time.
    late: u64,
    /// Every `(size, parts)` run, smallest first, deferred or not.
    plan: Vec<(usize, usize)>,
    /// The response as given, when it has a recorded rate: `prepare`
    /// re-plans from it at the prepared rate.
    source: Option<Box<ImpulseResponse>>,
    /// The largest partition the schedule may use.
    largest: usize,
    /// The smallest partition computed by the worker (`usize::MAX`:
    /// none).
    defer: usize,
    /// The rate the current plan's response is at, when known.
    planned: Option<SampleRate>,
    /// The direct head's side of a response swap; empty until
    /// [`reserve_swaps`](Self::reserve_swaps).
    head_swap: swap::HeadSwap,
    /// How the head runs this sample: the head swap's crossfade is a
    /// third case of the head's own dispatch, so the steady state pays
    /// nothing for it.
    head_mode: swap::HeadMode,
}

/// A deferred run's audio-thread side: the input block filling and the
/// output block playing.
#[derive(Clone, Debug)]
struct Port {
    size: usize,
    pos: usize,
    input: Box<[f32]>,
    playing: Box<[f32]>,
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

impl Convolver<Inline> {
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
    /// transform of `2·largest` per lane, plus its products), and
    /// [`deferred`](Self::deferred) removes it.
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
        Self::build(ir, block, largest, head, usize::MAX)
    }

    /// [`non_uniform`](Self::non_uniform) with every run of `from`
    /// samples or more *deferred*: given a block of slack in the
    /// schedule (`skip ≥ 1`), and computed a block ahead by a [`Worker`]
    /// instead of at its boundary, so the audio thread's worst block
    /// costs what its average does. This one computes them [`Inline`]
    /// (the deterministic reference, same output as a threaded worker
    /// that keeps its deadlines); hand them to threads with
    /// [`with_worker`](Convolver::with_worker).
    ///
    /// A deferred run of size `L` has one period, `L` samples, to
    /// compute its block (Battenberg & Avizienis 2011's deadline); with
    /// host buffers of `H` samples a worker sees about `L − H` of it, so
    /// choose `from` at least twice the largest host buffer.
    ///
    /// ```
    /// use stave_dsp::primitives::spectral::Head;
    /// use stave_dsp::{Convolver, ImpulseResponse};
    ///
    /// let hall = vec![0.01_f32; 96_000];
    /// let conv = Convolver::deferred(&ImpulseResponse::mono(&hall), 64, 8_192, Head::Direct, 512);
    /// // The runs of 512 samples and more go to the worker.
    /// let deferred = conv.schedule().filter(|&(size, _)| size >= 512).count();
    /// assert!(deferred >= 1);
    /// assert_eq!(conv.deferred_runs(), deferred);
    /// ```
    ///
    /// # Panics
    ///
    /// As [`non_uniform`](Self::non_uniform), or if `from <= block`
    /// (the smallest run's deadline is the block itself).
    pub fn deferred(
        ir: &ImpulseResponse,
        block: usize,
        largest: usize,
        head: Head,
        from: usize,
    ) -> Self {
        assert!(
            from > block,
            "Convolver deferred runs must be larger than the block {block}, got {from}"
        );
        Self::build(ir, block, largest, head, from)
    }

    fn build(ir: &ImpulseResponse, block: usize, largest: usize, head: Head, defer: usize) -> Self {
        assert!(
            block.is_power_of_two(),
            "Convolver partition must be a power of two, got {block}"
        );
        assert!(
            largest.is_power_of_two() && largest >= block,
            "Convolver largest partition must be a power of two of at least {block}, got {largest}"
        );
        let (mut conv, runs) = Self::plan(ir, block, largest, head, defer, defer);
        conv.worker.assign(runs);
        if ir.sample_rate().is_some() {
            conv.source = Some(Box::new(ir.clone()));
        }
        conv
    }

    /// The plan for `ir` as given (no source kept), and its deferred
    /// runs: the schedule keeps slack for runs of `defer` or more, and
    /// runs of `split` or more go to the worker (`split == defer`, except
    /// in the test that holds a deferred convolver to the same schedule
    /// stepped in place).
    fn plan(
        ir: &ImpulseResponse,
        block: usize,
        largest: usize,
        head: Head,
        defer: usize,
        split: usize,
    ) -> (Self, Vec<Run>) {
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
        let spans = schedule(ir_len, block, largest, head, lanes, paths, defer);
        let plan = spans.iter().map(|s| (s.size, s.parts)).collect();
        let (mut segments, mut ports, mut runs) = (Vec::new(), Vec::new(), Vec::new());
        for span in spans {
            if span.size >= split {
                ports.push(Port {
                    size: span.size,
                    pos: 0,
                    input: vec![0.0; lanes * span.size].into_boxed_slice(),
                    playing: vec![0.0; lanes * span.size].into_boxed_slice(),
                });
                runs.push(Run::new(ir, span));
            } else {
                segments.push(Segment::new(ir, span));
            }
        }
        let conv = Self {
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
            ports,
            worker: Inline::new(),
            late: 0,
            plan,
            source: None,
            largest,
            defer,
            planned: ir.sample_rate(),
            head_swap: swap::HeadSwap::default(),
            head_mode: swap::HeadMode::of(head),
        };
        (conv, runs)
    }

    /// This convolver with its deferred runs computed by `worker`
    /// (which [`assign`](Worker::assign)s them now: a threaded worker
    /// spawns here). Allocates; build it off the audio thread.
    pub fn with_worker<V: Worker>(mut self, mut worker: V) -> Convolver<V> {
        worker.assign(self.worker.take());
        Convolver {
            block: self.block,
            lanes: self.lanes,
            head: self.head,
            ir_len: self.ir_len,
            heads: self.heads,
            history: self.history,
            pos: self.pos,
            segments: self.segments,
            ports: self.ports,
            worker,
            late: self.late,
            plan: self.plan,
            source: self.source,
            largest: self.largest,
            defer: self.defer,
            planned: self.planned,
            head_swap: self.head_swap,
            head_mode: self.head_mode,
        }
    }
}

impl<W: Worker> Convolver<W> {
    /// The smallest partition `B`, in samples: the one that sets the
    /// latency.
    pub const fn block(&self) -> usize {
        self.block
    }

    /// The partitions the frequency-domain part runs, over every size.
    pub fn partitions(&self) -> usize {
        self.plan.iter().map(|&(_, parts)| parts).sum()
    }

    /// The schedule: `(partition size, count)` per run, smallest first,
    /// deferred runs included. A uniform convolver has one run (none for
    /// a response no longer than its direct head).
    pub fn schedule(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.plan.iter().copied()
    }

    /// The runs a [`Worker`] computes (the largest sizes of the
    /// schedule; 0 unless built [`deferred`](Convolver::deferred)).
    pub fn deferred_runs(&self) -> usize {
        self.ports.len()
    }

    /// Deferred blocks the worker did not deliver by their boundary
    /// (each played as silence for that run), since construction.
    pub fn late_blocks(&self) -> u64 {
        self.late
    }

    /// The worker computing the deferred runs.
    pub fn worker(&self) -> &W {
        &self.worker
    }

    /// The worker, mutably.
    pub fn worker_mut(&mut self) -> &mut W {
        &mut self.worker
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
    /// block step (or a deferred run's exchange with the worker) when its
    /// block fills.
    #[inline]
    fn advance(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut y = [0.0f32; 2];
        for segment in &mut self.segments {
            segment.advance(x, &mut y, self.lanes);
        }
        for (index, port) in self.ports.iter_mut().enumerate() {
            let (l, pos) = (port.size, port.pos);
            for lane in 0..self.lanes {
                y[lane] += port.playing[lane * l + pos];
                port.input[lane * l + pos] = x[lane];
            }
            port.pos += 1;
            if port.pos == l {
                port.pos = 0;
                if !self
                    .worker
                    .exchange(index, &mut port.input, &mut port.playing)
                {
                    self.late += 1;
                }
            }
        }
        match self.head_mode {
            swap::HeadMode::Direct => {
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
                    y[path.output] += dot(window, &path.taps);
                }
                self.pos = (pos + 1) & (b - 1);
            }
            swap::HeadMode::Swapping => {
                // By value: a `&mut y` would keep `y` in memory on the
                // steady path too (about 12 instructions a sample).
                let head = self.head_swapping(x);
                y[0] += head[0];
                y[1] += head[1];
            }
            swap::HeadMode::Partitioned => {}
        }
        y
    }
}

impl<W: Worker> Stage for Convolver<W> {
    /// Re-plans from a response with a recorded rate when the rate
    /// differs from the current plan's (this allocates: the response is
    /// resampled, its partitions transformed again, and the worker
    /// handed the new runs), then clears the state. A response without
    /// a recorded rate is taken as given.
    fn prepare(&mut self, sample_rate: f32) {
        if self.source.is_some() && self.planned != Some(SampleRate::new(sample_rate)) {
            if let Some(source) = self.source.take() {
                let ir = source.resampled(SampleRate::new(sample_rate));
                let swappable = self.head_swap.is_reserved()
                    || self.segments.iter().any(|s| s.swap.is_reserved());
                let (plan, runs) = Convolver::plan(
                    &ir,
                    self.block,
                    self.largest,
                    self.head,
                    self.defer,
                    self.defer,
                );
                self.worker.assign(runs);
                self.ir_len = plan.ir_len;
                self.heads = plan.heads;
                self.history = plan.history;
                self.segments = plan.segments;
                self.ports = plan.ports;
                self.plan = plan.plan;
                self.planned = plan.planned;
                self.head_swap = swap::HeadSwap::default();
                self.head_mode = swap::HeadMode::of(self.head);
                self.source = Some(source);
                if swappable {
                    self.reserve_swaps();
                }
            }
        }
        self.worker.prepare(sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        for segment in &mut self.segments {
            segment.reset();
        }
        for port in &mut self.ports {
            port.pos = 0;
            port.input.fill(0.0);
            port.playing.fill(0.0);
        }
        self.worker.reset();
        self.history.fill(0.0);
        self.pos = 0;
        self.head_swap.finish(&mut self.heads);
        self.head_mode = swap::HeadMode::of(self.head);
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

    /// A deferred convolver, its runs computed a block ahead through the
    /// [`Inline`] worker, is bit-identical to the same slack schedule
    /// stepped in place at every boundary: deferral moves the work, not
    /// the arithmetic. Mono, stereo and true stereo; both heads; one and
    /// two deferred sizes.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn deferral_is_bit_identical_to_stepping_in_place() {
        let mut seed = 0x1234_5678u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 - 0.5
        };
        let h: Vec<f32> = (0..5_000).map(|_| noise()).collect();
        let g: Vec<f32> = (0..5_000).map(|_| noise()).collect();
        let x: Vec<(f32, f32)> = (0..20_000).map(|_| (noise(), noise())).collect();
        for ir in [
            ImpulseResponse::mono(&h),
            ImpulseResponse::stereo(&h, &g),
            ImpulseResponse::true_stereo(&h, &g, &g, &h),
        ] {
            for head in [Head::Partitioned, Head::Direct] {
                for (largest, from) in [(1_024, 256), (2_048, 128)] {
                    let mut deferred = Convolver::deferred(&ir, 32, largest, head, from);
                    let (mut stepped, runs) =
                        Convolver::plan(&ir, 32, largest, head, from, usize::MAX);
                    assert!(runs.is_empty() && deferred.deferred_runs() > 0);
                    assert_eq!(deferred.plan, stepped.plan);
                    deferred.prepare(48_000.0);
                    stepped.prepare(48_000.0);
                    for (n, &(l, r)) in x.iter().enumerate() {
                        let (a, b) = if ir.layout().lanes() == 1 {
                            (deferred.tick(l), stepped.tick(l))
                        } else {
                            (deferred.tick_stereo(l, r).0, stepped.tick_stereo(l, r).0)
                        };
                        assert_eq!(
                            a.to_bits(),
                            b.to_bits(),
                            "{:?} {head:?} {from}: sample {n}",
                            ir.layout()
                        );
                    }
                    assert_eq!(deferred.late_blocks(), 0);
                }
            }
        }
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
