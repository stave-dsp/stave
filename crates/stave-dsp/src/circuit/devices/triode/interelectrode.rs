//! A triode's interelectrode capacitances: fixed capacitors between its
//! electrodes, as a datasheet lists them. With a plate gain of `−A`,
//! the grid–plate capacitance appears at the grid as `(1 + A)·Cgp` (the
//! Miller effect) — a 12AX7 stage's top end.
//!
//! The parts are the manufacturers' sheets (RCA HB-3, GE, Philips, as
//! collected at frank.pocnet.net), unshielded, section 1. Verified by
//! `crates/stave-test/tests/capacitance_matches_ngspice.rs`: the 12AX7
//! stage's response to 100 MHz and its Miller input at 10 kHz, the
//! element against ngspice's B-source law with the same capacitors.

/// A triode's interelectrode capacitances (F), as datasheets give them
/// for one section: grid to cathode (the datasheet's input capacitance,
/// heater included), grid to plate, plate to cathode (output, heater
/// included). A circuit method places them as fixed capacitors on the
/// element.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Interelectrode {
    pub grid_cathode: f64,
    pub grid_plate: f64,
    pub plate_cathode: f64,
}

impl Interelectrode {
    /// No capacitance: the static law alone.
    pub const NONE: Self = Self {
        grid_cathode: 0.0,
        grid_plate: 0.0,
        plate_cathode: 0.0,
    };

    /// 12AX7, section 1: RCA's 12AX7A sheet (RCA HB-3; GE's 12AX7 sheet,
    /// unshielded, the same): grid–plate 1.7 pF, grid–cathode 1.6 pF,
    /// plate–cathode 0.46 pF. For [`Koren::T12AX7`](super::koren::Koren::T12AX7)
    /// and the measured 12AX7s of [`Dempwolf`](super::dempwolf::Dempwolf).
    pub const T12AX7: Self = Self::pf(1.6, 1.7, 0.46);

    /// 12AT7, section 1: RCA HB-3 and GE (unshielded): 1.5, 2.2, 0.5 pF.
    pub const T12AT7: Self = Self::pf(2.2, 1.5, 0.5);

    /// 12AU7A, section 1: RCA HB-3: 1.5, 1.6, 0.5 pF.
    pub const T12AU7: Self = Self::pf(1.6, 1.5, 0.5);

    /// 6DJ8 as Philips's ECC88 sheet (1958, unshielded): 1.4, 3.3, 1.8 pF.
    pub const T6DJ8: Self = Self::pf(3.3, 1.4, 1.8);

    /// 6SN7GTB, section 1: GE ET-T899 (unshielded; RCA's 6SN7GTA and
    /// Tung-Sol agree): 4.0, 2.2, 0.7 pF.
    pub const T6SN7: Self = Self::pf(2.2, 4.0, 0.7);

    /// From picofarads: grid–cathode, grid–plate, plate–cathode.
    const fn pf(grid_cathode: f64, grid_plate: f64, plate_cathode: f64) -> Self {
        Self {
            grid_cathode: grid_cathode * 1e-12,
            grid_plate: grid_plate * 1e-12,
            plate_cathode: plate_cathode * 1e-12,
        }
    }
}

impl Default for Interelectrode {
    fn default() -> Self {
        Self::NONE
    }
}
