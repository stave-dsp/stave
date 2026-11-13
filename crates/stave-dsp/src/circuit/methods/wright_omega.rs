//! The Wright omega function: the w with w + ln(w) = x. It closes the
//! Shockley diode's implicit equation in the wave domain.

use crate::circuit::devices::diode::Shockley;

/// Accurate to f64 rounding: an asymptotic starting guess, then Newton on
/// w + ln w − x until the step is below rounding (quadratic, so a few
/// steps; capped in case of a pathological input).
const MAX_STEPS: u32 = 12;

pub fn wright_omega(x: f64) -> f64 {
    let mut w = if x < -2.0 {
        x.exp()
    } else if x < 1.0 {
        // Interpolates the two asymptotes through ω(1) = 1.
        let e = x.exp();
        e / (1.0 + e) * 2.0 * (0.5 + 0.1 * (x + 2.0))
    } else {
        x - x.ln()
    };
    for _ in 0..MAX_STEPS {
        let step = (w + w.ln() - x) * w / (1.0 + w);
        w -= step;
        if step.abs() <= 4.0 * f64::EPSILON * w {
            break;
        }
    }
    w
}

/// The closed-form method for a diode pair at a wave-digital root
/// (Werner, Nangia, Smith, Abel 2015): whichever diode conducts, alone,
/// through [`wright_omega`]. No iterations; the off diode's leakage is
/// ignored.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WrightOmega;

impl WrightOmega {
    /// The wave an antiparallel `diode` pair reflects for an arriving
    /// `wave` through port resistance `ohms`.
    #[inline]
    pub fn reflect_pair(diode: &Shockley, wave: f64, ohms: f64) -> f64 {
        let (is, knee) = (diode.saturation_amps, diode.knee_volts());
        let r_is = ohms * is;
        let magnitude = wave.abs();
        let w = wright_omega((r_is / knee).ln() + (magnitude + r_is) / knee);
        wave.signum() * (magnitude + 2.0 * r_is - 2.0 * knee * w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_its_defining_equation_across_the_range() {
        for x in [-30.0, -5.0, -2.0, -0.5, 0.0, 1.0, 3.0, 20.0, 200.0, 2_000.0] {
            let w = wright_omega(x);
            assert!(w > 0.0, "{x}");
            assert!(
                (w + w.ln() - x).abs() < 1e-12 * x.abs().max(1.0),
                "{x}: {w}"
            );
        }
        assert!((wright_omega(1.0) - 1.0).abs() < 1e-14);
    }
}
