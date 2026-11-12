//! LAYER 2 — derived composition machinery built on kernel + primitives.
//! `chain` = Sequence with addressable/swappable slots (owns its
//! StageKind dispatch); `modmatrix` = forge's control plane.

pub mod chain;
pub mod modmatrix;
pub mod named_chain;
