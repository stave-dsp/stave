//! **Node** — kernel form #1: the one processing
//! surface. Leaves and composites present the SAME surface (the
//! Cmajor duality); everything in `blocks/` either implements it or
//! contains things that do.

pub trait Stage {
    /// The rate `tick` will actually run at — inside an N× oversampler
    /// that is N× the host rate.
    fn prepare(&mut self, rate: f32);
    fn reset(&mut self);
    fn tick(&mut self, x: f32) -> f32;
    /// Block form. Default = the tick loop, so it is bit-identical by
    /// construction; overrides (block API) must stay bit-exact
    /// against it — each carries a pinning test. The payoff sites are
    /// containers (`Chain` in structures/ dispatches each stage
    /// once per block instead of once per sample), not the leaf math.
    fn process_block(&mut self, io: &mut [f32]) {
        for s in io {
            *s = self.tick(*s);
        }
    }
    /// Group delay in samples AT THE PREPARED RATE. Composites
    /// propagate it by the invariants: serial sums (`Chain`), parallel
    /// pads to the max ([`Par2`](crate::kernel::parallel::Par2)), regions
    /// divide. Leaves default to 0.
    fn latency_samples(&self) -> u32 {
        0
    }
    /// Ring-out after input stops, in samples at the prepared rate.
    /// `None` = unbounded (self-sustaining feedback). Leaves default
    /// to 0; composites combine by the tail invariants (Sequence sums,
    /// Branch maxes, Oversampled divides rounding up, Loop is honest).
    fn tail_samples(&self) -> Option<u32> {
        Some(0)
    }
}

impl<S: Stage + ?Sized> Stage for Box<S> {
    fn prepare(&mut self, rate: f32) {
        (**self).prepare(rate);
    }
    fn reset(&mut self) {
        (**self).reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        (**self).tick(x)
    }
    fn process_block(&mut self, io: &mut [f32]) {
        (**self).process_block(io);
    }
    fn latency_samples(&self) -> u32 {
        (**self).latency_samples()
    }
    fn tail_samples(&self) -> Option<u32> {
        (**self).tail_samples()
    }
}
