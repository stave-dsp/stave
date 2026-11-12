//! A tube amp's B+ supply: a full-wave rectifier (diode plus the
//! rectifier's own resistance — a few ohms for silicon, a few hundred
//! for a valve rectifier) charging a reservoir capacitor, then a choke
//! (or resistor) into a filter capacitor that feeds the load. Driven by
//! the load's current each sample, it returns the B+ the load sees: sag
//! under draw, ripple at twice the mains frequency, the choke's ring —
//! all from the circuit, none declared.
//!
//! Per sample the capacitors and choke are SPICE's trapezoidal
//! companions; the linear network seen from the reservoir reduces to
//! `Vr = A + Rth·Id`, so the only nonlinear unknown is the rectifier's
//! voltage, solved by Newton with SPICE's junction limiting.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::kernel::node::{Stage, Tail};

/// Between the reservoir and the filter capacitor.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SupplyFilter {
    Choke { henries: f64, ohms: f64 },
    Resistor { ohms: f64 },
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PowerSupplyConfig {
    pub mains_hz: f64,
    /// Peak of each half of the secondary.
    pub secondary_peak_volts: f64,
    pub rectifier: Shockley,
    /// Rectifier plus transformer winding resistance.
    pub rectifier_ohms: f64,
    pub reservoir_farads: f64,
    pub filter: SupplyFilter,
    pub filter_farads: f64,
    pub newton: NewtonSettings,
}

impl PowerSupplyConfig {
    /// Silicon bridge, CLC with a 10 H choke, 450 V secondary peak.
    pub const SILICON_CHOKE: Self = Self {
        mains_hz: 60.0,
        secondary_peak_volts: 450.0,
        rectifier: Shockley::D1N4007,
        rectifier_ohms: 20.0,
        reservoir_farads: 32e-6,
        filter: SupplyFilter::Choke {
            henries: 10.0,
            ohms: 100.0,
        },
        filter_farads: 32e-6,
        newton: NewtonSettings::DEFAULT,
    };
}

/// A [`Stage`] from load current to supply voltage: `tick` takes the
/// amps the load draws this sample and returns the B+ it sees
/// ([`tick_f64`](PowerSupply::tick_f64) is the same step in `f64`).
pub struct PowerSupply {
    config: PowerSupplyConfig,
    sample_rate: f32,
    step_seconds: f64,
    phase: f64,
    reservoir_volts: f64,
    reservoir_amps: f64,
    filter_volts: f64,
    filter_amps: f64,
    choke_amps: f64,
    choke_volts: f64,
    diode_volts: f64,
    stats: NewtonStats,
}

impl PowerSupply {
    pub fn new(config: PowerSupplyConfig) -> Self {
        Self {
            config,
            sample_rate: 48_000.0,
            step_seconds: 1.0 / 48_000.0,
            phase: 0.0,
            reservoir_volts: 0.0,
            reservoir_amps: 0.0,
            filter_volts: 0.0,
            filter_amps: 0.0,
            choke_amps: 0.0,
            choke_volts: 0.0,
            diode_volts: 0.0,
            stats: NewtonStats::default(),
        }
    }

    pub fn configure(&mut self, config: &PowerSupplyConfig) {
        self.config = *config;
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    pub fn reservoir_volts(&self) -> f64 {
        self.reservoir_volts
    }

    /// Run from the current state with a steady load for `seconds` and
    /// return the mean B+ over the last mains half-cycle (the ripple's
    /// period): the supply's settled voltage at that load.
    pub fn settle(&mut self, load_amps: f64, seconds: f64) -> f64 {
        let rate = 1.0 / self.step_seconds;
        let ripple_period = (rate / (2.0 * self.config.mains_hz)).round() as usize;
        let total = (seconds * rate) as usize;
        let mut sum = 0.0;
        for i in 0..total {
            let b = self.tick_f64(load_amps);
            if i + ripple_period >= total {
                sum += b;
            }
        }
        sum / ripple_period as f64
    }

    /// One sample with `load_amps` drawn from B+; returns B+.
    pub fn tick_f64(&mut self, load_amps: f64) -> f64 {
        let c = &self.config;
        let t = self.step_seconds;
        self.phase = (self.phase + c.mains_hz * t).fract();
        let source = c.secondary_peak_volts * (core::f64::consts::TAU * self.phase).sin().abs();

        // Trapezoidal companions: i = G·v − H for each capacitor.
        let g1 = 2.0 * c.reservoir_farads / t;
        let h1 = g1 * self.reservoir_volts + self.reservoir_amps;
        let g2 = 2.0 * c.filter_farads / t;
        let h2 = g2 * self.filter_volts + self.filter_amps;
        // The series branch: i = Gl·(vr − vb) + Jl.
        let (gl, jl) = match c.filter {
            SupplyFilter::Resistor { ohms } => (1.0 / ohms, 0.0),
            SupplyFilter::Choke { henries, ohms } => {
                let k = t / (2.0 * henries);
                let d = 1.0 + k * ohms;
                (k / d, (self.choke_amps + k * self.choke_volts) / d)
            }
        };
        // Reservoir and filter KCL with rectifier current `id` injected:
        //   (g1 + gl)·vr − gl·vb = id + h1 − jl
        //   −gl·vr + (g2 + gl)·vb = jl + h2 − load
        let det = (g1 + gl) * (g2 + gl) - gl * gl;
        let filter_rhs = jl + h2 - load_amps;
        let vr_at = |id: f64| ((id + h1 - jl) * (g2 + gl) + gl * filter_rhs) / det;
        let (base, per_amp) = (vr_at(0.0), (g2 + gl) / det);

        // The rectifier: source − vd − (R + Rth)·Id(vd) − base = 0.
        let diode = c.rectifier;
        let series = c.rectifier_ohms + per_amp;
        let residual = |vd: f64| {
            let (id, gd) = diode.current_and_conductance(vd);
            (source - vd - series * id - base, -1.0 - series * gd)
        };
        let solved = c
            .newton
            .solve_scalar(self.diode_volts, residual, |new, old| {
                diode.limit_step(new, old)
            });
        self.stats.record_solve(&solved);
        self.diode_volts = solved.value;
        // KCL at the converged point (residual = 0).
        let id = (source - solved.value - base) / series;

        let vr = base + per_amp * id;
        let vb = (filter_rhs + gl * vr) / (g2 + gl);
        let il = gl * (vr - vb) + jl;
        self.reservoir_amps = g1 * vr - h1;
        self.filter_amps = g2 * vb - h2;
        if let SupplyFilter::Choke { ohms, .. } = c.filter {
            self.choke_volts = vr - vb - ohms * il;
        }
        self.choke_amps = il;
        self.reservoir_volts = vr;
        self.filter_volts = vb;
        vb
    }
}

impl Stage for PowerSupply {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.step_seconds = 1.0 / f64::from(sample_rate);
        self.reset();
    }

    /// Discharged, mains at zero: switch-on.
    fn reset(&mut self) {
        self.phase = 0.0;
        self.reservoir_volts = 0.0;
        self.reservoir_amps = 0.0;
        self.filter_volts = 0.0;
        self.filter_amps = 0.0;
        self.choke_amps = 0.0;
        self.choke_volts = 0.0;
        self.diode_volts = 0.0;
    }

    /// Load amps in, B+ volts out.
    fn tick(&mut self, load_amps: f32) -> f32 {
        self.tick_f64(f64::from(load_amps)) as f32
    }

    /// The supply is driven by the mains, not by its input: its output
    /// never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rectifier spends most of each cycle reverse-biased; Newton must
    /// converge there in a step or two at the host rate, never at its cap.
    #[test]
    fn the_rectifier_converges_at_the_host_rate() {
        let mut supply = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
        supply.prepare(48_000.0);
        for _ in 0..48_000 {
            supply.tick_f64(0.03);
        }
        let stats = supply.stats();
        assert_eq!(stats.capped, 0);
        assert!(
            (stats.iterations as f64) < 3.0 * stats.solves as f64,
            "{stats:?}"
        );
    }
}
