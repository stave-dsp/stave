//! Input resistor into a capacitor to ground, shunted by an antiparallel
//! diode pair; output across the capacitor. A wave-digital tree with
//! the pair at the root, solved per sample by Newton.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::circuit::methods::wdf::{Capacitor, OnePort, Parallel, ResistiveSource};
use crate::circuit::methods::wright_omega::wright_omega;
use crate::kernel::node::{Stage, Tail};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DiodeClipperConfig {
    pub input_ohms: f64,
    pub capacitor_farads: f64,
    pub diode: Shockley,
    pub solver: PairSolver,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PairSolver {
    /// Newton on the exact pair, with SPICE's junction limiting.
    Newton(NewtonSettings),
    /// Werner et al.'s closed form: whichever diode conducts, alone.
    /// No iterations; the off diode's leakage is ignored.
    WrightOmega,
}

pub struct DiodeClipper {
    tree: Parallel<ResistiveSource, Capacitor>,
    sample_rate: f32,
    diode: Shockley,
    solver: PairSolver,
    volts: f64,
    stats: NewtonStats,
}

impl Default for PairSolver {
    fn default() -> Self {
        Self::ALL[0]
    }
}

impl PairSolver {
    pub const ALL: [Self; 2] = [Self::Newton(NewtonSettings::DEFAULT), Self::WrightOmega];
    pub const NAMES: [&'static str; 2] = [Self::ALL[0].name(), Self::ALL[1].name()];

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Newton(_) => "newton",
            Self::WrightOmega => "wright-omega",
        }
    }
}

impl DiodeClipper {
    pub fn new(config: DiodeClipperConfig) -> Self {
        Self {
            tree: Parallel::new(
                ResistiveSource {
                    ohms: config.input_ohms,
                    volts: 0.0,
                },
                Capacitor::new(config.capacitor_farads),
            ),
            sample_rate: 48_000.0,
            diode: config.diode,
            solver: config.solver,
            volts: 0.0,
            stats: NewtonStats::default(),
        }
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// Component values change in place, state kept.
    pub fn configure(&mut self, config: &DiodeClipperConfig) {
        self.tree.left.ohms = config.input_ohms;
        self.tree.right.set_farads(config.capacitor_farads);
        self.diode = config.diode;
        self.solver = config.solver;
    }

    /// The wave the pair reflects for an arriving wave a through port
    /// resistance R, in closed form (Werner, Nangia, Smith, Abel 2015).
    fn reflect_closed_form(&self, wave: f64, ohms: f64) -> f64 {
        let (is, knee) = (self.diode.saturation_amps, self.diode.knee_volts());
        let r_is = ohms * is;
        let magnitude = wave.abs();
        let w = wright_omega((r_is / knee).ln() + (magnitude + r_is) / knee);
        wave.signum() * (magnitude + 2.0 * r_is - 2.0 * knee * w)
    }

    /// The root's voltage v for an arriving wave a through port
    /// resistance R: v + R·i(v) = a, with SPICE's junction limiting.
    fn solve(&mut self, wave: f64, ohms: f64, settings: &NewtonSettings) -> f64 {
        let diode = self.diode;
        let residual = |v: f64| {
            let (i, g) = diode.pair_current_and_conductance(v);
            (v + ohms * i - wave, 1.0 + ohms * g)
        };
        let solved = settings.solve_scalar(self.volts, residual, |new, old| {
            diode.limit_pair_step(new, old)
        });
        self.stats.record_solve(&solved);
        solved.value
    }
}

impl Stage for DiodeClipper {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.tree.right.prepare(sample_rate);
        self.volts = 0.0;
        self.stats = NewtonStats::default();
    }

    fn reset(&mut self) {
        self.tree.right.reset();
        self.volts = 0.0;
    }

    fn tick(&mut self, x: f32) -> f32 {
        self.tree.left.volts = f64::from(x);
        let wave = self.tree.reflected();
        let ohms = self.tree.resistance();
        let reflected = match self.solver {
            PairSolver::Newton(settings) => {
                self.volts = self.solve(wave, ohms, &settings);
                2.0 * self.volts - wave
            }
            PairSolver::WrightOmega => {
                let b = self.reflect_closed_form(wave, ohms);
                self.volts = 0.5 * (wave + b);
                b
            }
        };
        self.tree.incident(reflected);
        self.volts as f32
    }

    /// With the diodes off, the capacitor discharges through the
    /// input resistance: τ = R·C.
    fn tail(&self) -> Tail {
        let tau = self.tree.left.ohms * self.tree.right.farads();
        Tail::from_time_constant(tau, self.sample_rate)
    }
}
