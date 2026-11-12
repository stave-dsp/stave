//! The signal kernel — the seven forms. Primitives
//! only: nothing here may grow product flavor, and this layer
//! depends on nothing below it — the layering check
//! (check-view-bindings.sh §4) holds with no allowlist. The
//! half-band machinery (`resample`) is Oversampled2x's implementation
//! detail, public only for the conformance instruments.
//!
//! Present: Node, Sequence, Branch (+ its Compensator pad on the
//! kernel ring), Loop, Oversampled, Lanes, Control (its param-source half
//! lives in stave-core::params), and Voices: event-keyed lanes under
//! a declared steal policy.

pub mod control;
pub mod feedback;
pub mod lanes;
pub mod node;
pub mod oversampled;
pub mod parallel;
pub mod resample;
pub(crate) mod ring;
pub mod sequence;
pub mod voices;
