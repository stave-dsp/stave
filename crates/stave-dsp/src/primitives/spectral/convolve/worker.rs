//! Who computes a convolver's deferred runs: the [`Worker`] contract and
//! the [`Inline`] worker that computes them on the calling thread.

use super::segment::Run;

/// Computes a [`Convolver`](super::Convolver)'s deferred [`Run`]s: one
/// block of each run per block period, a period ahead of when the
/// convolver plays it.
///
/// [`Inline`] computes on the calling thread at the boundary (the same
/// output, block for block, as a threaded worker that meets every
/// deadline — the deterministic reference). A host-side crate supplies
/// threaded workers (stave-core's `workers::Workers`: one thread per
/// partition size, a lock-free hand-off); stave-dsp itself stays free of
/// threads and atomics.
///
/// The contract: [`exchange`](Worker::exchange) runs on the audio thread
/// and must not allocate, lock or block (except a worker that documents
/// an offline mode that waits); [`assign`](Worker::assign),
/// [`prepare`](Worker::prepare) and [`reset`](Worker::reset) run where
/// the convolver's own `prepare` and `reset` do.
pub trait Worker {
    /// Take over `runs`, the convolver's deferred runs, run `i` at index
    /// `i`: at [`Convolver::with_worker`](super::Convolver::with_worker)
    /// and whenever `prepare` re-plans. May allocate, spawn and block.
    fn assign(&mut self, runs: Vec<Run>);

    /// The host's sample rate, before the first exchange (for deadlines
    /// in seconds). The default ignores it.
    fn prepare(&mut self, _sample_rate: f32) {}

    /// Forget every input and every block in flight; the next exchange
    /// of each run plays silence.
    fn reset(&mut self);

    /// Run `index`'s block boundary: take `input` (its newest block,
    /// [`Run::block_len`] samples), and put into `output` the block
    /// computed from the previous boundary's input. Buffers are swapped,
    /// not copied: both stay allocated, and the caller overwrites
    /// `input` before its next boundary.
    ///
    /// Returns `false` when that block was not ready in time: `output`
    /// is then silence (the worker's defined degradation), and the
    /// convolver counts it ([`Convolver::late_blocks`](super::Convolver::late_blocks)).
    fn exchange(&mut self, index: usize, input: &mut Box<[f32]>, output: &mut Box<[f32]>) -> bool;
}

/// The [`Worker`] that computes every deferred run on the calling
/// thread, at its boundary: never late, and the reference a threaded
/// worker is held to (bit for bit, when it meets its deadlines).
#[derive(Clone, Debug, Default)]
pub struct Inline {
    runs: Vec<Run>,
    /// Per run, the block computed at the last boundary.
    pending: Vec<Box<[f32]>>,
}

impl Inline {
    /// No runs yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Give the runs back (to hand them to another worker).
    pub(crate) fn take(&mut self) -> Vec<Run> {
        self.pending.clear();
        core::mem::take(&mut self.runs)
    }
}

impl Worker for Inline {
    fn assign(&mut self, runs: Vec<Run>) {
        self.pending = runs
            .iter()
            .map(|r| vec![0.0; r.block_len()].into_boxed_slice())
            .collect();
        self.runs = runs;
    }

    fn reset(&mut self) {
        for run in &mut self.runs {
            run.reset();
        }
        for block in &mut self.pending {
            block.fill(0.0);
        }
    }

    #[inline]
    fn exchange(&mut self, index: usize, input: &mut Box<[f32]>, output: &mut Box<[f32]>) -> bool {
        core::mem::swap(output, &mut self.pending[index]);
        self.runs[index].step(input, &mut self.pending[index]);
        true
    }
}
