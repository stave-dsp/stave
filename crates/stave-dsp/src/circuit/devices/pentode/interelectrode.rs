//! A pentode's (or beam tetrode's) interelectrode capacitances: fixed
//! capacitors between its electrodes, as a datasheet lists them.
//!
//! The parts are the manufacturers' sheets (Philips, Mullard, RCA HB-3,
//! GEC, as collected at frank.pocnet.net). Verified by
//! `crates/stave-test/tests/capacitance_matches_ngspice.rs`: an EL34
//! stage's response to 100 MHz, the element against ngspice's B-source
//! law with the same capacitors.

/// A pentode's interelectrode capacitances (F). Datasheets give the
/// input capacitance (control grid to every electrode but the plate),
/// the output capacitance (plate to every electrode but the control
/// grid) and the control grid to plate; with the screen and cathode both
/// at signal ground in any stage, the input is placed grid to cathode and
/// the output plate to cathode. `grid_screen` is the control grid to
/// screen part, for a circuit that drives the screen (zero where the
/// datasheet folds it into the input, as they all do).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Interelectrode {
    pub grid_cathode: f64,
    pub grid_screen: f64,
    pub grid_plate: f64,
    pub plate_cathode: f64,
}

impl Interelectrode {
    /// No capacitance: the static law alone.
    pub const NONE: Self = Self {
        grid_cathode: 0.0,
        grid_screen: 0.0,
        grid_plate: 0.0,
        plate_cathode: 0.0,
    };

    /// EL34: Philips's sheet (Mullard's the same): input 15.2 pF, output
    /// 8.4 pF, grid–plate at most 1.1 pF (taken at the maximum).
    pub const EL34: Self = Self::pf(15.2, 1.1, 8.4);

    /// 6L6GC: RCA HB-3 (Tung-Sol, unshielded, the same): 10, 0.6, 6.5 pF.
    pub const T6L6GC: Self = Self::pf(10.0, 0.6, 6.5);

    /// KT88: GEC / M-O Valve (cold, unscreened, tetrode connection): 16,
    /// 1.2, 12 pF.
    pub const KT88: Self = Self::pf(16.0, 1.2, 12.0);

    /// EL84: Mullard (January 1964; RCA's 6BQ5 the same): 10.8, at most
    /// 0.5, 6.5 pF.
    pub const EL84: Self = Self::pf(10.8, 0.5, 6.5);

    /// From picofarads: input (grid–cathode), grid–plate, output
    /// (plate–cathode); no separate grid–screen part.
    const fn pf(input: f64, grid_plate: f64, output: f64) -> Self {
        Self {
            grid_cathode: input * 1e-12,
            grid_screen: 0.0,
            grid_plate: grid_plate * 1e-12,
            plate_cathode: output * 1e-12,
        }
    }
}

impl Default for Interelectrode {
    fn default() -> Self {
        Self::NONE
    }
}
