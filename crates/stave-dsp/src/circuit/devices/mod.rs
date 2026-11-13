//! Device models, as SPICE model cards hold them: one module per device
//! family, one trait per family that every model of it implements, and
//! parts as constants of a model's parameters.
//!
//! | Family | Trait | Models |
//! |---|---|---|
//! | [`diode`] | [`Diode`](diode::Diode) | [`Shockley`](diode::Shockley) |
//! | [`triode`] | [`Triode`](triode::Triode) | [`Koren`](triode::koren::Koren), [`Dempwolf`](triode::dempwolf::Dempwolf), the run-time choice [`Tube`](triode::Tube) |
//! | [`magnetic`] | [`Magnetic`](magnetic::Magnetic) | [`JilesAtherton`](magnetic::JilesAtherton) |

pub mod diode;
pub mod magnetic;
pub mod triode;
