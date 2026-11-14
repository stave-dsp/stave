//! Non-inverting op-amp stage with an antiparallel diode pair in its
//! feedback loop — the Tube Screamer's clipping stage. Ideal op-amp: the
//! inverting input follows the input, so the ground leg (R in series with
//! C) carries a current set by the input alone, and that current flows
//! through the feedback network (Rf ‖ Cf ‖ diodes). Output = input +
//! feedback voltage, so the stage never drops below the dry signal.
//! Trapezoidal companions for both capacitors; one scalar Newton solve for
//! the feedback voltage per sample, with SPICE's junction limiting.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::kernel::node::{Stage, Tail};
use crate::units::Seconds;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OpAmpClipperConfig {
    /// The ground leg's resistor (sets the gain and, with its capacitor,
    /// the bass cut).
    pub ground_ohms: f64,
    pub ground_farads: f64,
    pub feedback_ohms: f64,
    pub feedback_farads: f64,
    pub diode: Shockley,
    pub newton: NewtonSettings,
}

impl OpAmpClipperConfig {
    /// The TS808 values: 4.7 kΩ + 47 nF to ground, 51 kΩ (+ the drive pot,
    /// the product's to add) ‖ 51 pF ‖ 1N4148 pair in the loop.
    pub const TS808: Self = Self {
        ground_ohms: 4.7e3,
        ground_farads: 47e-9,
        feedback_ohms: 51e3,
        feedback_farads: 51e-12,
        diode: Shockley::N4148,
        newton: NewtonSettings::DEFAULT,
    };
}

/// The op-amp diode clipper (see the module docs).
///
/// # Models
/// The Tube Screamer's clipping stage: a non-inverting ideal op-amp
/// with an antiparallel diode pair in its feedback loop
/// ([`OpAmpClipperConfig::TS808`]).
///
/// # Derivation
/// Ideal op-amp nodal analysis, trapezoidal companions for both
/// capacitors, one scalar Newton solve per sample for the feedback
/// voltage with SPICE's junction limiting.
///
/// # Verified
/// `crates/stave-test/tests/matches_ngspice.rs::opamp_clipper_matches_ngspice`:
/// 5e-5 normalized RMS at 16×, the error falling as h² (3× or more at
/// 32×), no capped solve;
/// `crates/stave-test/tests/solver_conformance.rs::opamp_clipper_converges_at_the_host_rate`.
///
/// # Cost
/// Datasheet `"OpAmpClipper"`.
///
/// # Aliasing
/// None of its own; its 2.6 µs feedback pole wants a high rate: the
/// reference holds at 16×.
///
/// # Valid ranges
/// Any positive rate (debug-asserted); an ideal op-amp, so no rail
/// clipping; Newton capped by its [`NewtonSettings`].
#[derive(Clone)]
pub struct OpAmpClipper {
    config: OpAmpClipperConfig,
    step: f64,
    sample_rate: f32,
    /// Ground-leg capacitor voltage and the leg's current, last sample.
    ground_volts: f64,
    ground_amps: f64,
    /// Feedback voltage (output − input) and the feedback capacitor's
    /// current, last sample.
    feedback_volts: f64,
    feedback_cap_amps: f64,
    stats: NewtonStats,
}

impl OpAmpClipper {
    pub fn new(config: OpAmpClipperConfig) -> Self {
        Self {
            config,
            step: 1.0 / 48_000.0,
            sample_rate: 48_000.0,
            ground_volts: 0.0,
            ground_amps: 0.0,
            feedback_volts: 0.0,
            feedback_cap_amps: 0.0,
            stats: NewtonStats::default(),
        }
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// Component values change in place, state kept (a drive pot turned
    /// mid-note).
    pub fn configure(&mut self, config: &OpAmpClipperConfig) {
        self.config = *config;
    }

    /// One sample in `f64` volts.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let c = self.config;
        let t = self.step;
        // Ground leg: i = (vin − vc)/R, C·dvc/dt = i, trapezoidal.
        let k = t / (2.0 * c.ground_farads);
        self.ground_volts = (self.ground_volts + k * (input / c.ground_ohms + self.ground_amps))
            / (1.0 + k / c.ground_ohms);
        self.ground_amps = (input - self.ground_volts) / c.ground_ohms;
        // Feedback: v/Rf + Cf·dv/dt + Id(v) = i_ground.
        let g_cap = 2.0 * c.feedback_farads / t;
        let history = g_cap * self.feedback_volts + self.feedback_cap_amps;
        let target = self.ground_amps + history;
        let diode = c.diode;
        let g_linear = 1.0 / c.feedback_ohms + g_cap;
        let residual = |v: f64| {
            let (i, g) = diode.pair_current_and_conductance(v);
            (g_linear * v + i - target, g_linear + g)
        };
        let solved = c
            .newton
            .solve_scalar(self.feedback_volts, residual, |new, old| {
                diode.limit_pair_step(new, old)
            });
        self.stats.record_solve(&solved);
        self.feedback_cap_amps = g_cap * solved.value - history;
        self.feedback_volts = solved.value;
        input + solved.value
    }
}

impl Stage for OpAmpClipper {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.step = 1.0 / f64::from(sample_rate);
        self.reset();
        self.stats = NewtonStats::default();
    }

    fn reset(&mut self) {
        self.ground_volts = 0.0;
        self.ground_amps = 0.0;
        self.feedback_volts = 0.0;
        self.feedback_cap_amps = 0.0;
    }

    fn tick(&mut self, x: f32) -> f32 {
        crate::kernel::denormal::normal64(self.tick_f64(f64::from(x)))
    }

    /// The slower of the two RC legs.
    fn tail(&self) -> Tail {
        let c = &self.config;
        let tau = (c.ground_ohms * c.ground_farads).max(c.feedback_ohms * c.feedback_farads);
        Tail::from_time_constant(Seconds::new(tau), self.sample_rate)
    }
}
