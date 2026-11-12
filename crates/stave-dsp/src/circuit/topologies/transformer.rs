//! A transformer on a Jiles–Atherton core, driven by a voltage through a
//! source resistance, secondary unloaded. The primary's flux linkage obeys
//! N·A·dB/dt = v − Rs·i with H = N·i/le and B = μ0·(H + M(H)); the
//! secondary is the turns ratio times the primary EMF. Trapezoidal in
//! time (flux linkage = the integral of the EMF), one Newton solve for the
//! primary current per sample with the core's own solve inside it.

use crate::circuit::devices::magnetic::{JilesAtherton, JilesAthertonCore, MU_0};
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::kernel::node::Stage;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TransformerConfig {
    pub core: JilesAtherton,
    pub primary_turns: f64,
    /// Core cross-section, m².
    pub core_area: f64,
    /// Mean magnetic path length, m.
    pub path_length: f64,
    /// Secondary turns over primary turns.
    pub turns_ratio: f64,
    pub source_ohms: f64,
    pub newton: NewtonSettings,
}

pub struct Transformer {
    config: TransformerConfig,
    core: JilesAthertonCore,
    step: f64,
    current: f64,
    emf: f64,
    flux_density: f64,
    stats: NewtonStats,
}

impl Transformer {
    pub fn new(config: TransformerConfig) -> Self {
        Self {
            config,
            core: JilesAthertonCore::new(config.core, config.newton),
            step: 0.0,
            current: 0.0,
            emf: 0.0,
            flux_density: 0.0,
            stats: NewtonStats::default(),
        }
    }

    /// The primary-current solve's statistics.
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// Component values change in place, state kept.
    pub fn configure(&mut self, config: &TransformerConfig) {
        self.config = *config;
        self.core.set_model(config.core);
    }

    /// The core's flux density, tesla.
    pub fn flux_density(&self) -> f64 {
        self.flux_density
    }

    /// Drive the primary with `volts`; returns the secondary's volts.
    pub fn process_volts(&mut self, volts: f64) -> f64 {
        let c = self.config;
        let field_per_amp = c.primary_turns / c.path_length;
        let linkage = c.primary_turns * c.core_area;
        let half_step = 0.5 * self.step;
        let (b0, e0) = (self.flux_density, self.emf);
        let core = &self.core;
        let residual = |amps: f64| {
            let h = field_per_amp * amps;
            let (m, chi) = core.trial(h);
            let b = MU_0 * (h + m);
            (
                linkage * (b - b0) - half_step * (volts - c.source_ohms * amps + e0),
                linkage * MU_0 * field_per_amp * (1.0 + chi) + half_step * c.source_ohms,
            )
        };
        let solved = c.newton.solve_scalar(self.current, residual, |new, _| new);
        self.stats.record_solve(&solved);
        self.current = solved.value;
        self.core.step(field_per_amp * self.current);
        self.flux_density = self.core.flux_density();
        self.emf = volts - c.source_ohms * self.current;
        c.turns_ratio * self.emf
    }
}

impl Stage for Transformer {
    fn prepare(&mut self, rate: f32) {
        debug_assert!(rate > 0.0, "sample rate must be positive");
        self.step = 1.0 / f64::from(rate);
        self.reset();
        self.stats = NewtonStats::default();
    }

    fn reset(&mut self) {
        self.core.reset();
        self.current = 0.0;
        self.emf = 0.0;
        self.flux_density = 0.0;
    }

    fn tick(&mut self, x: f32) -> f32 {
        self.process_volts(f64::from(x)) as f32
    }
}
