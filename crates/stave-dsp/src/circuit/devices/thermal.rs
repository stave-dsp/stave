//! Temperature in devices (DSP.md H9): what a degree does to a junction
//! circuit. A junction's temperature law is the diode card's
//! ([`Shockley::at_celsius`](super::diode::Shockley::at_celsius), the bias
//! drift of anything biased by a junction); an exponential converter's is
//! [`Expo`], the pitch drift of a VCO. Drive either from a slow random
//! temperature ([`OrnsteinUhlenbeck`](crate::primitives::control::drift::OrnsteinUhlenbeck)).

/// 0 °C in kelvin.
pub const ZERO_CELSIUS: f64 = 273.15;

/// A VCO's exponential converter: a matched transistor pair whose
/// collector currents are in the ratio `exp(ΔV/Vt)`, `Vt = kT/q`.
///
/// # Models
/// The temperature dependence of a bipolar exponential converter (the
/// core of a 1 V/octave VCO): its control scaled to one octave per
/// `Vt(T₀)·ln 2` at the reference temperature `T₀`, so at `T` an
/// intended `n` octaves become `n·T₀/T` — about −0.33 %/K of the
/// interval, −4 cents/K per octave above the reference current. A
/// tempco resistor of `α` ppm/K in the scaling network multiplies the
/// control by `1 + α·(T − T₀)`; `α = 1/T₀` (≈ +3300 ppm/K) cancels the
/// `1/T` exactly.
///
/// # Derivation
/// The Ebers–Moll pair: `I₂/I₁ = exp((V_BE2 − V_BE1)/Vt)`, so the pitch
/// ratio is `2^(n·(T₀/T)·(1 + α(T − T₀)))`. Saturation-current mismatch
/// and bulk resistance (the converter's other errors) are not modelled.
///
/// # Verified
/// `crates/stave-dsp/src/circuit/devices/thermal/tests.rs::expo_drifts_as_kt_over_q`
/// (the ratio against `exp(ΔV·q/kT)` from the control voltage directly,
/// to 1e-12; −1200·n/T₀ cents/K at the reference; an ideal tempco
/// cancels to 1e-12, a +3500 ppm/K part leaves its analytic residue).
///
/// # Cost
/// One `exp2` per call; not a stage, so no datasheet line.
///
/// # Aliasing
/// None: a control law.
///
/// # Valid ranges
/// Junction temperatures −55 to +125 °C (a silicon part's operating
/// range); any interval.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Expo {
    reference_kelvin: f64,
    tempco_per_kelvin: f64,
}

impl Expo {
    /// Trimmed at `reference_celsius`, no temperature compensation.
    pub fn uncompensated(reference_celsius: f64) -> Self {
        Self::new(reference_celsius, 0.0)
    }

    /// Trimmed at `reference_celsius`, its scaling compensated by a
    /// tempco resistor of `tempco_ppm` per kelvin.
    pub fn new(reference_celsius: f64, tempco_ppm: f64) -> Self {
        Self {
            reference_kelvin: reference_celsius + ZERO_CELSIUS,
            tempco_per_kelvin: tempco_ppm * 1e-6,
        }
    }

    /// The tempco that cancels the converter's `1/T`, ppm/K.
    pub fn ideal_tempco_ppm(reference_celsius: f64) -> f64 {
        1e6 / (reference_celsius + ZERO_CELSIUS)
    }

    /// The fraction of each intended octave delivered at `celsius`.
    #[inline]
    pub fn gain(&self, celsius: f64) -> f64 {
        let t = celsius + ZERO_CELSIUS;
        let t0 = self.reference_kelvin;
        t0 / t * (1.0 + self.tempco_per_kelvin * (t - t0))
    }

    /// The frequency ratio for `octaves` above the reference current at
    /// `celsius`.
    #[inline]
    pub fn ratio(&self, octaves: f64, celsius: f64) -> f64 {
        (octaves * self.gain(celsius)).exp2()
    }

    /// How far `octaves` lands from where it was trimmed, in cents.
    pub fn cents(&self, octaves: f64, celsius: f64) -> f64 {
        1200.0 * octaves * (self.gain(celsius) - 1.0)
    }
}

#[cfg(test)]
mod tests;
