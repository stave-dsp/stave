//! Control-signal primitives: generators (LFOs, envelopes, random
//! walks), followers and smoothers. Most are [`Source`](crate::Source)s,
//! readable without advancing; all are [`Stage`](crate::Stage)s.

pub mod adsr;
pub mod drift;
pub mod envelope;
pub mod lag;
pub mod lfo;
pub mod mod_lfo;
pub mod note_sources;
pub mod random_walk;
pub mod slot;
pub mod smoothed;
pub mod sub;
