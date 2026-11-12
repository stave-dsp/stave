//! **Layer 3 — circuit.** Models derived from schematics: devices
//! (diodes, triodes, magnetic cores), solution methods (Newton with
//! SPICE junction limiting, wave digital filters, load lines, Wright
//! omega) and topologies built from them, verified against ngspice in
//! stave-test. Circuits speak volts; [`AudioAdapter`](crate::AudioAdapter)
//! maps them to full scale.

pub mod audio;
pub mod devices;
pub mod methods;
pub mod topologies;
