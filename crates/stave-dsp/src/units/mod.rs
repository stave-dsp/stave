//! **Layer 0 — units.** Quantities as types, so a mix-up is a compile
//! error rather than a wrong sound: [`Hz`], [`Db`], [`Seconds`],
//! [`Samples`], [`Gain`], [`Ratio`], [`Semitones`], [`Cents`],
//! [`SampleRate`], and the circuit units [`Volts`], [`Amps`], [`Ohms`],
//! [`Farads`], [`Henries`]. Units depend on nothing, and every other
//! layer may use them.
//!
//! *Claim:* a unit converts exactly as its definition says and costs
//! nothing at run time.
//!
//! *Verified by:* seeded property tests of every conversion and its
//! inverse (`units/tests.rs`, against the crate's `db_to_amp` and
//! `amp_to_db` bit for bit), stave-core's `tests/unit_text.rs` (the text
//! matches the param `Unit`s), and Kani proofs of what the model checker
//! can reason about (`units/proofs.rs`: prefix selection, frame
//! rounding).
//!
//! Every unit is a `Copy` newtype, `#[repr(transparent)]` over its
//! scalar with every method `#[inline]`, so it costs nothing at runtime.
//! A unit is built with a constructor ([`hz`], [`ms`], [`db`],
//! [`kohm`] …, or `Unit::new`) and read back with `get()` in its base
//! scale. Constructors check the value's range in debug builds only;
//! a release build never panics on the audio path.
//!
//! **Conversions are explicit methods**, never `From` between scales:
//! [`Db::to_gain`], [`Gain::to_db`], [`Seconds::samples`],
//! [`Samples::seconds`], [`Hz::period`], [`Semitones::ratio`].
//! **Operators exist only where they read as physics**: a unit adds to
//! itself and scales by its scalar; `Hz * Ratio` is a frequency,
//! `Gain * f32` a sample, `Volts / Ohms` a current. Anything else is a
//! compile error:
//!
//! ```compile_fail
//! use stave_dsp::units::{db, hz};
//! let nonsense = hz(440.0) + db(-6.0);
//! ```
//!
//! ```compile_fail
//! use stave_dsp::units::{ms, samples};
//! // A duration and a sample count meet only through a SampleRate.
//! let nonsense = ms(10.0) + samples(480.0);
//! ```
//!
//! **Precision, chosen per quantity:**
//!
//! | Unit | Scalar | Why |
//! |---|---|---|
//! | [`Hz`], [`SampleRate`] | `f32` | the crate's sample-rate and setter type; exact for every host rate. Phase accumulates in `f64` via [`Hz::cycles_per_sample`] |
//! | [`Db`], [`Gain`] | `f32` | the precision of the samples a gain scales |
//! | [`Ratio`], [`Semitones`], [`Cents`] | `f32` | resolves an interval to ~1e-4 cents; matches [`Hz`] |
//! | [`Seconds`], [`Samples`] | `f64` | durations become sample counts, and `f32` stops counting whole samples past 2²⁴ (22 s at 768 kHz); stave-core's smoothing times are `f64` |
//! | [`Volts`], [`Amps`], [`Ohms`], [`Farads`], [`Henries`] | `f64` | the circuit solvers' precision: junction currents span twelve decades |
//!
//! **Display** prints the unit. Where stave-core's param `Unit` has a
//! twin (Hz, dB, ms/s, ratio) the text is identical (`2.40 kHz`,
//! `-6.0 dB`, `30.0 ms`, `4.0:1`); circuit units read in engineering
//! notation (`4.70 kΩ`, `22.0 nF`).

#[macro_use]
mod newtype;
mod circuit;
mod frequency;
mod level;
mod pitch;
mod text;
mod time;

pub use circuit::{
    amps, farads, henries, kohm, ma, megohm, mh, mv, na, nf, ohms, pf, ua, uf, uh, uv, volts, Amps,
    Farads, Henries, Ohms, Volts,
};
pub use frequency::{hz, khz, Hz, SampleRate};
pub use level::{db, Db, Gain};
pub use pitch::{cents, semitones, Cents, Ratio, Semitones};
pub use time::{ms, samples, seconds, us, Samples, Seconds};

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;
