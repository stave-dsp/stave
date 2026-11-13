//! **Layer 8 — structures.** Reusable compositions built on everything
//! below: [`Chain`](chain::Chain) (serial composition with addressable,
//! swappable slots and sense endpoints), the [`chain!`](crate::chain)
//! macro naming a chain's slots, and [`ModMatrix`](modmatrix::ModMatrix)
//! (a router whose targets are a chain's stage axes).

pub mod chain;
pub mod modmatrix;
pub mod named_chain;
