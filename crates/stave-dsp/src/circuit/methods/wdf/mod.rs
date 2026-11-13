//! Wave digital filters (Fettweis): every element is a port with a
//! resistance R, exchanging voltage waves a (arriving) and b (leaving),
//! so v = (a + b) / 2 and i = (a − b) / (2R). An adaptor joins ports
//! into one adapted port that reflects without waiting on its parent, so
//! a tree computes in one pass up and one pass down per sample.
//!
//! - [`elements`] — the element set (N5): resistor, capacitor, inductor,
//!   resistive voltage and current sources, the series, parallel,
//!   polarity-inverter and ideal-transformer adaptors, and the roots no
//!   tree can adapt (ideal sources, short, open, switch);
//! - [`rtype`] — the R-type adaptor (N4): any topology, its scattering
//!   matrix from modified nodal analysis, ideal op-amps absorbed;
//! - [`Wdf`] — a [`Netlist`](crate::circuit::netlist::Netlist) built
//!   into trees under one R-type root with a multi-port nonlinear root
//!   (`Circuit<Wdf>`): the method a netlist chooses by type parameter;
//! - [`pair`] — the antiparallel diode pair at a hand-built tree's root
//!   ([`PairMethod`]).

mod circuit;
pub mod elements;
mod forest;
pub mod pair;
pub mod rtype;

pub use circuit::{NonlinearRoot, Structure, Wdf};
pub use elements::{Capacitor, Inductor, Parallel, ResistiveSource, Resistor, Root, Series};
pub use pair::PairMethod;
pub use rtype::RType;

/// One port of a tree, seen from its parent.
pub trait OnePort {
    fn resistance(&self) -> f64;
    /// The wave this port sends to its parent.
    fn reflected(&mut self) -> f64;
    /// The wave its parent sends back.
    fn incident(&mut self, wave: f64);
    /// Re-derive port resistances for `sample_rate` (a reactance's, and
    /// everything below an adaptor); clears state. Rate-free ports
    /// ignore it.
    fn prepare(&mut self, _sample_rate: f32) {}
}

