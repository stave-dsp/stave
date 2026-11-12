//! **Branch** — kernel form #3: split → parallel
//! paths → join, with the alignment invariant owned here: branches arrive
//! at the join together (`Compensator` pads computed from reported
//! latencies). Split/join math is caller-owned.
//!
use crate::kernel::node::Stage;
use crate::kernel::ring::Ring;

/// Latency compensation as a SERVICE: the realization of the Branch
/// invariant (`stave-core latency.rs` `parallel().pad`) and of hand-rolled
/// dry paths (Forge, multiband saturation). A fixed INTEGER delay by type —
/// zero samples is a true passthrough, storage untouched.
pub struct Compensator {
    ring: Ring,
    samples: u32,
}

impl Default for Compensator {
    fn default() -> Self {
        Self::new()
    }
}

impl Compensator {
    pub const fn new() -> Self {
        Self {
            ring: Ring::new(),
            samples: 0,
        }
    }

    /// Size for a pad (allocates — message thread). Re-preparing
    /// flushes.
    pub fn prepare(&mut self, samples: u32) {
        self.samples = samples;
        if samples > 0 {
            self.ring.prepare(samples as usize);
        }
    }

    pub const fn samples(&self) -> u32 {
        self.samples
    }

    pub fn reset(&mut self) {
        if self.samples > 0 {
            self.ring.reset();
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        if self.samples == 0 {
            return x;
        }
        let y = self.ring.delayed();
        self.ring.push(x);
        y
    }
}

/// Parallel pair with automatic latency balance. Dry/wet, M/S, and
/// crossover splits are all this shape: the SPLIT and the JOIN are
/// the caller's math (an identity copy, a sum/difference matrix,
/// complementary filters); the combinator owns what a graph owes any
/// parallel composition — both branches arrive at the join aligned,
/// whatever either branch reports.
pub struct Par2<A: Stage, B: Stage> {
    pub a: A,
    pub b: B,
    pad_a: Compensator,
    pad_b: Compensator,
}

impl<A: Stage, B: Stage> Par2<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self {
            a,
            b,
            pad_a: Compensator::new(),
            pad_b: Compensator::new(),
        }
    }

    /// Both branch outputs for one input pair, join-aligned. The
    /// caller splits before and joins after — this is the whole
    /// parallel contract in one call.
    #[inline]
    pub fn tick_pair(&mut self, xa: f32, xb: f32) -> (f32, f32) {
        (
            self.pad_a.tick(self.a.tick(xa)),
            self.pad_b.tick(self.b.tick(xb)),
        )
    }
}

impl<A: Stage, B: Stage> Stage for Par2<A, B> {
    fn prepare(&mut self, rate: f32) {
        self.a.prepare(rate);
        self.b.prepare(rate);
        // The parallel invariant (stave-core latency.rs): the join is the
        // slower branch; the faster one is padded to meet it.
        let (la, lb) = (self.a.latency_samples(), self.b.latency_samples());
        let join = la.max(lb);
        self.pad_a.prepare(join - la);
        self.pad_b.prepare(join - lb);
    }
    fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
        self.pad_a.reset();
        self.pad_b.reset();
    }
    /// `Stage::tick` view: fan one input to both branches and sum the
    /// aligned outputs (the dry/wet and crossover join). For matrix
    /// splits/joins (M/S), use [`tick_pair`](Self::tick_pair).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (ya, yb) = self.tick_pair(x, x);
        ya + yb
    }
    fn latency_samples(&self) -> u32 {
        self.a.latency_samples().max(self.b.latency_samples())
    }
    fn tail_samples(&self) -> Option<u32> {
        Some(self.a.tail_samples()?.max(self.b.tail_samples()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::ring::Ring;

    /// A test stage with declared latency: a pure integer delay.
    struct Late {
        ring: Ring,
        samples: u32,
    }
    impl Late {
        fn new(samples: u32) -> Self {
            Self {
                ring: Ring::new(),
                samples,
            }
        }
    }
    impl Stage for Late {
        fn prepare(&mut self, _rate: f32) {
            if self.samples > 0 {
                self.ring.prepare(self.samples as usize);
            }
        }
        fn reset(&mut self) {
            self.ring.reset();
        }
        fn tick(&mut self, x: f32) -> f32 {
            if self.samples == 0 {
                return x;
            }
            let y = self.ring.delayed();
            self.ring.push(x);
            y
        }
        fn latency_samples(&self) -> u32 {
            self.samples
        }
    }

    /// A gain leaf — zero latency, distinguishable output.
    struct Gain(f32);
    impl Stage for Gain {
        fn prepare(&mut self, _rate: f32) {}
        fn reset(&mut self) {}
        fn tick(&mut self, x: f32) -> f32 {
            x * self.0
        }
    }

    #[test]
    fn par2_auto_balances_the_fast_branch() {
        // Dry/wet shape: identity dry vs a 7-sample-latent wet. The
        // combinator must pad the dry branch so the join is aligned —
        // with a unity wet, the sum is exactly 2× the delayed input.
        let mut p = Par2::new(Gain(1.0), Late::new(7));
        p.prepare(48_000.0);
        assert_eq!(p.latency_samples(), 7, "join reports the slow branch");
        let mut out = Vec::new();
        for i in 0..32 {
            let x = if i == 0 { 1.0 } else { 0.0 }; // impulse
            out.push(p.tick(x));
        }
        for (i, &y) in out.iter().enumerate() {
            let want = if i == 7 { 2.0 } else { 0.0 };
            assert_eq!(
                y, want,
                "sample {i}: both branches must land at the join together"
            );
        }
    }

    #[test]
    fn par2_matrix_split_carries_distinct_lanes() {
        // M/S shape: distinct inputs per branch through tick_pair,
        // asymmetric latency, aligned outputs.
        let mut p = Par2::new(Late::new(3), Gain(0.5));
        p.prepare(48_000.0);
        let mut got = Vec::new();
        for i in 0..16 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            got.push(p.tick_pair(x, x));
        }
        assert_eq!(got[3], (1.0, 0.5), "both lanes arrive at the join at t=3");
        assert!(got
            .iter()
            .enumerate()
            .all(|(i, &(a, b))| i == 3 || (a, b) == (0.0, 0.0)));
    }
}
