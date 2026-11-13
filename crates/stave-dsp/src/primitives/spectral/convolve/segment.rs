//! One uniform run of a partition schedule: its partitions' spectra,
//! frequency-domain delay line and transforms.

use super::super::fft::{Complex, Fft};
use super::super::impulse_response::ImpulseResponse;
use super::{fdl_slot, swap};
use super::schedule::Span;

/// One input → output path's partitions within a [`Segment`].
#[derive(Clone, Debug)]
pub(crate) struct SegmentPath {
    pub(crate) input: usize,
    pub(crate) output: usize,
    /// `parts` partition spectra of `size + 1` bins, scaled by
    /// `1/2·size` so the unnormalised inverse lands at unity.
    pub(crate) spectra: Split,
}

/// Spectra stored as separate real and imaginary arrays, so the
/// multiply-accumulate over the delay line runs on four flat `f32`
/// slices (which the compiler vectorises).
#[derive(Clone, Debug)]
pub(crate) struct Split {
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Split {
    pub(crate) fn zeroed(len: usize) -> Self {
        Self {
            re: vec![0.0; len],
            im: vec![0.0; len],
        }
    }

    pub(crate) fn store(&mut self, at: usize, bins: &[Complex<f32>]) {
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

/// One uniform partitioned run of a [`Span`]: its transform, input
/// frames, delay line and output.
#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub(crate) size: usize,
    /// Where this run's partitions start in the response, in taps.
    pub(crate) offset: usize,
    skip: usize,
    pub(crate) parts: usize,
    pub(crate) paths: Vec<SegmentPath>,
    pub(crate) fft: Fft<f32>,
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
    pub(crate) spectrum: Vec<Complex<f32>>,
    /// The sum over the delay line, split form.
    acc: Split,
    /// Position in the current block, `0..L`.
    pos: usize,
    /// A response swap ([`Convolver::swap_response`](super::Convolver::swap_response)):
    /// empty until [`Convolver::reserve_swaps`](super::Convolver::reserve_swaps).
    pub(crate) swap: swap::SegmentSwap,
}

impl Segment {
    pub(crate) fn new(ir: &ImpulseResponse, span: Span) -> Self {
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

    pub(crate) fn reset(&mut self) {
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
    pub(crate) fn advance(&mut self, x: [f32; 2], y: &mut [f32; 2], lanes: usize) {
        let l = self.size;
        let pos = self.pos;
        for lane in 0..lanes {
            y[lane] += self.out[lane * 2 * l + l + pos];
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
    pub(crate) fn step(&mut self, lanes: usize) {
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
            // The old response's block, then the new one's, blended in
            // place: the block played out fades between them, and the
            // per-sample path never looks at the swap.
            self.convolve(lanes);
            self.swap.keep_old_block(&mut self.out);
            self.swap.install(&mut self.paths);
            self.convolve(lanes);
            self.swap.blend_block(&mut self.out, lanes, l);
        } else {
            self.convolve(lanes);
        }
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

/// A deferred run of a [`Convolver`](super::Convolver)'s schedule: the
/// arithmetic of one partition size, computed a block ahead by a
/// [`Worker`](super::Worker), on whatever thread it chooses.
///
/// Each [`step`](Run::step) takes the run's newest input block and
/// returns the output block the convolver plays from the next boundary
/// on: the run's schedule keeps one block of slack (`skip ≥ 1`), so that
/// block never needs the input that has just arrived, and whoever
/// computes it has one block period to do so (Battenberg & Avizienis
/// 2011's deadline). A run allocates nothing after construction.
#[derive(Clone, Debug)]
pub struct Run {
    segment: Segment,
    lanes: usize,
}

impl Run {
    /// The run of `span` (whose `skip ≥ 1`), stepping one block early:
    /// its own delay line keeps one slot less.
    pub(crate) fn new(ir: &ImpulseResponse, span: Span) -> Self {
        debug_assert!(span.skip >= 1, "a deferred run keeps a block of slack");
        Self {
            segment: Segment::new(
                ir,
                Span {
                    skip: span.skip - 1,
                    ..span
                },
            ),
            lanes: ir.layout().lanes(),
        }
    }

    /// The partition size `L`: the run's block, period and deadline, in
    /// samples.
    pub fn size(&self) -> usize {
        self.segment.size
    }

    /// The lanes a block holds (1 mono, 2 stereo).
    pub fn lanes(&self) -> usize {
        self.lanes
    }

    /// The samples in one block: `lanes · size`.
    pub fn block_len(&self) -> usize {
        self.lanes * self.segment.size
    }

    /// One block: `input` is the newest `size` samples of each lane, lane
    /// after lane; `output` receives, the same way, the block to play
    /// from the next boundary. Both are [`block_len`](Run::block_len)
    /// long. Never allocates.
    pub fn step(&mut self, input: &[f32], output: &mut [f32]) {
        let l = self.segment.size;
        for lane in 0..self.lanes {
            let frame = &mut self.segment.frames[lane * 2 * l + l..(lane + 1) * 2 * l];
            frame.copy_from_slice(&input[lane * l..(lane + 1) * l]);
        }
        self.segment.step(self.lanes);
        for lane in 0..self.lanes {
            let out = &self.segment.out[lane * 2 * l + l..(lane + 1) * 2 * l];
            output[lane * l..(lane + 1) * l].copy_from_slice(out);
        }
    }

    /// Forget every input (the convolver's `reset`).
    pub fn reset(&mut self) {
        self.segment.reset();
    }
}
