//! **Loop** — kernel form #4: feedback around a
//! node, ONLY through an explicit delay (the gen~/Faust `~` rule).
//! The zero-delay cycle is unwritable: the delay is `NonZeroU32` in
//! the constructor signature, not a runtime check.
//!
//! The loop owns structure, never voicing: `y = node(x + delayed_y)`.
//! Feedback gain, damping, saturation-in-the-loop all belong to the
//! node itself (compose them with `Serial` inside) — a delay's
//! regeneration, an amp's NFB, and a reverb's recirculation are the
//! same shape with different nodes.
//!
//! Tail is honest by default: a feedback loop cannot know its own
//! stability, so it reports unbounded unless the project declares a
//! measured bound (`with_tail_bound`). Conservative beats wrong.

use core::num::NonZeroU32;

use crate::kernel::node::Stage;
use crate::kernel::ring::Ring;

pub struct Loop<S: Stage> {
    pub node: S,
    ring: Ring,
    delay: NonZeroU32,
    tail_bound: Option<u32>,
}

impl<S: Stage> Loop<S> {
    pub fn new(node: S, delay: NonZeroU32) -> Self {
        Self {
            node,
            ring: Ring::new(),
            delay,
            tail_bound: None,
        }
    }

    /// Declare a measured decay bound in samples (a project claim,
    /// pinned by that project's tests — the form cannot derive it).
    pub fn with_tail_bound(mut self, samples: u32) -> Self {
        self.tail_bound = Some(samples);
        self
    }

    pub const fn delay(&self) -> NonZeroU32 {
        self.delay
    }
}

impl<S: Stage> Stage for Loop<S> {
    fn prepare(&mut self, rate: f32) {
        self.node.prepare(rate);
        self.ring.prepare(self.delay.get() as usize);
    }
    fn reset(&mut self) {
        self.node.reset();
        self.ring.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let fb = self.ring.delayed();
        let y = self.node.tick(x + fb);
        // One non-finite sample must not recirculate forever: the
        // history stores 0 in its place (finite values bit-exact).
        self.ring.push(crate::primitives::denormal::finite_or_zero(y));
        y
    }
    fn latency_samples(&self) -> u32 {
        // The feed-forward path passes the node once; the loop adds
        // history, not input-to-output delay.
        self.node.latency_samples()
    }
    fn tail_samples(&self) -> Option<u32> {
        self.tail_bound
    }
}
