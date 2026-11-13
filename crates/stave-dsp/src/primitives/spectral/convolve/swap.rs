//! Swapping a mono [`Convolver`]'s response while it runs, click-free:
//! the new response's partitions are transformed into buffers reserved
//! beforehand, and each run crossfades from the old response's output
//! to the new one's over its next block (the direct head over its `B`
//! samples). The input's delay line is shared, so the new response sees
//! the whole history at once: after the fade the output is exactly the
//! new convolution, with no settling (Wefers 2015, §5.5, filter
//! exchange in the frequency domain with an output crossfade).

use super::{dot, Convolver, Head, HeadPath, Segment, SegmentPath, Split};

/// How a convolver's direct head runs: not at all, plain, or through a
/// swap's crossfade.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum HeadMode {
    Direct,
    Swapping,
    Partitioned,
}

impl HeadMode {
    pub(super) fn of(head: Head) -> Self {
        match head {
            Head::Direct => HeadMode::Direct,
            Head::Partitioned => HeadMode::Partitioned,
        }
    }
}

/// Where a run is in a swap.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum State {
    /// No swap pending.
    #[default]
    Idle,
    /// The new partitions are staged; the next block step starts the
    /// fade.
    Staged,
    /// The block being played out fades from the old response's output
    /// to the new one's.
    Fading,
}

/// One run's side of a swap: the staged partitions and the old
/// response's output block during the fade.
#[derive(Clone, Debug, Default)]
pub(super) struct SegmentSwap {
    staged: Option<Split>,
    /// `2L` samples: the old response's last block during a fade, and
    /// the scratch frame for staging otherwise.
    old: Vec<f32>,
    state: State,
}

impl SegmentSwap {
    fn reserve(&mut self, spectra: usize, frame: usize) {
        self.staged = Some(Split::zeroed(spectra));
        self.old = vec![0.0; frame];
        self.state = State::Idle;
    }

    pub(super) fn is_reserved(&self) -> bool {
        self.staged.is_some()
    }

    /// Blends the block about to be played out (the second half of
    /// each lane's `2L` output) from the old response's, kept in `old`,
    /// to the new one's in `out`: a linear ramp ending all new.
    pub(super) fn blend_block(&self, out: &mut [f32], lanes: usize, l: usize) {
        for lane in 0..lanes {
            let at = lane * 2 * l + l;
            let (new, old) = (&mut out[at..at + l], &self.old[at..at + l]);
            for (p, (n, &o)) in new.iter_mut().zip(old).enumerate() {
                let r = (p + 1) as f32 / l as f32;
                *n = o + (*n - o) * r;
            }
        }
    }

    /// At a block step: `true` when a staged swap starts fading now; a
    /// finished fade returns to idle.
    pub(super) fn begins_fade(&mut self) -> bool {
        match self.state {
            State::Staged => {
                self.state = State::Fading;
                true
            }
            State::Fading => {
                self.state = State::Idle;
                false
            }
            State::Idle => false,
        }
    }

    /// Keeps the old response's block (just computed into `out`) for the
    /// fade; `out` is overwritten by the next convolution.
    pub(super) fn keep_old_block(&mut self, out: &mut Vec<f32>) {
        core::mem::swap(out, &mut self.old);
    }

    /// Makes the staged partitions the run's response.
    pub(super) fn install(&mut self, paths: &mut [SegmentPath]) {
        if let (Some(staged), Some(path)) = (self.staged.as_mut(), paths.first_mut()) {
            core::mem::swap(&mut path.spectra, staged);
        }
    }

    /// Ends any swap at once (a reset): a staged response becomes the
    /// current one.
    pub(super) fn finish(&mut self, paths: &mut [SegmentPath]) {
        if self.state == State::Staged {
            self.install(paths);
        }
        self.state = State::Idle;
    }
}

/// The direct head's side of a swap: the new head taps, reversed, and
/// how far the fade is.
#[derive(Clone, Debug, Default)]
pub(super) struct HeadSwap {
    staged: Vec<f32>,
    /// Staged, waiting for the first partitioned run's fade to start,
    /// so the head and that run fade over the same block.
    waiting: bool,
    /// Samples into the fade, `1..=B`; 0 when idle.
    fade: usize,
}

impl HeadSwap {
    /// `true` while the head is waiting to fade or fading.
    #[inline]
    pub(super) fn is_active(&self) -> bool {
        self.waiting || self.fade != 0
    }

    pub(super) fn is_reserved(&self) -> bool {
        !self.staged.is_empty()
    }

    /// The head's output for one window: blended during a fade.
    #[inline]
    pub(super) fn dot(&self, window: &[f32], taps: &[f32]) -> f32 {
        let old = dot(window, taps);
        if self.fade == 0 {
            return old;
        }
        let r = self.fade as f32 / self.staged.len() as f32;
        old + (dot(window, &self.staged) - old) * r
    }

    /// One sample on: starts the fade with the first run's (`run_fading`),
    /// installs the new taps when it completes.
    #[inline]
    pub(super) fn advance(&mut self, heads: &mut [HeadPath], run_fading: bool) {
        if self.waiting && run_fading {
            self.waiting = false;
            self.fade = 1;
            return;
        }
        if self.fade == 0 {
            return;
        }
        self.fade += 1;
        if self.fade > self.staged.len() {
            self.finish(heads);
        }
    }

    pub(super) fn finish(&mut self, heads: &mut [HeadPath]) {
        if self.fade != 0 || self.waiting {
            if let Some(head) = heads.first_mut() {
                core::mem::swap(&mut head.taps, &mut self.staged);
            }
        }
        self.fade = 0;
        self.waiting = false;
    }
}

impl Segment {
    /// `true` while this run's output block is fading.
    pub(super) fn is_fading(&self) -> bool {
        self.swap.state == State::Fading
    }

    /// Transforms `taps`' share of this run into the staged partitions.
    fn stage(&mut self, taps: &[f32]) {
        let size = self.size;
        let bins = size + 1;
        let scale = 1.0 / (2 * size) as f32;
        let Some(staged) = self.swap.staged.as_mut() else {
            return;
        };
        let frame = &mut self.swap.old[..2 * size];
        for p in 0..self.parts {
            frame.fill(0.0);
            let start = (self.offset + p * size).min(taps.len());
            let end = (start + size).min(taps.len());
            for (f, &v) in frame.iter_mut().zip(&taps[start..end]) {
                *f = v * scale;
            }
            self.fft.forward_real(frame, &mut self.spectrum);
            staged.store(p * bins, &self.spectrum);
        }
        self.swap.state = State::Staged;
    }
}

impl Convolver {
    /// The direct head for one sample while its swap is waiting or
    /// fading (kept off the steady-state path); back to the plain head
    /// when the fade completes.
    #[inline(always)]
    pub(super) fn head_swapping(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut y = [0.0f32; 2];
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
        if !self.head_swap.is_active() {
            self.head_mode = HeadMode::Direct;
        }
        self.pos = (pos + 1) & (b - 1);
        y
    }

    /// Reserves the buffers [`swap_response`](Self::swap_response)
    /// needs: one more set of partition spectra and one output block per
    /// run (this allocates; call it before the audio starts). A
    /// [`prepare`](crate::Stage::prepare) that re-plans keeps the
    /// reservation.
    ///
    /// # Panics
    ///
    /// Unless the response is mono (one lane, one path).
    pub fn reserve_swaps(&mut self) {
        assert!(
            self.lanes == 1 && self.segments.iter().all(|s| s.paths.len() <= 1),
            "Convolver::reserve_swaps: only a mono response can be swapped"
        );
        for segment in &mut self.segments {
            let spectra = segment.parts * (segment.size + 1);
            segment.swap.reserve(spectra, 2 * segment.size);
        }
        if !self.heads.is_empty() {
            self.head_swap.staged = vec![0.0; self.block];
            self.head_swap.fade = 0;
        }
    }

    /// `true` while a swap is staged or fading.
    pub fn is_swapping(&self) -> bool {
        self.head_swap.fade != 0 || self.head_swap.waiting || self.segments.iter().any(|s| s.swap.state != State::Idle)
    }

    /// Replaces the (mono) response with `taps`, zero-padded to the
    /// planned length, crossfading over each run's next block: the
    /// direct head with the first partitioned run, every run over the
    /// block after its next step. Once the fade is over, the output
    /// is the new convolution of the whole input history, as a convolver
    /// built with `taps` would give.
    ///
    /// Returns `false`, changing nothing, when no swap was reserved
    /// ([`reserve_swaps`](Self::reserve_swaps)), a swap is still under
    /// way ([`is_swapping`](Self::is_swapping)), or `taps` is longer than
    /// the planned response.
    ///
    /// Realtime: never allocates; costs one forward transform per
    /// partition, so a long response is better swapped at control rate.
    pub fn swap_response(&mut self, taps: &[f32]) -> bool {
        let reserved = self.segments.iter().all(|s| s.swap.is_reserved())
            && (self.heads.is_empty() || self.head_swap.is_reserved());
        if !reserved || self.is_swapping() || taps.len() > self.ir_len.max(self.block) {
            return false;
        }
        for segment in &mut self.segments {
            segment.stage(taps);
        }
        if !self.heads.is_empty() {
            let b = self.block;
            for (k, t) in self.head_swap.staged.iter_mut().enumerate() {
                *t = taps.get(b - 1 - k).copied().unwrap_or(0.0);
            }
            if self.segments.is_empty() {
                self.head_swap.fade = 1;
            } else {
                self.head_swap.waiting = true;
            }
            self.head_mode = HeadMode::Swapping;
        }
        true
    }
}
