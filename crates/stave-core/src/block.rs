//! Block scaffolding: the per-frame plumbing every engine used
//! to hand-roll — channel counting and clamping, lane-sliced reads
//! and writes, zeroing of undriven outputs, and block peak metering
//! — owned HERE, once. Engines keep the decisions (what to do at
//! each lane count); the framework keeps the mechanics.
//!
//! ```ignore
//! let lanes = ctx.lanes::<2>();
//! match lanes.active() {
//!     2 => {
//!         let peaks = lanes.run(|fr| {
//!             let (l, r) = (fr.input(0), fr.input(1));
//!             // ... the actual DSP ...
//!             fr.write(0, out_l);
//!             fr.write(1, out_r);
//!         });
//!     }
//!     _ => { /* the engine's own fallback */ }
//! }
//! ```

use core::marker::PhantomData;

use crate::process::ProcessCtx;

/// Block peaks, measured by the scaffolding: `input` is the raw
/// per-frame peak over the active input lanes (before the engine
/// touches anything); `output` is the peak of everything written.
#[derive(Copy, Clone, Debug, Default)]
pub struct Peaks {
    pub input: f32,
    pub output: f32,
}

/// Lane-sliced access to one block. Constructed by
/// [`ProcessCtx::lanes`]; undriven output channels are zeroed at
/// construction, so an early return can never leak stale samples.
pub struct Lanes<'c, const MAX: usize> {
    frames: usize,
    active: usize,
    /// Lanes with a REAL input behind them (< `active` for
    /// generators — an instrument's inputs read silence).
    in_active: usize,
    in_ptrs: [*const f32; MAX],
    out_ptrs: [*mut f32; MAX],
    _borrows_ctx: PhantomData<&'c mut ()>,
}

/// One frame's view: read the inputs, write the outputs. Writes are
/// peak-metered as they happen.
pub struct LaneFrame<'l, const MAX: usize> {
    lanes: &'l Lanes<'l, MAX>,
    index: usize,
    peak_out: &'l mut f32,
}

impl<'a> ProcessCtx<'a> {
    /// The block's lane view, at most `MAX` lanes (the engine's own
    /// capacity). For an EFFECT, active = min(inputs, outputs, MAX);
    /// for a GENERATOR (zero inputs — instruments) the lanes are
    /// OUTPUT-driven: active = min(outputs, MAX) and `input()` reads
    /// silence (the chime consumer forced this: min(0, out) locked
    /// every instrument out of its own outputs). Output channels
    /// beyond the active set are zeroed here and now.
    pub fn lanes<const MAX: usize>(&mut self) -> Lanes<'_, MAX> {
        let frames = self.frames;
        let in_chans = self.input.channel_count();
        let out_chans = self.output.channel_count();
        let active = if in_chans == 0 {
            out_chans.min(MAX)
        } else {
            in_chans.min(out_chans).min(MAX)
        };
        let in_active = in_chans.min(active);
        // Undriven outputs are silence, unconditionally.
        for c in active..out_chans {
            self.output.channel(c)[..frames].fill(0.0);
        }
        let mut in_ptrs = [core::ptr::null::<f32>(); MAX];
        let mut out_ptrs = [core::ptr::null_mut::<f32>(); MAX];
        for l in 0..active {
            if l < in_active {
                in_ptrs[l] = self.input.channel_ref(l).as_ptr();
            }
            out_ptrs[l] = self.output.channel(l).as_mut_ptr();
        }
        Lanes {
            frames,
            active,
            in_active,
            in_ptrs,
            out_ptrs,
            _borrows_ctx: PhantomData,
        }
    }
}

impl<'c, const MAX: usize> Lanes<'c, MAX> {
    /// How many lanes are live this block (never more than `MAX`).
    #[inline]
    pub fn active(&self) -> usize {
        self.active
    }
    #[inline]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Run `body` once per frame with lane access, then return the
    /// block peaks. The raw input peak is measured over every active
    /// lane whether or not the body reads it.
    #[inline]
    pub fn run(self, mut body: impl FnMut(&mut LaneFrame<'_, MAX>)) -> Peaks {
        let mut peaks = Peaks::default();
        for f in 0..self.frames {
            for l in 0..self.in_active {
                // SAFETY: `in_ptrs[l]` came from a live channel slice
                // of at least `frames` samples (AudioBuffer::from_raw
                // contract); `f < frames`, `l < in_active`.
                let x = unsafe { *self.in_ptrs[l].add(f) };
                peaks.input = peaks.input.max(x.abs());
            }
            let mut frame = LaneFrame {
                lanes: &self,
                index: f,
                peak_out: &mut peaks.output,
            };
            body(&mut frame);
        }
        peaks
    }
}

impl<const MAX: usize> LaneFrame<'_, MAX> {
    /// This frame's position in the block (scopes, schedules).
    #[inline]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The input sample on `lane`. Panics if the lane is not active
    /// — branch on [`Lanes::active`] first; lane count is the
    /// engine's decision.
    #[inline]
    pub fn input(&self, lane: usize) -> f32 {
        assert!(lane < self.lanes.active, "lane {lane} is not active");
        if lane >= self.lanes.in_active {
            return 0.0; // a generator's inputs are silence
        }
        // SAFETY: bounds asserted; pointer validity per from_raw.
        unsafe { *self.lanes.in_ptrs[lane].add(self.index) }
    }

    /// Write the output sample on `lane` (peak-metered).
    #[inline]
    pub fn write(&mut self, lane: usize, value: f32) {
        assert!(lane < self.lanes.active, "lane {lane} is not active");
        *self.peak_out = self.peak_out.max(value.abs());
        // SAFETY: bounds asserted; pointer validity + no-alias per
        // from_raw; the ctx borrow held by Lanes keeps these live.
        unsafe { *self.lanes.out_ptrs[lane].add(self.index) = value };
    }
}
