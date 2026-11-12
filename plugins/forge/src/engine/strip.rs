//! One M/S lane, whole: the amp channel (declared as a `chain!`, so
//! the code's shape IS the signal path) and its region stack.

use stave_dsp::{
    shapes, Compensator, CurveStage, FilterSpec, FilterStage, Frame, Hysteresis, NonlinearRc,
    Oversampled2x, Slew, StageKind, SwapPoint,
};

use crate::engine::voicing::conduction_sense;
use stave_dsp::RcClipperConfig;
use stave_dsp::{Oversampler2xFir, Oversampler2xFirInner};

/// The strip's interior clock: the nested region factor. Structure
/// knowledge lives with the structure that has it.
pub(super) const OS_FACTOR: f32 = (Oversampler2xFir::FACTOR * Oversampler2xFirInner::FACTOR) as f32;

stave_dsp::chain! {
    /// The amp channel, in signal order. Slot names are the handles
    /// everything else uses: `.primary` is the StageId (routes,
    /// swaps, senses); `.voicing()` is the typed cell (retuning).
    /// `primary` and `second` KIND-SWITCH at runtime (Curve ↔ the
    /// screamer RC / Bypass), so those two are touched through
    /// swap-aware matching, never the typed accessor.
    pub(in crate::engine) struct AmpChain {
        primary: Curve = CurveStage::from_fn_with_sense(
            -12.0,
            12.0,
            0.0,
            shapes::knee(1.0),
            conduction_sense,
        ),
        voicing: Filter = FilterStage::new(FilterSpec::HighShelf { hz: 900.0, db: 0.0 }),
        second: Curve = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(2.5)),
        post: Filter = FilterStage::new(FilterSpec::Peak { hz: 1_000.0, db: 0.0, q: 1.0 }),
        grit: Slew = Slew::new(200.0, 200.0),
        iron: Hysteresis = Hysteresis::new(0.4, 0.0),
    }
}

/// One M/S lane, whole: the amp channel, the nested 4× stack, the
/// dry-path compensator, the base frames routes displace from, and
/// the two swap points (chain XOR bench is the `SwapPoint`
/// primitive's invariant, not this struct's). Grouping the lane's
/// state makes index-crossing (lane 0's chain through lane 1's
/// oversampler) unwritable.
pub(super) struct Strip {
    /// The lane's audio path AS STRUCTURE: the nested 4× stack is
    /// `Oversampled2x::front(Oversampled2x::nested(amp))` — rate contract and
    /// the 66+18 latency arithmetic live in the form.
    pub(super) region: Oversampled2x<Oversampled2x<AmpChain>>,
    /// Host-domain (L/R) dry-path compensation for this lane —
    /// the latency laws' pad, realized.
    pub(super) dry: Compensator,
    pub(super) primary_base: Frame,
    pub(super) second_base: Frame,
    /// The swap points: primary alternates Curve↔Rc, second
    /// Curve↔Bypass. The bench invariant lives in the primitive.
    pub(super) primary_bench: SwapPoint,
    pub(super) second_bench: SwapPoint,
}

impl Strip {
    pub(super) fn new() -> Self {
        Self {
            region: Oversampled2x::front(Oversampled2x::nested(AmpChain::new())),
            dry: Compensator::new(),
            primary_base: Frame::NEUTRAL,
            second_base: Frame::NEUTRAL,
            // The chain starts both nonlinear slots as Curve, so the
            // benches hold the OTHER kind: a spare RC for primary
            // (re-voiced in place on unpark) and the free Bypass
            // unit for second.
            primary_bench: SwapPoint::parking(StageKind::Rc(NonlinearRc::ts_feedback_clipper(
                &RcClipperConfig {
                    tau_ms: 3.0,
                    threshold: 0.5,
                    hardness: 4.0,
                },
                1.2,
            ))),
            second_bench: SwapPoint::parking(StageKind::Bypass),
        }
    }

    /// The amp channel — named slots and all.
    #[inline]
    pub(super) fn amp(&self) -> &AmpChain {
        &self.region.inner.inner
    }
    #[inline]
    pub(super) fn amp_mut(&mut self) -> &mut AmpChain {
        &mut self.region.inner.inner
    }
}
