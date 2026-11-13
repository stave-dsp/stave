//! **Layer 4 — circuit.** Models derived from schematics, in separate
//! concerns so any circuit can be solved any supported way:
//!
//! - [`devices`] — one module per device family (diode, triode,
//!   magnetic), one trait per family that every published model of it
//!   implements, parts as constants;
//! - [`netlist`] — a circuit as data ([`Netlist`](netlist::Netlist)):
//!   nodes, elements, and the SPICE subset the ngspice decks are written
//!   in, so one deck feeds ngspice and stave;
//! - [`methods`] — how a circuit is solved (Newton with SPICE junction
//!   limiting, wave digital filters, load lines, Wright omega);
//! - [`topologies`] — named circuits, each a hand-derived solution of
//!   its schematic with its ngspice case in stave-test.
//!
//! *Claim:* a model solves the schematic it names. *Verified by:*
//! ngspice on the same netlist (`stave-test/tests/matches_ngspice.rs`,
//! netlists in `stave-test/spice/`), and high-precision solutions where
//! SPICE has no model (`matches_reference.rs`). Circuits speak volts;
//! [`AudioAdapter`](crate::AudioAdapter) maps them to full scale.

pub mod audio;
pub mod devices;
pub mod methods;
pub mod netlist;
pub mod topologies;
