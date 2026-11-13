//! **Oversampled2x** — run a stage in a clock domain twice as fast, and
//! [`Oversampled`], the same region at any whole factor with linear- or
//! minimum-phase filters (`general.rs`). `Oversampled2x` is the 2×/4×
//! form plugins build: one name, two linear-phase half-band designs
//! as construction presets — `front` (full-steepness pair guarding the
//! base Nyquist) and `nested` (the relaxed interior pair: inside a front
//! region the wideband content is already gone). The wrapper owns the
//! crossing: the interior is prepared at 2× (no stale-rate coefficients
//! possible), latency and tail are divided back to the outer rate.
//! A 4× stack is `Oversampled2x::front(Oversampled2x::nested(stage))`,
//! and its 66 + 18 = 84 sample latency is computed from that structure.

use crate::kernel::node::{Stage, Tail};
use crate::kernel::resample::{Oversampler2xFir, Oversampler2xFirInner};

mod general;
#[cfg(test)]
mod tests;

pub use general::Oversampled;

#[derive(Clone)]
enum Halfbands {
    Front(Oversampler2xFir),
    Nested(Oversampler2xFirInner),
}

/// Block chunk at the outer rate for [`Stage::process`]: the 2× interior
/// block lives on the stack, so the block form allocates nothing.
const CHUNK: usize = 64;

#[derive(Clone)]
pub struct Oversampled2x<S: Stage> {
    pub inner: S,
    os: Halfbands,
}

impl<S: Stage> Oversampled2x<S> {
    /// Full-steepness pair: flat to ~20 kHz at the base rate, steep
    /// skirt before decimation. The outermost region of a stack.
    pub fn front(inner: S) -> Self {
        Self {
            inner,
            os: Halfbands::Front(Oversampler2xFir::new()),
        }
    }

    /// Relaxed interior pair for nesting inside a `front` region.
    pub fn nested(inner: S) -> Self {
        Self {
            inner,
            os: Halfbands::Nested(Oversampler2xFirInner::new()),
        }
    }

    /// The clock-domain factor between outside and inside.
    pub const FACTOR: u32 = 2;

    /// This region's own round-trip group delay at ITS outer rate.
    fn own_latency(&self) -> u32 {
        match &self.os {
            // (133−1)/2 = 66 outer samples through the up+down pair.
            Halfbands::Front(_) => Oversampler2xFir::latency() as u32,
            // (73−1) samples at the inner rate = 36 at this outer.
            Halfbands::Nested(_) => (Oversampler2xFirInner::latency_base() * 2) as u32,
        }
    }
}

impl<S: Stage> Stage for Oversampled2x<S> {
    fn prepare(&mut self, sample_rate: f32) {
        self.reset_halfbands();
        // The region contract: the interior runs at FACTOR× and is
        // prepared at that rate.
        self.inner.prepare(sample_rate * Self::FACTOR as f32);
    }
    fn reset(&mut self) {
        self.reset_halfbands();
        self.inner.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_with(x, |inner, s| inner.tick(s))
    }
    /// Upsample a chunk, run the interior's block form once over the 2×
    /// chunk, decimate back. Bit-identical to the tick loop whenever the
    /// interior's `process` is (each half-band sees the same sequence).
    fn process(&mut self, block: &mut [f32]) {
        let mut up = [0.0f32; 2 * CHUNK];
        let inner = &mut self.inner;
        for chunk in block.chunks_mut(CHUNK) {
            match &mut self.os {
                Halfbands::Front(o) => o.process_with(chunk, &mut up, |b| inner.process(b)),
                Halfbands::Nested(o) => o.process_with(chunk, &mut up, |b| inner.process(b)),
            }
        }
    }
    fn latency(&self) -> u32 {
        let inner = self.inner.latency();
        debug_assert!(
            inner % Self::FACTOR == 0,
            "interior group delay must be integer at the outer rate ({inner}/2)"
        );
        self.own_latency() + inner / Self::FACTOR
    }
    /// The interior tail at the outer rate rounds up (a bound may be
    /// conservative), plus the pair's own ring-out.
    fn tail(&self) -> Tail {
        self.inner
            .tail()
            .div_ceil(Self::FACTOR)
            .plus(self.own_latency())
    }
}

impl<S: Stage> Oversampled2x<S> {
    /// One outer sample through a closure interior: `body` runs once
    /// per inner (2×) sample with the interior stage and that sample,
    /// so per-sample context (a smoothed drive, a mode) reaches the
    /// oversampled domain without being parked in the stage's fields.
    /// `tick` is `tick_with(x, |inner, s| inner.tick(s))`. A stateless
    /// interior is `()`, the wire:
    ///
    /// ```
    /// use stave_dsp::{Oversampled2x, Stage};
    ///
    /// let mut os = Oversampled2x::front(());
    /// os.prepare(48_000.0);
    /// let drive = 4.0;
    /// let y = os.tick_with(0.5, |(), s| (s * drive).tanh());
    /// assert!(y.is_finite());
    /// ```
    #[inline]
    pub fn tick_with(&mut self, x: f32, mut body: impl FnMut(&mut S, f32) -> f32) -> f32 {
        let inner = &mut self.inner;
        match &mut self.os {
            Halfbands::Front(o) => o.tick_with(x, |s| body(inner, s)),
            Halfbands::Nested(o) => o.tick_with(x, |s| body(inner, s)),
        }
    }

    fn reset_halfbands(&mut self) {
        match &mut self.os {
            Halfbands::Front(o) => o.reset(),
            Halfbands::Nested(o) => o.reset(),
        }
    }
}
