//! `Environment` — the host-imposed conditions a plugin prepares under. One value, threaded down the prepare cascade, so
//! every coefficient in the tree derives from THE SAME rate and no
//! stage can be prepared against a stale one (the rate contract).
//!
//! Rate regions: an oversampled subtree runs at an integer multiple
//! of the outer rate. [`Environment::region`] derives the inner
//! environment from the wrapper's typed factor — the multiplier is
//! data on the wrapper (`Oversampled2x::FACTOR`), never a loose
//! literal at a call site.

use crate::info::Layout;

/// The conditions of one prepare. `PartialEq` is exact on purpose: a
/// re-prepare at the same conditions is not a change.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Environment {
    /// Hz this (sub)tree runs at.
    pub sample_rate: f32,
    /// Upper bound on frames per process call at this rate.
    pub max_block: usize,
    /// The bus layout the host chose.
    pub layout: Layout,
}

impl Environment {
    /// `sample_rate` Hz, blocks of at most `max_block` frames, stereo.
    pub const fn new(sample_rate: f32, max_block: usize) -> Self {
        Self {
            sample_rate,
            max_block,
            layout: Layout::STEREO,
        }
    }

    /// The same conditions on `layout`.
    pub const fn with_layout(self, layout: Layout) -> Self {
        Self { layout, ..self }
    }

    /// The environment INSIDE a rate region running at `factor`× the
    /// outer rate: rate multiplies, and so does the block bound.
    pub const fn region(self, factor: usize) -> Self {
        Self {
            sample_rate: self.sample_rate * factor as f32,
            max_block: self.max_block * factor,
            layout: self.layout,
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
