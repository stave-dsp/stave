//! The state layer's first primitive: a
//! value-snapshot change detector for tick-driven redraw — the
//! client-side twin of `stave_core::params::Watch`, and exactly what
//! a poll-based retained UI needs: recompute (repaint) only when the
//! inputs moved.
//!
//! RE-SCOPE, recorded: the push-based `Signal`/`Computed` graph from
//! the proposal is deferred until cross-widget invalidation has a
//! real consumer (the earning criterion). `Changed<T>` IS the
//! degenerate signal graph for a polled source.

/// Reports true when the polled value differs from the last poll
/// (and on the first poll — the initial paint).
#[derive(Clone, Debug, Default)]
pub struct Changed<T: PartialEq> {
    last: Option<T>,
}

impl<T: PartialEq> Changed<T> {
    pub const fn new() -> Self {
        Self { last: None }
    }

    pub fn changed(&mut self, now: T) -> bool {
        let hit = self.last.as_ref() != Some(&now);
        self.last = Some(now);
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_poll_fires_then_only_edges() {
        let mut c = Changed::new();
        assert!(c.changed([1u64, 2]));
        assert!(!c.changed([1, 2]));
        assert!(c.changed([1, 3]));
        assert!(!c.changed([1, 3]));
    }
}
