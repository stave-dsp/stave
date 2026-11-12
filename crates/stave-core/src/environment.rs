//! `Environment` — the host-imposed operating conditions a processor
//! prepares under. One value, threaded down the prepare cascade, so
//! every coefficient in the tree derives from THE SAME rate and no
//! stage can be prepared against a stale one (the rate contract).
//!
//! Rate regions: an oversampled subtree runs at an integer multiple
//! of the outer rate. [`Environment::region`] derives the inner
//! environment from the wrapper's typed factor — the multiplier is
//! data on the wrapper (`Oversampler2xFir::FACTOR`), never a loose
//! literal at a call site.

/// The conditions of one activation. `PartialEq` is exact on
/// purpose: a re-prepare at the same conditions is not a change.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Environment {
    /// Samples per second this (sub)tree runs at.
    pub sample_rate: f32,
    /// Upper bound on frames per process call at this rate.
    pub max_block: usize,
}

impl Environment {
    pub const fn new(sample_rate: f32, max_block: usize) -> Self {
        Self {
            sample_rate,
            max_block,
        }
    }

    /// The environment INSIDE a rate region running at `factor`× the
    /// outer rate: rate multiplies, and so does the block bound.
    pub const fn region(self, factor: usize) -> Self {
        Self {
            sample_rate: self.sample_rate * factor as f32,
            max_block: self.max_block * factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_scales_rate_and_block() {
        let outer = Environment::new(48_000.0, 512);
        let inner = outer.region(4);
        assert_eq!(inner, Environment::new(192_000.0, 2048));
        assert_eq!(outer.region(1), outer, "1x region is the identity");
    }
}
