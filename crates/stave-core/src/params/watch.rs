//! `Watch` — block-rate change detection over a FIXED set of params:
//! the state-semantics replacement for hand-rolled `last_*` caches.
//!
//! Semantics: STATE, not events. A poll reports changed when any
//! watched param's plain value DIFFERS from the previous poll — a
//! write that lands the same value is not a change, so idempotent
//! re-applies (presets, state loads) stay free, and derived rebuilds
//! never fire twice for the same state. The first poll always
//! reports changed (the initial build). If a feature ever needs true
//! write-event tracking (host dirty flags), that is a different tool
//! with a different name — do not overload this one.
//!
//! Alloc-free and RT-safe: `N` is fixed at compile time, the
//! snapshot is `[u64; N]` of value bits, and polling is N relaxed
//! atomic loads.
//!
//! **The activation invariant: reset every watch in `activate()`.** A
//! re-prepare wipes TWO things the params can't see — rate-derived
//! coefficients AND param-applied structure that `prepare()`
//! resets (re-voiced curves, clipper settings) — so the first poll
//! after activation must report changed and force full
//! re-application (forge's `boot()` is the worked example). A
//! rate-watching variant (`changed_env`) once existed for the
//! first half; it was deleted UNUSED because it cannot see the
//! second half — the reset covers both, and a partial guarantee
//! that reads like a full one is a trap.

use super::AnyParam;

#[derive(Clone, Copy)]
pub struct Watch<const N: usize> {
    seen: Option<[u64; N]>,
}

impl<const N: usize> Watch<N> {
    pub const fn new() -> Self {
        Self { seen: None }
    }

    /// Poll once per block. True when any watched value differs from
    /// the last poll (always true on the first poll after `new` or
    /// [`reset`](Self::reset)).
    pub fn changed(&mut self, params: [&dyn AnyParam; N]) -> bool {
        let mut now = [0u64; N];
        for (slot, p) in now.iter_mut().zip(params) {
            *slot = p.value().to_bits();
        }
        let hit = self.seen != Some(now);
        self.seen = Some(now);
        hit
    }

    /// Forget the snapshot — the next poll reports changed. **Call
    /// from `activate()`** (the activation invariant, module docs): a
    /// re-prepare wipes state the params can't see.
    pub fn reset(&mut self) {
        self.seen = None;
    }
}

impl<const N: usize> Default for Watch<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Mode-gated structural watching:
/// a rebuild whose BASE params always matter, plus params that matter
/// only inside one mode of a structural selector — the state-keyed
/// binding set, as data. The modes are declared once at construction
/// (`ModalWatch::new([(SCREAMER, ..), (PUSHPULL, ..)])`-style index
/// pairing); nothing is evaluated per block beyond the polls.
///
/// The invariant this type makes unwritable-wrong: every watch polls
/// UNCONDITIONALLY (a gated poll would go stale and replay an old
/// edit when its mode re-engages), but a gated param only FIRES the
/// rebuild while its mode is current. Put the selector itself in the
/// base set — entering a mode then re-fires the rebuild, which reads
/// the gated params' CURRENT values.
pub struct ModalWatch<const B: usize, const G: usize> {
    base: Watch<B>,
    /// `modes[i]` is the selector value under which `gated[i]` fires.
    modes: [usize; G],
    gated: [Watch<1>; G],
}

impl<const B: usize, const G: usize> ModalWatch<B, G> {
    pub const fn new(modes: [usize; G]) -> Self {
        Self {
            base: Watch::new(),
            modes,
            gated: [Watch::new(); G],
        }
    }

    /// Poll once per block with the selector's current value.
    pub fn changed(
        &mut self,
        mode: usize,
        base: [&dyn AnyParam; B],
        gated: [&dyn AnyParam; G],
    ) -> bool {
        let mut hit = self.base.changed(base);
        for (i, p) in gated.into_iter().enumerate() {
            // Poll FIRST (unconditionally), gate the fire.
            let moved = self.gated[i].changed([p]);
            hit |= moved && self.modes[i] == mode;
        }
        hit
    }

    /// Forget every snapshot — the next poll reports changed.
    pub fn reset(&mut self) {
        self.base.reset();
        for g in self.gated.iter_mut() {
            g.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{FloatParam, ParamId, Range};

    #[test]
    fn state_semantics() {
        let p = FloatParam::new(ParamId(1), "x", Range::linear(0.0, 1.0)).default(0.5);
        let q = FloatParam::new(ParamId(2), "y", Range::linear(0.0, 1.0));
        let mut w = Watch::<2>::new();
        assert!(w.changed([&p, &q]), "first poll = initial build");
        assert!(!w.changed([&p, &q]), "steady state");
        p.set_value(0.5);
        assert!(!w.changed([&p, &q]), "same-value write is NOT a change");
        p.set_value(0.7);
        assert!(w.changed([&p, &q]));
        assert!(!w.changed([&p, &q]));
        w.reset();
        assert!(w.changed([&p, &q]), "reset forgets the snapshot");
    }

    #[test]
    fn modal_semantics() {
        // The forge invariant in miniature: base = [stack], gated = drive
        // (mode 2), bias (mode 3).
        let stack = FloatParam::new(ParamId(1), "stack", Range::linear(0.0, 3.0));
        let drive = FloatParam::new(ParamId(2), "drive", Range::linear(0.0, 10.0)).default(4.0);
        let bias = FloatParam::new(ParamId(3), "bias", Range::linear(0.0, 1.0));
        let mut w = ModalWatch::<1, 2>::new([2, 3]);
        let mode = |s: &FloatParam| s.plain() as usize;

        assert!(
            w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "first poll fires"
        );
        assert!(
            !w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "steady"
        );

        drive.set_value(6.0);
        assert!(
            !w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "gated edit outside its mode does not fire"
        );

        stack.set_value(2.0);
        assert!(
            w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "entering the mode fires"
        );
        assert!(
            !w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "the outside edit was consumed by the poll, not replayed"
        );

        drive.set_value(8.0);
        assert!(
            w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "gated edit inside its mode fires"
        );
        bias.set_value(0.4);
        assert!(
            !w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "the OTHER mode's param stays gated"
        );

        w.reset();
        assert!(
            w.changed(mode(&stack), [&stack], [&drive, &bias]),
            "reset re-fires"
        );
    }
}
