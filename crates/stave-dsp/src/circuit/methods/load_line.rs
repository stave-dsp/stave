//! A node fed from a supply through a load resistor, with a device
//! drawing current from it: a plate, collector or drain. When the node's
//! own capacitance settles far faster than a sample (a triode plate:
//! ~10 ns against ~20 µs), it is in equilibrium every sample, and the
//! physics is one scalar equation, solved by Newton:
//!
//! ```text
//!     (supply − v) / load = I(v)
//! ```

use crate::circuit::methods::newton::{NewtonSettings, Solved};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LoadLine {
    pub supply_volts: f64,
    pub load_ohms: f64,
}

impl LoadLine {
    /// `current(v)` returns the device current at node voltage `v` and
    /// its slope dI/dv. The node is not bounded: a device that only sinks
    /// current keeps it between ground and the supply by itself.
    #[inline]
    pub fn solve(
        &self,
        current: impl Fn(f64) -> (f64, f64),
        seed: f64,
        settings: &NewtonSettings,
    ) -> Solved {
        let residual = |v: f64| {
            let (i, di) = current(v);
            (
                (self.supply_volts - v) / self.load_ohms - i,
                -1.0 / self.load_ohms - di,
            )
        };
        settings.solve_scalar(seed, residual, |candidate, _| candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resistive_device_divides_the_supply() {
        let line = LoadLine {
            supply_volts: 300.0,
            load_ohms: 100e3,
        };
        let device = |v: f64| (v / 100e3, 1.0 / 100e3);
        let s = line.solve(device, 0.0, &NewtonSettings::default());
        assert!(s.converged, "{s:?}");
        assert!((s.value - 150.0).abs() < 1e-9, "{s:?}");
    }

    /// A device that only sinks current needs no rails: from seeds far
    /// outside them, the solution lands between ground and the supply.
    #[test]
    fn a_sinking_device_keeps_the_node_inside_the_supply_unaided() {
        use crate::circuit::devices::triode::koren::Koren;
        let line = LoadLine {
            supply_volts: 300.0,
            load_ohms: 100e3,
        };
        let tube = Koren::T12AX7;
        for vgk in [-60.0, -8.0, -2.0, -0.5, 0.0, 1.0, 5.0] {
            for seed in [-300.0, 0.0, 150.0, 300.0, 900.0] {
                let s = line.solve(
                    |v| tube.plate_current(vgk, v),
                    seed,
                    &NewtonSettings::default(),
                );
                assert!(s.converged, "vgk {vgk} seed {seed}: {s:?}");
                assert!(
                    (0.0..=300.0).contains(&s.value),
                    "vgk {vgk} seed {seed}: {s:?}"
                );
            }
        }
    }
}
