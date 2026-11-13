//! The pentode family — beam tetrodes included: the [`Pentode`] trait
//! and its published model, [`koren::Koren`] (plate and screen current).
//!
//! A beam tetrode (6L6GC, KT88) has no suppressor grid; its beam-forming
//! plates do the suppressor's job, and its plate curves are a pentode's,
//! so the same family and the same law hold for both.
//!
//! Cohen and Hélie's refinement (DAFx 2010) is not here: its published
//! form is a triode fit, and no pentode parameter set exists to check it
//! against.

pub mod interelectrode;
pub mod koren;

pub use interelectrode::Interelectrode;

/// Currents into a pentode's plate and screen grid (cathode current is
/// their sum) and their partial derivatives, at control-grid, screen and
/// plate voltages to the cathode.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PentodeCurrents {
    pub plate: f64,
    pub screen: f64,
    pub plate_per_grid_volt: f64,
    pub plate_per_screen_volt: f64,
    pub plate_per_plate_volt: f64,
    pub screen_per_grid_volt: f64,
    pub screen_per_screen_volt: f64,
}

/// The pentode family: every published pentode or beam-tetrode model
/// implements it, so a circuit takes any.
pub trait Pentode: Copy {
    /// Plate and screen currents, and their partials, at control-grid
    /// (`vg1k`), screen (`vg2k`) and plate (`vpk`) volts to the cathode.
    fn currents(&self, vg1k: f64, vg2k: f64, vpk: f64) -> PentodeCurrents;
}
