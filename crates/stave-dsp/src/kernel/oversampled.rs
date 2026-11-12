//! **Oversampled** — kernel form #5: run a node in
//! another clock domain. ONE name, two half-band designs as
//! construction presets: `front` (full-steepness pair guarding the
//! base Nyquist) and `nested` (the relaxed interior pair — inside a
//! front region the wideband content is already gone). The wrapper
//! owns the crossing: interior prepared at 2× (no stale-rate
//! coefficients possible), latency divided back to the outer rate.
//! Forge's 4× stack is `Oversampled2x::front(Oversampled2x::nested(chain))`
//! and its 66+18=84 latency is computed from this structure.

use crate::kernel::node::Stage;
use crate::kernel::resample::{Oversampler2xFir, Oversampler2xFirInner};

enum Halfbands {
    Front(Oversampler2xFir),
    Nested(Oversampler2xFirInner),
}

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
            Halfbands::Front(_) => Oversampler2xFir::latency_samples() as u32,
            // (73−1) samples at the inner rate = 36 at this outer.
            Halfbands::Nested(_) => (Oversampler2xFirInner::latency_samples_base() * 2) as u32,
        }
    }
}

impl<S: Stage> Stage for Oversampled2x<S> {
    fn prepare(&mut self, rate: f32) {
        match &mut self.os {
            Halfbands::Front(o) => o.prepare(rate),
            Halfbands::Nested(o) => o.prepare(rate),
        }
        // The region contract: the interior runs at FACTOR× and is
        // prepared at that rate.
        self.inner.prepare(rate * Self::FACTOR as f32);
    }
    fn reset(&mut self) {
        match &mut self.os {
            Halfbands::Front(o) => o.reset(),
            Halfbands::Nested(o) => o.reset(),
        }
        self.inner.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let inner = &mut self.inner;
        match &mut self.os {
            Halfbands::Front(o) => o.process_one(x, |s| inner.tick(s)),
            Halfbands::Nested(o) => o.process_one(x, |s| inner.tick(s)),
        }
    }
    fn latency_samples(&self) -> u32 {
        let inner = self.inner.latency_samples();
        debug_assert!(
            inner % Self::FACTOR == 0,
            "interior group delay must be integer at the outer rate ({inner}/2)"
        );
        self.own_latency() + inner / Self::FACTOR
    }
    fn tail_samples(&self) -> Option<u32> {
        // Interior tail at the outer rate rounds UP (a bound may be
        // conservative), plus the pair's own ring-out.
        Some(self.own_latency() + self.inner.tail_samples()?.div_ceil(Self::FACTOR))
    }
}
