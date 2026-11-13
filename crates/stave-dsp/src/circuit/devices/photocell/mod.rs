//! The photocell family: a light-dependent resistor (LDR) — a
//! cadmium-sulphide cell whose resistance falls with the light it
//! absorbs, slowly, and slower still the brighter it was — and the
//! vactrol, an LDR sealed with an LED that lights it.
//!
//! - [`Najnudel`] — one part's parameters: the cell (Najnudel, Müller,
//!   Hélie & Roze's two-carrier photoresistor, DAFx-23), the LED driving
//!   it and the coupling between them, and the light-history ratio.
//!   Parts: [`Najnudel::VTL5C3`], [`Najnudel::NSL32`].
//! - [`Vactrol`] — a running part: LED current (or light) in, ohms out,
//!   each sample. Its attack is milliseconds and its release tens to
//!   hundreds, longer after a bright, long exposure (the paper's
//!   program dependence), and over hours it adapts to the light it has
//!   lived in (the datasheets' light history).
//! - [`LedDriver`] — the LED's electrical side: a voltage through a
//!   series resistor into the LED, solved in closed form.
//!
//! A photocell is not a netlist element: its resistance moves on the
//! cell's own clock, milliseconds, a thousand samples, so a circuit
//! holds it as an N6 variable resistor and the vactrol sets that
//! resistor's ohms each sample ([`VariableResistor`](crate::circuit::VariableResistor),
//! with [`Smoothing::NONE`](crate::circuit::Smoothing::NONE): the cell is
//! its own smoothing). The optical path closes across one sample, a
//! microsecond-scale lag against a millisecond device.
//! [`topologies::opto_compressor`](crate::circuit::topologies::opto_compressor)
//! is the worked circuit.
//!
//! ```
//! use stave_dsp::circuit::devices::photocell::{Najnudel, Vactrol};
//!
//! let mut cell = Vactrol::new(Najnudel::VTL5C3);
//! cell.prepare(48_000.0);
//! assert!(cell.ohms() > 9e6, "dark: {}", cell.ohms());
//! // 10 mA for half a second: on, within a few milliseconds.
//! let on = (0..24_000).map(|_| cell.step(0.010)).last().unwrap();
//! assert!((2_000.0..3_200.0).contains(&on), "{on}");
//! // Dark again: 100 kohm takes milliseconds, the dark resistance seconds.
//! let after = (0..4_800).map(|_| cell.step(0.0)).last().unwrap();
//! assert!((1e5..2e6).contains(&after), "{after}");
//! ```

mod najnudel;
mod vactrol;

pub use najnudel::{Cell, Coupling, History, Line, Najnudel};
pub use vactrol::{LedDriver, Vactrol};

#[cfg(test)]
mod tests;
