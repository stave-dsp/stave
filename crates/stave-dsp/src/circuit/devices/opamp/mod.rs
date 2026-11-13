//! The op-amp family: the [`OpAmp`] trait and its models, [`Ideal`] (a
//! nullor: infinite gain and bandwidth, no rails) and [`Macromodel`]
//! (finite gain and gain-bandwidth, slew limit, input offset and rail
//! clipping), with parts as constants of the macromodel
//! ([`Macromodel::TL072`], [`Macromodel::JRC4558`], [`Macromodel::LM308`]).
//! [`Amp`] is the run-time choice a netlist element holds.
//!
//! A netlist places an op-amp with
//! [`ElementKind::OpAmp`](crate::circuit::netlist::ElementKind::OpAmp):
//! its inputs draw no current, and its output is a voltage source, so
//! feedback networks are written exactly as on the schematic.

mod macromodel;

pub use macromodel::{Macromodel, Prepared};

use crate::units::{Hz, Volts};

/// What an op-amp's datasheet specifies, open loop: what a solver needs
/// to place it. [`Ideal`] reports infinite gain and bandwidth.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OpenLoop {
    /// DC differential gain, volts per volt (`f64::INFINITY` for ideal).
    pub gain: f64,
    /// Unity-gain frequency (`f32::INFINITY` for ideal).
    pub gain_bandwidth: Hz,
    /// The output's largest rate of change, volts per second.
    pub slew_rate: f64,
    /// Input offset voltage, added to `v(+) − v(−)`.
    pub offset: Volts,
    /// How close the output swings to each supply rail.
    pub headroom: Volts,
}

/// The op-amp family: every model reports its open-loop specification,
/// so a solver places any (a nullor for [`Ideal`], the gain stage and
/// rails of [`Macromodel`]).
pub trait OpAmp: Copy {
    fn open_loop(&self) -> OpenLoop;

    /// True when the model is the nullor: `v(+) = v(−)` with an
    /// unbounded output.
    fn is_ideal(&self) -> bool {
        self.open_loop().gain.is_infinite()
    }
}

/// The ideal op-amp: a nullor, `v(+) = v(−)`, its output whatever
/// current the feedback asks.
///
/// # Models
///
/// The textbook infinite-gain amplifier with no rails, no bandwidth
/// limit and no offset.
///
/// # Derivation
///
/// MNA's nullor stamp (Vlach & Singhal, *Computer Methods for Circuit
/// Analysis and Design*, §4.3): one branch current at the output, one
/// row `v(+) − v(−) = 0`. It is linear, so it costs the DK solve nothing.
///
/// # Verified
///
/// `stave-test/tests/dk_matches_ngspice.rs`
/// `opamp_clipper_with_an_ideal_op_amp_element_matches_ngspice`: the
/// TS808 clipper built with this element against ngspice's
/// `opamp_clipper` deck (which writes the ideal op-amp as controlled
/// sources), normalized RMS 5e-5 at 16×.
///
/// # Cost
///
/// None beyond the linear network's: one more MNA row.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// Any circuit with negative feedback around it; open loop it has no
/// solution (the MNA matrix is singular).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Ideal;

impl OpAmp for Ideal {
    fn open_loop(&self) -> OpenLoop {
        OpenLoop {
            gain: f64::INFINITY,
            gain_bandwidth: Hz::new(f32::INFINITY),
            slew_rate: f64::INFINITY,
            offset: Volts::new(0.0),
            headroom: Volts::new(0.0),
        }
    }
}

impl OpAmp for Macromodel {
    fn open_loop(&self) -> OpenLoop {
        OpenLoop {
            gain: self.gain,
            gain_bandwidth: self.gain_bandwidth,
            slew_rate: self.slew_rate,
            offset: self.offset,
            headroom: self.headroom,
        }
    }
}

/// Any op-amp model, chosen at run time: what a netlist element holds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Amp {
    Ideal(Ideal),
    Macromodel(Macromodel),
}

impl OpAmp for Amp {
    fn open_loop(&self) -> OpenLoop {
        match self {
            Self::Ideal(a) => a.open_loop(),
            Self::Macromodel(a) => a.open_loop(),
        }
    }
}

#[cfg(test)]
mod tests;
