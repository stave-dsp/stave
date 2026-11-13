//! Device models, as SPICE model cards hold them: one module per device
//! family, one trait per family that every model of it implements, and
//! parts as constants of a model's parameters.
//!
//! | Family | Trait | Models |
//! |---|---|---|
//! | [`diode`] | [`Diode`](diode::Diode) | [`Shockley`](diode::Shockley) (breakdown and series resistance as fields) |
//! | [`bjt`] | [`Bjt`](bjt::Bjt) | [`EbersMoll`](bjt::EbersMoll), [`GummelPoon`](bjt::GummelPoon) |
//! | [`jfet`] | [`Jfet`](jfet::Jfet) | [`ShichmanHodges`](jfet::ShichmanHodges) |
//! | [`mosfet`] | [`Mosfet`](mosfet::Mosfet) | [`ShichmanHodges`](mosfet::ShichmanHodges) (square law) |
//! | [`triode`] | [`Triode`](triode::Triode) | [`Koren`](triode::koren::Koren), [`Dempwolf`](triode::dempwolf::Dempwolf), the run-time choice [`Tube`](triode::Tube) |
//! | [`pentode`] | [`Pentode`](pentode::Pentode) | [`Koren`](pentode::koren::Koren) (beam tetrodes too) |
//! | [`opamp`] | [`OpAmp`](opamp::OpAmp) | [`Ideal`](opamp::Ideal), [`Macromodel`](opamp::Macromodel), the run-time choice [`Amp`](opamp::Amp) |
//! | [`ota`] | [`Ota`](ota::Ota) | [`BipolarPair`](ota::BipolarPair) |
//! | [`magnetic`] | [`Magnetic`](magnetic::Magnetic) | [`JilesAtherton`](magnetic::JilesAtherton) |
//!
//! Transistors come in two [`Polarity`]s; a P device is its N twin with
//! terminal voltages and currents negated.

pub mod bjt;
pub mod diode;
pub mod jfet;
pub(crate) mod junction;
pub mod magnetic;
pub mod mosfet;
pub mod opamp;
pub mod ota;
pub mod pentode;
mod polarity;
pub mod triode;

pub use polarity::Polarity;

/// kT/q at `celsius`, from the SI-exact Boltzmann constant and elementary
/// charge: the thermal voltage a netlist's `.temp` gives its junctions.
pub fn thermal_volts(celsius: f64) -> f64 {
    1.380_649e-23 * (celsius + 273.15) / 1.602_176_634e-19
}
