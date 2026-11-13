//! First-order ADAA at a wave-digital root (Albertini, Bernardini &
//! Sarti, DAFx 2020 and JAES 2021): the root's reflected wave replaced by
//! its mean over the line from the last arriving wave to this one, in
//! closed form from the root's solved voltages
//! ([`methods::adaa`](crate::circuit::methods::adaa) has the derivation).
//!
//! The tree around the root does its part: the waves its leaves send up
//! enter the backward scan averaged with their last values (the
//! kernel's half-sample delay, applied to the linear elements), and its
//! capacitors' port resistances are designed for `1.5·T`. A root solving
//! `a = v + R·i(v)` reflects `b = a − 2R·i`, so under ADAA
//!
//! ```text
//!     b̃ = ā − 2R·ĩ      ā = ½(a[n] + a[n−1]),  ĩ = one_port_mean(ΔI/Δv, Δi/Δv, ī, −R)
//! ```
//!
//! and the root's voltage is `½(ā + b̃) = ā − R·ĩ`.
//!
//! ```
//! use stave_dsp::circuit::devices::diode::Shockley;
//! use stave_dsp::circuit::methods::wdf::adaa::{Antiparallel, Root};
//! use stave_dsp::circuit::methods::wright_omega::WrightOmega;
//!
//! // Two samples into an antiparallel pair behind 1 kΩ: the mean
//! // reflection sits between the two plain reflections.
//! let (pair, ohms) = (Shockley::N4148, 1e3);
//! let mut root = Root::default();
//! let mut plain = [0.0; 2];
//! let mut mean = 0.0;
//! for (n, a) in [1.0, 3.0].into_iter().enumerate() {
//!     let b = WrightOmega::reflect_pair(&pair, a, ohms);
//!     plain[n] = b;
//!     mean = root.reflect(&Antiparallel(pair), a, 0.5 * (a + b), ohms).reflected;
//! }
//! assert!(plain[1] < mean && mean < plain[0], "{plain:?} {mean}");
//! ```

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::adaa::one_port_mean;

/// A one-port root's current law, as the stateful kernel needs it: the
/// current, and the divided differences of its antiderivative and of
/// itself (exact where the two voltages meet).
pub trait Law {
    fn current(&self, volts: f64) -> f64;
    /// `(I(b) − I(a))/(b − a)` for `I = ∫ i dv`; `i(a)` where `a = b`.
    fn mean_current(&self, a: f64, b: f64) -> f64;
    /// `(i(b) − i(a))/(b − a)`; the conductance where `a = b`.
    fn secant(&self, a: f64, b: f64) -> f64;
}

/// One junction, breakdown included: its antiderivative in closed form
/// (`Junction::antiderivative`).
impl Law for Shockley {
    fn current(&self, volts: f64) -> f64 {
        self.junction().current_and_conductance(volts).0
    }
    fn mean_current(&self, a: f64, b: f64) -> f64 {
        self.junction().mean_current(a, b)
    }
    fn secant(&self, a: f64, b: f64) -> f64 {
        self.junction().secant(a, b)
    }
}

/// An identical antiparallel pair, `2·Is·sinh(v/(N·Vt))`, both junctions
/// counted.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Antiparallel(pub Shockley);

impl Law for Antiparallel {
    fn current(&self, volts: f64) -> f64 {
        self.0.pair_current(volts)
    }
    fn mean_current(&self, a: f64, b: f64) -> f64 {
        self.0.pair_mean_current(a, b)
    }
    fn secant(&self, a: f64, b: f64) -> f64 {
        self.0.pair_secant(a, b)
    }
}

/// What a root under ADAA hands the backward scan.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Reflection {
    /// `b̃`, the mean reflected wave.
    pub reflected: f64,
    /// `ā`, the mean arriving wave (the up wave, synchronized).
    pub arriving: f64,
    /// The root's voltage at `n − ½`, `½(ā + b̃)`.
    pub volts: f64,
}

/// A root's history under ADAA: the last arriving wave, solved voltage
/// and current. At rest by default.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Root {
    wave: f64,
    volts: f64,
    amps: f64,
}

impl Root {
    /// The mean reflection for an arriving `wave` whose root equation
    /// `wave = v + R·i(v)` the root's method solved at `volts`, through
    /// port resistance `ohms`.
    pub fn reflect(&mut self, law: &impl Law, wave: f64, volts: f64, ohms: f64) -> Reflection {
        let amps = law.current(volts);
        let (v0, v1) = (self.volts, volts);
        let mean = one_port_mean(
            law.mean_current(v0, v1),
            law.secant(v0, v1),
            0.5 * (self.amps + amps),
            -ohms,
        );
        let arriving = 0.5 * (self.wave + wave);
        *self = Self { wave, volts, amps };
        Reflection {
            reflected: arriving - 2.0 * ohms * mean,
            arriving,
            volts: arriving - ohms * mean,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::methods::wright_omega::WrightOmega;

    /// A slow wave is a steady one: the mean reflection is the plain one
    /// half a sample back, to second order in the step.
    #[test]
    fn a_slow_wave_reflects_as_without_adaa() {
        let (diode, ohms) = (Shockley::N4148, 2.2e3);
        let mut root = Root::default();
        let mut worst: f64 = 0.0;
        for n in 0..2_000 {
            let a = 2.0 * (n as f64 * 1e-3).sin();
            let b = WrightOmega::reflect_single(&diode, a, ohms);
            let r = root.reflect(&diode, a, 0.5 * (a + b), ohms);
            let half_back = 2.0 * ((n as f64 - 0.5) * 1e-3).sin();
            let expected = WrightOmega::reflect_single(&diode, half_back, ohms);
            if n > 0 {
                worst = worst.max((r.reflected - expected).abs());
            }
        }
        assert!(worst < 1e-4, "{worst}");
    }

    /// The single-diode closed form solves the root equation.
    #[test]
    fn the_single_diode_reflection_solves_its_root() {
        let (diode, ohms) = (Shockley::N4148, 4.7e3);
        for a in [-10.0, -0.3, 0.0, 0.4, 0.9, 25.0] {
            let b = WrightOmega::reflect_single(&diode, a, ohms);
            let (v, i) = (0.5 * (a + b), (a - b) / (2.0 * ohms));
            assert!((i - diode.current(v)).abs() < 1e-9 * (1.0 + i.abs()), "{a}: {i} vs {}", diode.current(v));
        }
    }
}
