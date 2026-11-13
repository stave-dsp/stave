//! The diode family: the [`Diode`] trait and its models. The one model
//! today is the Shockley junction, i = Is·(exp(v / (n·Vt)) − 1).

/// The diode family: a junction's current and conductance at a voltage,
/// and how a Newton step on that voltage is bounded. Every diode model
/// implements it, so a circuit solver takes any.
pub trait Diode: Copy {
    /// `(current, conductance)` at `volts` (anode to cathode).
    fn current_and_conductance(&self, volts: f64) -> (f64, f64);

    /// Bound a Newton step from `old` to `new` without moving the
    /// solution (SPICE's `pnjlim` for a junction).
    fn limit_step(&self, new: f64, old: f64) -> f64;
}

impl Diode for Shockley {
    #[inline]
    fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        Shockley::current_and_conductance(self, volts)
    }

    #[inline]
    fn limit_step(&self, new: f64, old: f64) -> f64 {
        Shockley::limit_step(self, new, old)
    }
}

/// The Shockley junction: i = Is·(exp(v / (n·Vt)) − 1), with SPICE's
/// junction limiting. Series resistance belongs to the circuit (a
/// netlist's diode `Rs` becomes a resistor).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Shockley {
    pub saturation_amps: f64,
    pub emission: f64,
    pub thermal_volts: f64,
}

/// kT/q at 27 °C (300.15 K), SPICE's default temperature, from the SI-exact
/// Boltzmann constant and elementary charge.
pub const THERMAL_VOLTS_27C: f64 = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;

impl Shockley {
    /// The SPICE model `D(Is=2.52n N=1.752)`.
    pub const D1N4148: Self = Self {
        saturation_amps: 2.52e-9,
        emission: 1.752,
        thermal_volts: THERMAL_VOLTS_27C,
    };

    /// The SPICE model `D(Is=7.03n N=1.8)` (1N4007, a power rectifier;
    /// its 34 mΩ bulk resistance belongs to the circuit's series ohms).
    pub const D1N4007: Self = Self {
        saturation_amps: 7.03e-9,
        emission: 1.8,
        thermal_volts: THERMAL_VOLTS_27C,
    };

    pub fn knee_volts(&self) -> f64 {
        self.emission * self.thermal_volts
    }

    /// Above this voltage a Newton step is compressed (SPICE's `vcrit`).
    pub fn critical_volts(&self) -> f64 {
        let knee = self.knee_volts();
        knee * (knee / (std::f64::consts::SQRT_2 * self.saturation_amps)).ln()
    }

    /// SPICE's `pnjlim`: a step from `old` to `new` that climbs past the
    /// critical voltage is taken logarithmically, so a Newton iterate can
    /// never jump far up the exponential; steps elsewhere — reverse bias
    /// included, where the junction is a constant leakage — are untouched.
    pub fn limit_step(&self, new: f64, old: f64) -> f64 {
        let (knee, critical) = (self.knee_volts(), self.critical_volts());
        if new <= critical || (new - old).abs() <= 2.0 * knee {
            return new;
        }
        if old > 0.0 {
            let arg = 1.0 + (new - old) / knee;
            if arg > 0.0 {
                old + knee * arg.ln()
            } else {
                critical
            }
        } else {
            knee * (new / knee).ln()
        }
    }

    /// `limit_step` for an antiparallel pair: either polarity climbs an
    /// exponential, so negative steps are limited in mirror.
    pub fn limit_pair_step(&self, new: f64, old: f64) -> f64 {
        if new < 0.0 {
            -self.limit_step(-new, -old)
        } else {
            self.limit_step(new, old)
        }
    }

    pub fn current(&self, volts: f64) -> f64 {
        self.saturation_amps * (volts / self.knee_volts()).exp_m1()
    }

    pub fn conductance(&self, volts: f64) -> f64 {
        self.saturation_amps / self.knee_volts() * (volts / self.knee_volts()).exp()
    }

    /// `(current, conductance)` from one exponential — the pair a Newton
    /// residual needs at every iterate.
    #[inline]
    pub fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        let knee = self.knee_volts();
        let em1 = (volts / knee).exp_m1();
        (
            self.saturation_amps * em1,
            self.saturation_amps / knee * (em1 + 1.0),
        )
    }

    /// The antiparallel pair's `(current, conductance)` from one
    /// exponential and its reciprocal.
    #[inline]
    pub fn pair_current_and_conductance(&self, volts: f64) -> (f64, f64) {
        let knee = self.knee_volts();
        let e = (volts / knee).exp();
        let inv = 1.0 / e;
        (
            self.saturation_amps * (e - inv),
            self.saturation_amps / knee * (e + inv),
        )
    }

    /// An antiparallel pair: odd in `volts`.
    pub fn pair_current(&self, volts: f64) -> f64 {
        2.0 * self.saturation_amps * (volts / self.knee_volts()).sinh()
    }

    pub fn pair_conductance(&self, volts: f64) -> f64 {
        2.0 * self.saturation_amps / self.knee_volts() * (volts / self.knee_volts()).cosh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_is_two_opposed_diodes() {
        let d = Shockley::D1N4148;
        for v in [-0.6, -0.1, 0.0, 0.3, 0.7] {
            let pair = d.current(v) - d.current(-v);
            assert!((d.pair_current(v) - pair).abs() < 1e-15 + 1e-9 * pair.abs());
        }
    }
}
