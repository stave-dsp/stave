//! Prewarping the bilinear transform (DSP.md R12): the trapezoidal rule
//! every reactance is discretised by — the DK method's companions, the
//! wave-digital port resistances — maps the analog frequency `ωa` to the
//! digital `ωd` by
//!
//! ```text
//!     ωa = k·tan(ωd·T/2)          k = 2/T: the plain trapezoidal rule
//! ```
//!
//! so a circuit run at the host rate has its response bent towards
//! Nyquist: a 7.2 kHz RC corner at 48 kHz reads 3.9 dB low at 16 kHz.
//! Prewarping chooses `k = ω₀ / tan(ω₀·T/2)` instead, which makes the
//! digital response exact at `ω₀` — for every reactance at once, so the
//! whole circuit's response is the analog one at `f₀`. That is the same
//! discretisation at a different design step,
//!
//! ```text
//!     Td = 2/k = tan(π·f₀·T) / (π·f₀)
//! ```
//!
//! which is how the methods apply it ([`design_step`]): every `2C/T` and
//! `2L/T` becomes `2C/Td` and `2L/Td`, nothing else changes, and the
//! sources still run on the real clock.
//!
//! # The trade-off
//!
//! Prewarping moves the error, it does not remove it: the map is still
//! a tangent, so frequencies below `f₀` read *lower* than the analog
//! ones by the factor `T/Td·tan(πfT)/(πfT)`, near `T/Td` at low
//! frequencies. A 16 kHz prewarp at 48 kHz scales the low band by 0.60:
//! a 20 Hz coupling corner moves to 33 Hz, and the RC clipper's 7.2 kHz
//! corner reads 1.5 dB off. A prewarp near the top of the band suits a
//! circuit whose character is set there (a treble booster's peak); a
//! circuit with corners across the band is better left unwarped and
//! oversampled — oversampling by `m` moves the whole tangent up by `m`
//! (16 kHz at 4× 48 kHz reads 0.2 dB off on the RC corner) at `m` times
//! the cost. A one-pole's error at the top of the band falls by about
//! `m²`; prewarping makes it zero at one frequency and spreads the rest.
//!
//! Every characterised circuit's error at 44.1, 48 and 96 kHz with and
//! without each option is in DSP.md R12; `crates/stave-test/tests/shipping_rate.rs`
//! measures it.

use std::f64::consts::PI;

/// The step a method discretises its reactances for when it runs at
/// `step` seconds and prewarps to `hz` (`None`: the plain trapezoidal
/// rule, `Td = T`). A prewarp at or above Nyquist is ignored: there is
/// no digital frequency to map it to.
pub fn design_step(step: f64, hz: Option<f64>) -> f64 {
    match hz {
        Some(f) if f > 0.0 && f * step < 0.5 => (PI * f * step).tan() / (PI * f),
        _ => step,
    }
}

/// The digital angle (radians a sample) where a method discretised for
/// the step `design` puts the analog frequency `hz`: `2·atan(π·f·Td)`,
/// the inverse of the tangent map, so an `analog_response` stays exact
/// under a prewarp.
pub fn digital_angle(hz: f64, design: f64) -> f64 {
    2.0 * (PI * hz * design).atan()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-pole RC low-pass, discretised by the trapezoidal rule at
    /// `design` and run at `step`, read at `hz`.
    fn one_pole(rc: f64, step: f64, design: f64, hz: f64) -> f64 {
        // y[n] = b·(x[n] + x[n−1]) + a·y[n−1] with k = 2/design.
        let k = 2.0 / design;
        let (b, a) = (1.0 / (1.0 + k * rc), (k * rc - 1.0) / (k * rc + 1.0));
        let w = 2.0 * PI * hz * step;
        let (re, im) = (1.0 + w.cos(), -w.sin());
        let (dre, dim) = (1.0 - a * w.cos(), a * w.sin());
        (b * b * (re * re + im * im) / (dre * dre + dim * dim)).sqrt()
    }

    #[test]
    fn a_prewarped_one_pole_is_exact_at_its_frequency() {
        let (rc, step) = (2.2e3 * 10e-9, 1.0 / 48_000.0);
        let analog = |hz: f64| 1.0 / (1.0 + (2.0 * PI * hz * rc).powi(2)).sqrt();
        let plain = one_pole(rc, step, step, 16e3);
        let warped = one_pole(rc, step, design_step(step, Some(16e3)), 16e3);
        let db = |x: f64| 20.0 * x.log10();
        assert!((db(plain) - db(analog(16e3))).abs() > 3.5, "plain: {}", db(plain) - db(analog(16e3)));
        assert!((db(warped) - db(analog(16e3))).abs() < 1e-9);
        // And the error moved down the band.
        let low = one_pole(rc, step, design_step(step, Some(16e3)), 7.2e3);
        assert!((db(low) - db(analog(7.2e3))).abs() > 1.0);
    }

    #[test]
    fn no_prewarp_and_nyquist_leave_the_step() {
        let step = 1.0 / 48_000.0;
        assert_eq!(design_step(step, None), step);
        assert_eq!(design_step(step, Some(24e3)), step);
        assert_eq!(design_step(step, Some(0.0)), step);
        // A low prewarp is nearly the plain rule.
        assert!((design_step(step, Some(1.0)) / step - 1.0).abs() < 1e-8);
    }
}
