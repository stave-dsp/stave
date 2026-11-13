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
//! The first partition costs `B` samples of latency, reported exactly.
//! [`Head::Direct`] removes it: the first `B` taps run in direct form,
//! sample by sample, and the partitioned part starts at tap `B`, where
//! its one-block delay lines up with the response — zero latency for
//! `B` multiply-adds per path per sample.
//!
//! Non-uniform schedules (short partitions first, long ones later) for
//! long responses at low latency are DSP.md F3.

use super::fft::{Complex, Fft};
use super::impulse_response::ImpulseResponse;
use crate::kernel::node::{Stage, Tail};

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

/// One input → output path's data.
#[derive(Clone, Debug)]
struct Path {
    input: usize,
    output: usize,
    /// The first `B` taps, reversed, for the direct-form head (empty
    /// with [`Head::Partitioned`]).
    head: Vec<f32>,
    /// `P` partition spectra of `B + 1` bins, scaled by `1/2B` so the
    /// unnormalised inverse lands at unity.
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
/// real-time auralization*, 2015, ch. 5), on the real [`Fft`].
///
/// # Verified
///
/// Against direct-form convolution in `f64` for random responses and
/// partitions, ragged blocks (blocks of one included), responses shorter
/// and longer than a partition, mono, stereo and true stereo, both heads:
/// worst error 2e-6 `‖h‖₂·max|x|` (budget 2e-5),
/// `crates/stave-dsp/tests/convolver_properties.rs`; an impulse returns
/// the response at every host rate, `tests/sample_rate_range.rs`. The
/// delay line's index maths is a Kani proof
/// (`delay_line_slots_are_distinct_and_in_range`).
///
/// # Cost
///
/// `tests/datasheet.rs`, "Convolver", per sample of mono audio on an
/// Apple M-series core: at a 256-sample partition about 15 ns for 1 k
/// taps, 34 for 16 k and 146 for 96 k (two seconds at 48 kHz); the
/// direct head at a 64-sample partition about 30 ns for 1 k taps and 149
/// for 16 k. A true-stereo response costs four paths, two transforms
/// each way.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// Partitions `1..=2^30`, powers of two; any response length. The
/// response is taken at the prepared rate as given (resampling is
/// DSP.md F4).
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
    parts: usize,
    lanes: usize,
    head: Head,
    ir_len: usize,
    paths: Vec<Path>,
    fft: Fft<f32>,
    /// Per input lane, `2B` samples: the previous block, then the one
    /// filling (overlap-save's frame).
    frames: Vec<f32>,
    /// Per input lane, `P` spectra of `B + 1` bins (the delay line).
    fdl: Split,
    /// The delay-line slot holding the newest spectrum.
    newest: usize,
    /// Per output lane, `2B` samples of the last inverse transform; the
    /// block being played out is the second half.
    out: Vec<f32>,
    /// One spectrum of scratch: the forward transform's output, then the
    /// sum before the inverse transform.
    spectrum: Vec<Complex<f32>>,
    /// The sum over the delay line, split form.
    acc: Split,
    /// Per input lane, `2B` samples of history for the direct head
    /// (each sample written twice, so a window is one slice).
    history: Vec<f32>,
    /// Position in the current block, `0..B`.
    pos: usize,
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
        assert!(
            block.is_power_of_two(),
            "Convolver partition must be a power of two, got {block}"
        );
        let lanes = ir.layout().lanes();
        let ir_len = ir.len();
        let skip = match head {
            Head::Partitioned => 0,
            Head::Direct => block,
        };
        let parts = ir_len.saturating_sub(skip).div_ceil(block);
        let bins = block + 1;
        let fft = Fft::new(2 * block);
        let scale = 1.0 / (2 * block) as f32;
        let mut frame = vec![0.0f32; 2 * block];
        let mut paths = Vec::new();
        for input in 0..lanes {
            for output in 0..lanes {
                let Some(h) = ir.path(input, output) else {
                    continue;
                };
                let head_taps = match head {
                    Head::Partitioned => Vec::new(),
                    Head::Direct => (0..block)
                        .map(|k| h.get(block - 1 - k).copied().unwrap_or(0.0))
                        .collect(),
                };
                let mut spectra = Split::zeroed(parts * bins);
                let mut spectrum = vec![Complex::default(); bins];
                for p in 0..parts {
                    frame.fill(0.0);
                    let start = (skip + p * block).min(h.len());
                    let end = (start + block).min(h.len());
                    for (f, &v) in frame.iter_mut().zip(&h[start..end]) {
                        *f = v * scale;
                    }
                    fft.forward_real(&frame, &mut spectrum);
                    spectra.store(p * bins, &spectrum);
                }
                paths.push(Path {
                    input,
                    output,
                    head: head_taps,
                    spectra,
                });
            }
        }
        Self {
            block,
            parts,
            lanes,
            head,
            ir_len,
            paths,
            fft,
            frames: vec![0.0; lanes * 2 * block],
            fdl: Split::zeroed(lanes * parts * bins),
            newest: 0,
            out: vec![0.0; lanes * 2 * block],
            spectrum: vec![Complex::default(); bins],
            acc: Split::zeroed(bins),
            history: match head {
                Head::Partitioned => Vec::new(),
                Head::Direct => vec![0.0; lanes * 2 * block],
            },
            pos: 0,
        }
    }

    /// The partition length `B`, in samples.
    pub const fn block(&self) -> usize {
        self.block
    }

    /// The partitions the frequency-domain part runs (`P`).
    pub const fn partitions(&self) -> usize {
        self.parts
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

    /// One sample in on every lane, one out: the output of the block
    /// being played out, plus the direct head, then the block step when
    /// the block fills.
    #[inline]
    fn advance(&mut self, x: [f32; 2]) -> [f32; 2] {
        let b = self.block;
        let pos = self.pos;
        let mut y = [0.0f32; 2];
        for lane in 0..self.lanes {
            y[lane] = self.out[lane * 2 * b + b + pos];
            self.frames[lane * 2 * b + b + pos] = x[lane];
        }
        if self.head == Head::Direct {
            for lane in 0..self.lanes {
                let h = &mut self.history[lane * 2 * b..(lane + 1) * 2 * b];
                h[pos] = x[lane];
                h[pos + b] = x[lane];
            }
            for path in &self.paths {
                let h = &self.history[path.input * 2 * b..(path.input + 1) * 2 * b];
                let window = &h[pos + 1..pos + 1 + b];
                y[path.output] += dot(window, &path.head);
            }
        }
        self.pos += 1;
        if self.pos == b {
            self.pos = 0;
            self.step();
        }
        y
    }

    /// The block step: transform each lane's frame into the delay line,
    /// sum the products per output, transform back.
    fn step(&mut self) {
        let parts = self.parts;
        if parts == 0 {
            return;
        }
        let b = self.block;
        let bins = b + 1;
        self.newest = (self.newest + 1) % parts;
        for lane in 0..self.lanes {
            let frame = &mut self.frames[lane * 2 * b..(lane + 1) * 2 * b];
            let at = (lane * parts + self.newest) * bins;
            self.fft.forward_real(frame, &mut self.spectrum);
            self.fdl.store(at, &self.spectrum);
            frame.copy_within(b.., 0);
        }
        for output in 0..self.lanes {
            self.acc.fill_zero();
            for path in self.paths.iter().filter(|p| p.output == output) {
                let line = path.input * parts * bins;
                for age in 0..parts {
                    let x = line + fdl_slot(self.newest, age, parts) * bins;
                    let h = age * bins;
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
            let out = &mut self.out[output * 2 * b..(output + 1) * 2 * b];
            self.fft.inverse_real(&mut self.spectrum, out);
        }
    }
}

impl Stage for Convolver {
    /// Clears the state. The plan does not depend on the rate: the
    /// response is taken at the prepared rate as given (resampling an
    /// [`ImpulseResponse`] to it is DSP.md F4).
    fn prepare(&mut self, _sample_rate: f32) {
        self.reset();
    }

    fn reset(&mut self) {
        self.frames.fill(0.0);
        self.fdl.fill_zero();
        self.out.fill(0.0);
        self.history.fill(0.0);
        self.newest = 0;
        self.pos = 0;
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
