//! Protocol models of stave's plugin ↔ host contracts (docs/overhaul.md
//! V4), checked exhaustively by a small explicit-state model checker
//! ([`check`]), and the same models as oracles: the simulated host
//! (stave-host `plugin::sim`) checks a real plugin's answers against
//! [`lifecycle`], and [`check::validate`] checks traces recorded from
//! real runs against a model.
//!
//! ```text
//! check      the checker: BFS, invariants, liveness, trace validation
//! lifecycle  setup / activate / process / state: what a host may call
//!            and what the wrapper answers
//! gesture    begin / perform / end through the bounded gesture ring
//! restore    a state load against the smoother (S1's read order)
//! slots      telemetry slot reservation
//! view       an editor's lifetime against its instance's (S7)
//! ```
//!
//! No dependencies, no unsafe: models are plain data.
#![forbid(unsafe_code)]

pub mod check;
pub mod gesture;
pub mod lifecycle;
pub mod restore;
pub mod slots;
pub mod view;
