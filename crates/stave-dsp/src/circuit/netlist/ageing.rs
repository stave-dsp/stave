//! Design-time component drift, applied to an already built unit.
//!
//! [`Drift`] is an explicit constant fractional-rate assumption, not a
//! universal law for aged hardware. The caller supplies a value ratio at a
//! reference duration from measurements or a stated design experiment.
//! Temperature acceleration, ESR, leakage, failures and device wear are not
//! inferred from elapsed time. Unspecified parts remain unchanged.
//!
//! Build manufacturing variation with [`super::tolerance::Tolerances::unit`]
//! first, then always age that baseline to the requested elapsed duration.
//! A pot's track drifts as one part, preserving its wiper position. Drift
//! variation has a separate random stream from manufacturing tolerance.
//!
//! ```
//! use stave_dsp::circuit::netlist::{Netlist, ageing::{Ageing, Drift}};
//! use stave_dsp::units::seconds;
//! let built = Netlist::parse_spice("* RC\nR1 in out 1k\nC1 out 0 100n").unwrap();
//! // Hypothetical measured 2% capacitance loss after 1,000 hours.
//! let profile = Ageing::new().part("c1", Drift::new(0.98, seconds(3_600_000.0)).unwrap());
//! let original = built.clone();
//! let aged = profile.unit(&built, seconds(3_600_000.0), 7, 0).unwrap();
//! assert!((aged.value("c1").unwrap() / 100e-9 - 0.98).abs() < 1e-14);
//! assert_eq!(built, original);
//! ```

use super::tolerance::{Tolerance, Tolerances};
use super::{Netlist, NetlistError};
use crate::units::Seconds;

/// A constant fractional rate, specified by a ratio after a duration.
///
/// The value at elapsed time `t` is `baseline * ratio^(t / duration)`.
/// Ratios below one decay and ratios above one grow. Optional seeded spread
/// multiplies the logarithmic rate, keeping its sign and zero-age identity.
/// No clipping is applied: an unrepresentable result is an error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drift {
    ratio: f64,
    duration: Seconds,
    spread: Tolerance,
}

fn invalid(element: &str, reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: element.into(),
        reason: reason.into(),
    }
}

impl Drift {
    /// A positive finite ratio over a positive finite reference duration.
    pub fn new(ratio: f64, duration: Seconds) -> Result<Self, NetlistError> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(invalid("ageing", "drift ratio must be positive and finite"));
        }
        if !duration.get().is_finite() || duration.get() <= 0.0 {
            return Err(invalid(
                "ageing",
                "reference duration must be positive and finite",
            ));
        }
        Ok(Self {
            ratio,
            duration,
            spread: Tolerance::EXACT,
        })
    }

    /// Fractional variation of the logarithmic drift rate across units.
    /// The same seed, unit and part always use the same rate at every age.
    pub fn spread(mut self, spread: Tolerance) -> Result<Self, NetlistError> {
        if !spread.fraction.is_finite() || !(0.0..1.0).contains(&spread.fraction) {
            return Err(invalid(
                "ageing",
                "rate spread must be finite and in [0, 1)",
            ));
        }
        self.spread = spread;
        Ok(self)
    }

    fn factor(&self, elapsed: Seconds, seed: u64, unit: u64, name: &str) -> f64 {
        if elapsed.get() == 0.0 || self.ratio == 1.0 {
            return 1.0;
        }
        // Domain separation keeps drift and build errors from being identical.
        let rate_scale = self.spread.factor(seed ^ 0x6167_6569_6e67_0001, unit, name);
        (self.ratio.ln() * rate_scale * (elapsed.get() / self.duration.get())).exp()
    }
}

/// Named passive parts' ageing assumptions. No default hardware drift is
/// invented; an empty profile leaves all parts unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ageing {
    parts: Vec<(String, Drift)>,
}

impl Ageing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a resistor, capacitor, inductor, pot or rheostat's drift.
    /// Names are case-insensitive; a later entry replaces an earlier one.
    /// Unknown names (including individual legs of a declared pot) are
    /// refused by [`Self::unit`].
    pub fn part(mut self, name: &str, drift: Drift) -> Self {
        let name = name.to_ascii_lowercase();
        self.parts.retain(|(n, _)| *n != name);
        self.parts.push((name, drift));
        self
    }

    /// Age a built baseline without modifying it. Every changed value is
    /// validated before the new netlist is returned; an error exposes no
    /// partially aged unit. `elapsed` must be nonnegative and finite.
    pub fn unit(
        &self,
        baseline: &Netlist,
        elapsed: Seconds,
        seed: u64,
        unit: u64,
    ) -> Result<Netlist, NetlistError> {
        if !elapsed.get().is_finite() || elapsed.get() < 0.0 {
            return Err(invalid(
                "ageing",
                "elapsed duration must be nonnegative and finite",
            ));
        }
        let parts = Tolerances::new().parts(baseline);
        let mut out = baseline.clone();
        for (name, drift) in &self.parts {
            let part = parts
                .iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| NetlistError::UnknownElement { name: name.clone() })?;
            let factor = drift.factor(elapsed, seed, unit, name);
            if !factor.is_finite() || factor <= 0.0 {
                return Err(invalid(name, "ageing factor is not positive and finite"));
            }
            if factor != 1.0 {
                part.set(&mut out, baseline, part.nominal * factor)?;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
