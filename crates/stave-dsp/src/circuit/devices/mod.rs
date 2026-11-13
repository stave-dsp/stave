//! Device models, as SPICE model cards hold them: one module per device
//! family, one trait per family that every model of it implements, and
//! parts as constants of a model's parameters.
//!
//! | Family | Trait | Models |
//! |---|---|---|
//! | [`diode`] | [`Diode`](diode::Diode) | [`Shockley`](diode::Shockley) (breakdown and series resistance as fields) |
//! | [`triode`] | [`Triode`](triode::Triode) | [`Koren`](triode::koren::Koren), [`Dempwolf`](triode::dempwolf::Dempwolf), the run-time choice [`Tube`](triode::Tube) |
//! | [`pentode`] | [`Pentode`](pentode::Pentode) | [`Koren`](pentode::koren::Koren) (beam tetrodes too) |
//! | [`opamp`] | [`OpAmp`](opamp::OpAmp) | [`Ideal`](opamp::Ideal), [`Macromodel`](opamp::Macromodel), the run-time choice [`Amp`](opamp::Amp) |
//! | [`ota`] | [`Ota`](ota::Ota) | [`BipolarPair`](ota::BipolarPair) |
//! | [`magnetic`] | [`Magnetic`](magnetic::Magnetic) | [`JilesAtherton`](magnetic::JilesAtherton) |

pub mod diode;
pub mod magnetic;
pub mod opamp;
pub mod ota;
pub mod pentode;
pub mod triode;
