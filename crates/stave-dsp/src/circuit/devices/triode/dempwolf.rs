//! Dempwolf and Zölzer, "A Physically-motivated Triode Model for Circuit
//! Simulations" (DAFx 2011), equations (10)–(12), with grid current:
//!
//! ```text
//!     Ik = G  · (ln(1 + exp(C  · (Va/μ + Vg))) / C )^γ
//!     Ig = Gg · (ln(1 + exp(Cg · Vg))          / Cg)^ξ + Ig0
//!     Ia = Ik − Ig
//! ```
//!
//! Voltages are grid and anode (plate) to cathode; currents in amps.

use super::{TriodeCurrents, TriodeModel};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Dempwolf {
    pub name: &'static str,
    pub g: f64,
    pub mu: f64,
    pub gamma: f64,
    pub c: f64,
    pub gg: f64,
    pub xi: f64,
    pub cg: f64,
    pub ig0: f64,
}

impl Dempwolf {
    /// Table 1, the three measured 12AX7s.
    pub const RSD_1: Self = Self {
        name: "12AX7 RSD-1",
        g: 2.242e-3,
        mu: 103.2,
        gamma: 1.26,
        c: 3.40,
        gg: 6.177e-4,
        xi: 1.314,
        cg: 9.901,
        ig0: 8.025e-8,
    };
    pub const RSD_2: Self = Self {
        name: "12AX7 RSD-2",
        g: 2.173e-3,
        mu: 100.2,
        gamma: 1.28,
        c: 3.19,
        gg: 5.911e-4,
        xi: 1.358,
        cg: 11.76,
        ig0: 4.527e-8,
    };
    pub const EHX_1: Self = Self {
        name: "12AX7 EHX-1",
        g: 1.371e-3,
        mu: 86.9,
        gamma: 1.349,
        c: 4.56,
        gg: 3.263e-4,
        xi: 1.156,
        cg: 11.99,
        ig0: 3.917e-8,
    };
}

/// G · (softplus(C·x) / C)^γ and its derivative in x.
#[inline]
fn smoothed_power(gain: f64, sharpness: f64, exponent: f64, x: f64) -> (f64, f64) {
    let (softplus, sigmoid) = super::softplus(sharpness * x);
    let base = softplus / sharpness;
    if base <= 0.0 {
        return (0.0, 0.0);
    }
    let value = gain * base.powf(exponent);
    (value, exponent * value / base * sigmoid)
}

impl Dempwolf {
    /// Grid current (eq. 11) and its slope, at grid-to-cathode volts.
    #[inline]
    pub fn grid_current(&self, vgk: f64) -> (f64, f64) {
        let (ig, dig) = smoothed_power(self.gg, self.cg, self.xi, vgk);
        (ig + self.ig0, dig)
    }
}

impl TriodeModel for Dempwolf {
    #[inline]
    fn currents(&self, vgk: f64, vpk: f64) -> TriodeCurrents {
        let (ik, dik) = smoothed_power(self.g, self.c, self.gamma, vpk / self.mu + vgk);
        let (ig, dig) = self.grid_current(vgk);
        TriodeCurrents {
            plate: ik - ig,
            grid: ig,
            plate_per_grid_volt: dik - dig,
            plate_per_plate_volt: dik / self.mu,
            grid_per_grid_volt: dig,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUBE: Dempwolf = Dempwolf::RSD_1;

    #[test]
    fn reproduces_the_papers_operating_points() {
        // Figure 6 (plate family) reads about 5.5 mA at Vg = 0.1 V, Va = 200 V.
        let i = TUBE.currents(0.1, 200.0);
        assert!((5.0e-3..6.0e-3).contains(&(i.plate + i.grid)), "{i:?}");
        // Figure 2(b): about 0.6 mA of grid current at Vg = +1 V.
        assert!((0.55e-3..0.7e-3).contains(&TUBE.currents(1.0, 200.0).grid));
        // Figure 2(a): essentially none below -1 V.
        assert!(TUBE.currents(-2.0, 200.0).grid < 1e-6);
    }

    #[test]
    fn partials_are_the_derivatives() {
        let (vg, va, h) = (-0.3, 180.0, 1e-6);
        let at = TUBE.currents(vg, va);
        let dg = (TUBE.currents(vg + h, va).plate - TUBE.currents(vg - h, va).plate) / (2.0 * h);
        let da = (TUBE.currents(vg, va + h).plate - TUBE.currents(vg, va - h).plate) / (2.0 * h);
        let dgg = (TUBE.currents(vg + h, va).grid - TUBE.currents(vg - h, va).grid) / (2.0 * h);
        assert!((at.plate_per_grid_volt - dg).abs() < 1e-6 * dg.abs());
        assert!((at.plate_per_plate_volt - da).abs() < 1e-6 * da.abs());
        assert!((at.grid_per_grid_volt - dgg).abs() < 1e-6 * dgg.abs().max(1e-12));
    }
}
