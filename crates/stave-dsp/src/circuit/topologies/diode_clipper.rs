//! Input resistor into a capacitor to ground, shunted by an antiparallel
//! diode pair; output across the capacitor. A wave-digital tree with
//! the pair at the root, solved per sample by the method `M`.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::circuit::methods::wdf::{Capacitor, OnePort, PairMethod, Parallel, ResistiveSource};
use crate::circuit::methods::wright_omega::WrightOmega;
use crate::kernel::node::{Stage, Tail};

/// The clipper's parts and its method: `M` is the pair's
/// [`PairMethod`] — [`NewtonSettings`] (Newton on the exact pair),
/// [`WrightOmega`] (the closed form), or the run-time choice
/// [`PairSolver`] (the default).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DiodeClipperConfig<M = PairSolver> {
    pub input_ohms: f64,
    pub capacitor_farads: f64,
    pub diode: Shockley,
    pub solver: M,
}

/// The pair's method, chosen at run time (a catalog setting).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PairSolver {
    /// Newton on the exact pair, with SPICE's junction limiting.
    Newton(NewtonSettings),
    /// Werner et al.'s closed form: whichever diode conducts, alone.
    /// No iterations; the off diode's leakage is ignored.
    WrightOmega,
}

/// The R–C–diode-pair clipper, solved by the method `M` (see the module
/// docs).
///
/// # Models
/// The R–C–diode-pair clipper (`stave-test/spice/diode_clipper.cir`):
/// input resistor into a capacitor to ground, shunted by an
/// antiparallel pair ([`Shockley::D1N4148`] in the tests).
///
/// # Derivation
/// A wave-digital tree (bilinear capacitor) with the pair at the root,
/// solved by `M`: Newton with SPICE's junction limiting, or Werner et
/// al.'s Wright-omega closed form.
///
/// # Verified
/// `crates/stave-test/tests/matches_ngspice.rs::diode_clipper_matches_ngspice`:
/// 5e-4 normalized RMS against ngspice at 4×, with either solver, no
/// capped solve;
/// `crates/stave-test/tests/solver_conformance.rs::diode_clipper_converges_at_the_host_rate`:
/// no solve capped at the host rate under a hot signal.
///
/// # Cost
/// Datasheet `"DiodeClipper (WDF, Newton)"`,
/// `"DiodeClipper (WDF, Wright omega)"`, and `"DiodeClipper x10 (WDF)"`
/// driven hard.
///
/// # Aliasing
/// None of its own: a hard clipper; run it inside an oversampled region
/// (the reference holds at 4×).
///
/// # Valid ranges
/// Any positive rate; Newton capped at [`NewtonSettings::DEFAULT`]'s 32
/// iterations, caps reported by `stats()`. Tested to 6 V peak (the hot
/// signal of `solver_conformance`).
#[derive(Clone)]
pub struct DiodeClipper<M = PairSolver> {
    tree: Parallel<ResistiveSource, Capacitor>,
    sample_rate: f32,
    diode: Shockley,
    solver: M,
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

impl PairMethod for PairSolver {
    #[inline]
    fn reflect(
        &self,
        diode: &Shockley,
        wave: f64,
        ohms: f64,
        volts: &mut f64,
        stats: &mut NewtonStats,
    ) -> f64 {
        match self {
            Self::Newton(settings) => settings.reflect(diode, wave, ohms, volts, stats),
            Self::WrightOmega => WrightOmega.reflect(diode, wave, ohms, volts, stats),
        }
    }
}

impl<M: PairMethod> DiodeClipper<M> {
    pub fn new(config: DiodeClipperConfig<M>) -> Self {
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
    pub fn configure(&mut self, config: &DiodeClipperConfig<M>) {
        self.tree.left.ohms = config.input_ohms;
        self.tree.right.set_farads(config.capacitor_farads);
        self.diode = config.diode;
        self.solver = config.solver;
    }
}

impl<M: PairMethod> Stage for DiodeClipper<M> {
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
        let reflected =
            self.solver
                .reflect(&self.diode, wave, ohms, &mut self.volts, &mut self.stats);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run<M: PairMethod>(solver: M) -> Vec<f32> {
        let mut c = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::N4148,
            solver,
        });
        c.prepare(48_000.0);
        (0..480)
            .map(|n| c.tick(3.0 * (n as f32 * 0.05).sin()))
            .collect()
    }

    /// The static methods are exactly the run-time choices they replace.
    #[test]
    fn a_static_method_is_bit_identical_to_its_run_time_choice() {
        assert_eq!(
            run(NewtonSettings::DEFAULT),
            run(PairSolver::Newton(NewtonSettings::DEFAULT))
        );
        assert_eq!(run(WrightOmega), run(PairSolver::WrightOmega));
    }
}
