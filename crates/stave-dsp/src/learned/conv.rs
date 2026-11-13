//! Causal dilated 1-D convolution, one output frame per tick, and the
//! frame history it reads. Crate-private building blocks of
//! [`WaveNet`](super::WaveNet).
//!
//! PyTorch's `Conv1d` (valid, stride 1) with output aligned to the last
//! input: `y[t][o] = b[o] + Σ_k Σ_i w[o][i][k] · x[t − (K−1−k)·d][i]`.
//!
//! Outputs are computed [`LANES`] at a time into a fixed-size
//! accumulator that stays in registers; weights are stored in that
//! order, `[group][tap][input][lane]` (zero-padded past the last output),
//! so the loop streams them once and the compiler vectorises the lanes
//! without reassociating any sum. Each output still accumulates
//! `bias + Σ_k Σ_i` in the same order, whatever the width.

use super::error::Weights;

/// Outputs per accumulator block: two 128-bit or one 256-bit vector.
pub(crate) const LANES: usize = 8;

/// A convolution's weights: `taps` kernel taps `dilation` frames apart,
/// `inputs` → `outputs` channels, an optional bias.
#[derive(Clone, Debug)]
pub(crate) struct Conv {
    pub(crate) inputs: usize,
    pub(crate) outputs: usize,
    pub(crate) taps: usize,
    pub(crate) dilation: usize,
    /// `[group][tap][input][lane]`, `ceil(outputs / LANES)` groups.
    weights: Vec<f32>,
    /// `[group][lane]`, zeros when the convolution has no bias.
    bias: Vec<f32>,
}

impl Conv {
    /// Read PyTorch's layout from `w`: weight `(outputs, inputs, taps)`
    /// row-major, then `outputs` biases when `bias`.
    pub(crate) fn read(
        w: &mut Weights<'_>,
        inputs: usize,
        outputs: usize,
        taps: usize,
        dilation: usize,
        bias: bool,
    ) -> Self {
        let torch: Vec<f32> = w.take(outputs * inputs * taps).collect();
        let groups = outputs.div_ceil(LANES);
        let mut weights = vec![0.0; groups * taps * inputs * LANES];
        for o in 0..outputs {
            let (g, lane) = (o / LANES, o % LANES);
            for i in 0..inputs {
                for k in 0..taps {
                    weights[((g * taps + k) * inputs + i) * LANES + lane] =
                        torch[(o * inputs + i) * taps + k];
                }
            }
        }
        let mut padded = vec![0.0; groups * LANES];
        if bias {
            for (p, b) in padded.iter_mut().zip(w.take(outputs)) {
                *p = b;
            }
        }
        Self {
            inputs,
            outputs,
            taps,
            dilation,
            weights,
            bias: padded,
        }
    }

    /// Frames of history the convolution reads, the current one
    /// included: `(taps − 1) · dilation + 1`.
    pub(crate) fn span(&self) -> usize {
        (self.taps - 1) * self.dilation + 1
    }

    /// Multiply-adds per frame: `inputs · outputs · taps`.
    pub(crate) fn multiply_adds(&self) -> usize {
        self.inputs * self.outputs * self.taps
    }

    /// `out = bias + conv(history)` at the newest frame.
    #[inline]
    pub(crate) fn apply(&self, history: &History, out: &mut [f32]) {
        let (taps, d) = (self.taps, self.dilation);
        self.run(|k| history.ago((taps - 1 - k) * d), out, true);
    }

    /// `out = bias + W · x` for a one-tap convolution (a 1×1).
    #[inline]
    pub(crate) fn apply_now(&self, x: &[f32], out: &mut [f32]) {
        self.run(|_| x, out, true);
    }

    /// `out += W · x` for a one-tap convolution, bias ignored.
    #[inline]
    pub(crate) fn add_now(&self, x: &[f32], out: &mut [f32]) {
        self.run(|_| x, out, false);
    }

    /// `out = (bias or out) + Σ_k W_k · frame(k)`, `LANES` outputs at a
    /// time. `out` is [`padded`] (whole lane groups), so every group is
    /// read and written whole; lanes past `outputs` hold zeros.
    #[inline]
    fn run<'a>(&self, frame: impl Fn(usize) -> &'a [f32], out: &mut [f32], from_bias: bool) {
        let per_tap = self.inputs * LANES;
        let groups = self.bias.len() / LANES;
        let out = &mut out[..groups * LANES];
        for ((weights, bias), out) in self
            .weights
            .chunks_exact(self.taps * per_tap)
            .zip(self.bias.chunks_exact(LANES))
            .zip(out.chunks_exact_mut(LANES))
        {
            let mut acc = lanes(if from_bias { bias } else { out });
            for (k, tap) in weights.chunks_exact(per_tap).enumerate() {
                let x = &frame(k)[..self.inputs];
                for (column, &xi) in tap.chunks_exact(LANES).zip(x) {
                    acc = axpy(acc, lanes(column), xi);
                }
            }
            out.copy_from_slice(&acc);
        }
    }
}

/// The length of a buffer `n` channels wide that a [`Conv`] writes:
/// rounded up to whole lane groups.
pub(crate) fn padded(n: usize) -> usize {
    n.div_ceil(LANES) * LANES
}

/// A lane group by value.
#[inline(always)]
fn lanes(s: &[f32]) -> [f32; LANES] {
    let mut v = [0.0; LANES];
    v.copy_from_slice(&s[..LANES]);
    v
}

/// `acc + column · x`, lane by lane, on values (so the accumulator
/// lives in registers and the lanes vectorise).
#[inline(always)]
fn axpy(mut acc: [f32; LANES], column: [f32; LANES], x: f32) -> [f32; LANES] {
    for j in 0..LANES {
        acc[j] += column[j] * x;
    }
    acc
}

/// The last `frames` frames of a `width`-channel signal, newest last.
/// Allocated once; [`push`](Self::push) and [`ago`](Self::ago) are
/// O(width) and allocation-free.
#[derive(Clone, Debug)]
pub(crate) struct History {
    width: usize,
    frames: usize,
    /// `frames · width`, frame-major.
    data: Vec<f32>,
    /// Frame index of the newest frame.
    newest: usize,
}

impl History {
    pub(crate) fn new(width: usize, frames: usize) -> Self {
        let frames = frames.max(1);
        Self {
            width,
            frames,
            data: vec![0.0; width * frames],
            newest: 0,
        }
    }

    /// Append the newest frame, dropping the oldest.
    #[inline]
    pub(crate) fn push(&mut self, frame: &[f32]) {
        self.newest += 1;
        if self.newest == self.frames {
            self.newest = 0;
        }
        let at = self.newest * self.width;
        self.data[at..at + self.width].copy_from_slice(&frame[..self.width]);
    }

    /// Every frame set to `frame`: the history of a signal that has held
    /// that value forever.
    pub(crate) fn fill(&mut self, frame: &[f32]) {
        for chunk in self.data.chunks_exact_mut(self.width) {
            chunk.copy_from_slice(&frame[..self.width]);
        }
    }

    /// The frame `ago` frames before the newest (`0` is the newest);
    /// `ago` must be below the frame count.
    #[inline]
    pub(crate) fn ago(&self, ago: usize) -> &[f32] {
        let mut i = self.newest + self.frames - ago;
        if i >= self.frames {
            i -= self.frames;
        }
        &self.data[i * self.width..(i + 1) * self.width]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolution_reads_torch_layout_and_taps_back_in_time() {
        // 2 in, 1 out, 2 taps, dilation 3: y = w[0][i][0]·x[t−3][i] + w[0][i][1]·x[t][i] + b.
        let raw = [1.0, 2.0, 10.0, 20.0, 0.5];
        let mut w = Weights::new(&raw);
        let conv = Conv::read(&mut w, 2, 1, 2, 3, true);
        w.finish().unwrap();
        assert_eq!(conv.span(), 4);
        let mut h = History::new(2, conv.span());
        let mut out = [0.0; LANES];
        for t in 0..6 {
            h.push(&[t as f32, 100.0 * t as f32]);
            conv.apply(&h, &mut out);
            // The history starts as zeros.
            let then = if t >= 3 { (t - 3) as f32 } else { 0.0 };
            // Raw order (o, i, k): input 0 taps (1, 2), input 1 taps (10, 20).
            let expect =
                1.0 * then + 2.0 * t as f32 + 10.0 * 100.0 * then + 20.0 * 100.0 * t as f32 + 0.5;
            assert_eq!(out[0], expect, "t={t}");
        }
    }

    #[test]
    fn short_weight_lists_are_counted() {
        let raw = [1.0; 3];
        let mut w = Weights::new(&raw);
        let _ = Conv::read(&mut w, 2, 2, 1, 1, false);
        assert_eq!(
            w.finish(),
            Err(crate::learned::LoadError::Weights {
                expected: 4,
                found: 3
            })
        );
    }
}
