//! A param's value as the stave-dsp unit its [`Unit`] names, converted
//! once, at the boundary between the param table and the DSP (DSP.md
//! U4): a `Unit::Hz` param yields [`Hz`], a `Unit::Db` param [`Db`] (or
//! its linear [`Gain`]), a `Unit::Ms` param [`Seconds`], a `Unit::Ratio`
//! param a [`Ratio`]. Every stave-dsp setter takes the unit, so a param
//! cannot be handed to the wrong one:
//!
//! ```
//! use stave_core::params::{FloatParam, ParamId, Range, Unit};
//! use stave_dsp::units::{hz, Hz};
//! use stave_dsp::Svf;
//!
//! let cutoff = FloatParam::new(ParamId(1), "Cutoff", Range::linear(20.0, 20_000.0))
//!     .unit(Unit::Hz)
//!     .default(1_000.0);
//! let mut filter = Svf::default();
//! filter.set(cutoff.value_as(), 0.707);
//! assert_eq!(cutoff.value_as::<Hz>(), hz(1_000.0));
//! ```
//!
//! A conversion costs what the arithmetic costs: `Hz`, `Db` and `Ratio`
//! are the value itself, `Seconds` a division, `Gain` a `powf`. The unit
//! is checked in debug builds only (a release build never panics on the
//! audio path): reading a `Unit::Db` param as [`Hz`] is a debug-build
//! panic.

use stave_dsp::units::{ms, Db, Gain, Hz, Ratio, Seconds};

use super::display::Unit;

/// A unit a param's value converts into: [`Hz`], [`Db`], [`Gain`],
/// [`Seconds`] or [`Ratio`]. Read through
/// [`FloatParam::value_as`](super::FloatParam::value_as), or per frame
/// through [`Ramp::next_as`](super::Ramp::next_as).
pub trait Quantity: Sized {
    /// `value`, in `unit`'s display scale (Hz, dB, ms, the ratio), as
    /// this unit. Debug builds panic when `unit` is not one this type
    /// reads.
    fn from_param(unit: Unit, value: f64) -> Self;
}

impl Quantity for Hz {
    #[inline]
    #[track_caller]
    fn from_param(unit: Unit, value: f64) -> Self {
        debug_assert!(matches!(unit, Unit::Hz), "a {unit:?} param read as Hz");
        Hz::new(value as f32)
    }
}

/// `Unit::Db`, and `Unit::DbFloor`, whose floor reads as silence (−inf dB).
impl Quantity for Db {
    #[inline]
    #[track_caller]
    fn from_param(unit: Unit, value: f64) -> Self {
        match unit {
            Unit::DbFloor(floor) if value <= f64::from(floor) => Db::new(f32::NEG_INFINITY),
            Unit::Db | Unit::DbFloor(_) => Db::new(value as f32),
            _ => {
                debug_assert!(false, "a {unit:?} param read as Db");
                Db::new(value as f32)
            }
        }
    }
}

/// A dB param as the amplitude it scales by: a `Unit::DbFloor` param at
/// its floor is silence (×0).
impl Quantity for Gain {
    #[inline]
    #[track_caller]
    fn from_param(unit: Unit, value: f64) -> Self {
        Db::from_param(unit, value).to_gain()
    }
}

/// A `Unit::Ms` param (its plain value is milliseconds).
impl Quantity for Seconds {
    #[inline]
    #[track_caller]
    fn from_param(unit: Unit, value: f64) -> Self {
        debug_assert!(matches!(unit, Unit::Ms), "a {unit:?} param read as Seconds");
        ms(value)
    }
}

/// A `Unit::Ratio` param (`4.0` reads `4.0:1`).
impl Quantity for Ratio {
    #[inline]
    #[track_caller]
    fn from_param(unit: Unit, value: f64) -> Self {
        debug_assert!(matches!(unit, Unit::Ratio), "a {unit:?} param read as Ratio");
        Ratio::new(value as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_dsp::units::{db, hz, seconds};

    #[test]
    fn each_unit_reads_as_its_type() {
        assert_eq!(Hz::from_param(Unit::Hz, 2_400.0), hz(2_400.0));
        assert_eq!(Db::from_param(Unit::Db, -6.0), db(-6.0));
        assert_eq!(Seconds::from_param(Unit::Ms, 250.0), seconds(0.25));
        assert_eq!(Ratio::from_param(Unit::Ratio, 4.0), Ratio::new(4.0));
        assert_eq!(Gain::from_param(Unit::Db, 0.0), Gain::UNITY);
    }

    #[test]
    fn a_floored_db_param_at_its_floor_is_silence() {
        assert_eq!(Db::from_param(Unit::DbFloor(-60), -60.0).get(), f32::NEG_INFINITY);
        assert_eq!(Gain::from_param(Unit::DbFloor(-60), -60.0), Gain::ZERO);
        assert_eq!(Db::from_param(Unit::DbFloor(-60), -59.0), db(-59.0));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "read as Hz")]
    fn the_wrong_unit_is_a_debug_panic() {
        let _ = Hz::from_param(Unit::Db, -6.0);
    }
}
