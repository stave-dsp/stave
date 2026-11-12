//! Norman Koren's triode plate-current law (Glass Audio, 1996):
//!
//! ```text
//!     E1 = (Vpk / Kp) · ln(1 + exp(Kp · (1/μ + Vgk / √(Kvb + Vpk²))))
//!     Ip = 2 · E1^Ex / Kg1          (0 below cutoff)
//! ```
//!
//! It models plate current only; grid current is not part of the law.

/// One tube's Koren model parameters, as a SPICE `.model` line holds them.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Koren {
    pub name: &'static str,
    pub mu: f64,
    pub ex: f64,
    pub kg1: f64,
    pub kp: f64,
    pub kvb: f64,
    /// The datasheet's typical operating bias in a 300 V / 100 kΩ stage.
    pub typical_grid_bias: f64,
}

impl Koren {
    pub const T12AX7: Self = Self {
        name: "12AX7",
        mu: 100.0,
        ex: 1.4,
        kg1: 1060.0,
        kp: 600.0,
        kvb: 300.0,
        typical_grid_bias: -1.5,
    };
    pub const T12AT7: Self = Self {
        name: "12AT7",
        mu: 60.0,
        ex: 1.35,
        kg1: 460.0,
        kp: 300.0,
        kvb: 300.0,
        typical_grid_bias: -2.0,
    };
    pub const T12AU7: Self = Self {
        name: "12AU7",
        mu: 21.5,
        ex: 1.3,
        kg1: 1180.0,
        kp: 84.0,
        kvb: 300.0,
        typical_grid_bias: -6.0,
    };
    pub const T6DJ8: Self = Self {
        name: "6DJ8",
        mu: 33.0,
        ex: 1.3,
        kg1: 330.0,
        kp: 320.0,
        kvb: 300.0,
        typical_grid_bias: -2.5,
    };
    pub const T6SN7: Self = Self {
        name: "6SN7",
        mu: 20.0,
        ex: 1.4,
        kg1: 1180.0,
        kp: 86.0,
        kvb: 300.0,
        typical_grid_bias: -6.0,
    };

    pub const ALL: [Self; 5] = [
        Self::T12AX7,
        Self::T12AT7,
        Self::T12AU7,
        Self::T6DJ8,
        Self::T6SN7,
    ];
    pub const NAMES: [&'static str; 5] = names(Self::ALL);
}

const fn names<const N: usize>(tubes: [Koren; N]) -> [&'static str; N] {
    let mut out = [""; N];
    let mut i = 0;
    while i < N {
        out[i] = tubes[i].name;
        i += 1;
    }
    out
}

/// Plate current and its partial derivatives, in amps and siemens.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PlateCurrent {
    pub amps: f64,
    /// ∂Ip/∂Vgk, the transconductance gm.
    pub per_grid_volt: f64,
    /// ∂Ip/∂Vpk, the plate conductance 1/rp.
    pub per_plate_volt: f64,
}

impl Koren {
    /// Current and both partials; all 0 below cutoff.
    #[inline]
    pub fn plate(&self, vgk: f64, vpk: f64) -> PlateCurrent {
        if vpk <= 0.0 {
            return PlateCurrent::default();
        }
        let s = (self.kvb + vpk * vpk).sqrt();
        let g = self.kp * (1.0 / self.mu + vgk / s);
        let (softplus, sigmoid) = super::softplus(g);
        let e1 = (vpk / self.kp) * softplus;
        if e1 <= 0.0 {
            return PlateCurrent::default();
        }
        let di_de1 = 2.0 * self.ex * e1.powf(self.ex - 1.0) / self.kg1;
        let de1_dvgk = vpk * sigmoid / s;
        let dg_dvpk = -self.kp * vgk * vpk / (s * s * s);
        let de1_dvpk = softplus / self.kp + (vpk / self.kp) * sigmoid * dg_dvpk;
        PlateCurrent {
            amps: 2.0 * e1.powf(self.ex) / self.kg1,
            per_grid_volt: di_de1 * de1_dvgk,
            per_plate_volt: di_de1 * de1_dvpk,
        }
    }
}

impl Koren {
    /// (Ip, dIp/dVpk) in amps and siemens; both 0 below cutoff.
    #[inline]
    pub fn plate_current(&self, vgk: f64, vpk: f64) -> (f64, f64) {
        let p = self.plate(vgk, vpk);
        (p.amps, p.per_plate_volt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slope_is_the_derivative_of_the_current() {
        let tube = Koren::T12AX7;
        for (vgk, vpk) in [(-1.5, 150.0), (-0.5, 80.0), (-3.0, 250.0)] {
            let (_, slope) = tube.plate_current(vgk, vpk);
            let h = 1e-4;
            let numeric = (tube.plate_current(vgk, vpk + h).0 - tube.plate_current(vgk, vpk - h).0)
                / (2.0 * h);
            assert!(
                (slope - numeric).abs() < 1e-6 * numeric.abs().max(1e-9),
                "{vgk} {vpk}"
            );
        }
    }

    #[test]
    fn transconductance_is_the_grid_derivative() {
        let tube = Koren::T12AX7;
        let (vgk, vpk, h) = (-1.2, 180.0, 1e-5);
        let p = tube.plate(vgk, vpk);
        let numeric = (tube.plate(vgk + h, vpk).amps - tube.plate(vgk - h, vpk).amps) / (2.0 * h);
        assert!(
            (p.per_grid_volt - numeric).abs() < 1e-6 * numeric,
            "{p:?} vs {numeric}"
        );
        assert_eq!(p.amps, tube.plate_current(vgk, vpk).0);
        assert_eq!(p.per_plate_volt, tube.plate_current(vgk, vpk).1);
        let mu = p.per_grid_volt / p.per_plate_volt;
        assert!((80.0..110.0).contains(&mu), "μ = gm·rp = {mu}");
    }

    #[test]
    fn cut_off_below_the_knee_and_conducting_above_it() {
        let tube = Koren::T12AX7;
        assert!(tube.plate_current(-8.0, 100.0).0 < 1e-9);
        let ma = tube.plate_current(0.0, 200.0).0 * 1e3;
        assert!((1.0..5.0).contains(&ma), "{ma} mA");
    }
}
