//! Input resistor into a capacitor to ground, shunted by an antiparallel
//! diode pair; output across the capacitor. A wave-digital tree with
//! the pair at the root, solved per sample by the method `M`.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::circuit::methods::wdf::adaa::{Antiparallel, Root};
use crate::circuit::methods::wdf::{Capacitor, OnePort, PairMethod, Parallel, ResistiveSource};
use crate::circuit::methods::wright_omega::WrightOmega;
use crate::kernel::node::{Stage, Tail};
use crate::units::Seconds;

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
/// no solve capped at the host rate under a hot signal;
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`
/// (DSP.md R5): ×10 into the Newton pair inside a 4× region, a
/// stepped tone's inharmonic energy below 20 kHz under −41 dB at
/// 44.1 kHz (measured −43.3) and −36 at 48 (−38.0). With ADAA:
/// `crates/stave-test/tests/adaa_matches_ngspice.rs::wdf_diode_clipper_with_adaa_matches_ngspice`
/// (6.9e-4 at 4×, either solver) and
/// `crates/stave-dsp/tests/aliasing_budgets.rs::wdf_diode_clipper_adaa_44k_holds_its_alias_budgets`
/// (at 44.1 kHz −28.4 dB at 1×, −39.3 at 2×, −64.6 at 4×, against
/// −9.9, −24.4 and −43.3 without).
///
/// # Cost
/// Datasheet `"DiodeClipper (WDF, Newton)"`,
/// `"DiodeClipper (WDF, Wright omega)"`, and `"DiodeClipper x10 (WDF)"`
/// driven hard; with ADAA `"DiodeClipper x10 (WDF) 1x ADAA"` and its
/// 2× and 4× lines, beside `"DiodeClipper x10 (WDF) 8x"`.
///
/// # Aliasing
/// None of its own by default: a hard clipper; run it inside an
/// oversampled region (the reference holds at 4×). With
/// [`set_antialiasing`](Self::set_antialiasing)`(Antialiasing::Adaa)`,
/// first-order ADAA at the root (Albertini et al. 2021: the pair's mean
/// reflection in closed form, the leaves' waves averaged, the capacitor
/// designed for `1.5·T`; [`methods::wdf::adaa`](crate::circuit::methods::wdf::adaa)),
/// level at 4× with plain 8× oversampling. The output is half a sample
/// late.
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
    antialiasing: Antialiasing,
    root: Root,
    capacitor_prev: f64,
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
    #[inline(always)]
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
            antialiasing: Antialiasing::None,
            root: Root::default(),
            capacitor_prev: 0.0,
        }
    }

    /// First-order ADAA at the root ([`Antialiasing::Adaa`]) or none
    /// (the default); re-prepares at the current rate. Under ADAA the
    /// pair's mean is taken on the exact antiparallel law whichever
    /// method solves it, so [`WrightOmega`]'s neglected off-diode
    /// leakage (at most `Is·R`, microvolts) enters the mean.
    pub fn set_antialiasing(&mut self, antialiasing: Antialiasing) {
        self.antialiasing = antialiasing;
        self.prepare(self.sample_rate);
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
        // Under ADAA the capacitor is designed for 1.5·T.
        let design_rate = f64::from(sample_rate) / self.antialiasing.design_step_scale();
        self.tree.right.prepare(design_rate as f32);
        self.stats = NewtonStats::default();
        self.reset();
    }

    fn reset(&mut self) {
        self.tree.right.reset();
        self.volts = 0.0;
        self.root.reset();
        self.capacitor_prev = 0.0;
    }

    fn tick(&mut self, x: f32) -> f32 {
        if self.antialiasing == Antialiasing::Adaa {
            return crate::kernel::denormal::normal64(self.tick_adaa(f64::from(x)));
        }
        crate::kernel::denormal::normal(self.tick_plain(x))
    }

    /// The mode is chosen once a block, so the plain path's loop is the
    /// plain tick alone.
    fn process(&mut self, block: &mut [f32]) {
        if self.antialiasing == Antialiasing::Adaa {
            for x in block.iter_mut() {
                *x = crate::kernel::denormal::normal64(self.tick_adaa(f64::from(*x)));
            }
        } else {
            for x in block.iter_mut() {
                *x = crate::kernel::denormal::normal(self.tick_plain(*x));
            }
        }
    }

    /// With the diodes off, the capacitor discharges through the
    /// input resistance: τ = R·C.
    fn tail(&self) -> Tail {
        let tau = self.tree.left.ohms * self.tree.right.farads();
        Tail::from_time_constant(Seconds::new(tau), self.sample_rate)
    }
}

impl<M: PairMethod> DiodeClipper<M> {
    /// One sample without ADAA: the tree up, the pair, the tree down.
    #[inline(always)]
    fn tick_plain(&mut self, x: f32) -> f32 {
        self.tree.left.volts = f64::from(x);
        let wave = self.tree.reflected();
        let ohms = self.tree.resistance();
        let reflected =
            self.solver
                .reflect(&self.diode, wave, ohms, &mut self.volts, &mut self.stats);
        self.tree.incident(reflected);
        self.volts as f32
    }

    /// One sample under ADAA (Albertini et al. 2021, Algorithm 1): the
    /// forward scan as usual, the pair's mean reflection, then the
    /// backward scan on the leaves' waves averaged with their last values.
    #[inline(never)]
    fn tick_adaa(&mut self, x: f64) -> f64 {
        let capacitor = self.tree.right.reflected();
        let (g_source, g_capacitor) = (1.0 / self.tree.left.ohms, 1.0 / self.tree.right.resistance());
        let share = g_source / (g_source + g_capacitor);
        let wave = share * x + (1.0 - share) * capacitor;
        let ohms = self.tree.resistance();
        self.solver
            .reflect(&self.diode, wave, ohms, &mut self.volts, &mut self.stats);
        let mean = self
            .root
            .reflect(&Antiparallel(self.diode), wave, self.volts, ohms);
        // Parallel adaptor, backward: each child gets b + up − its own wave,
        // every term at n − ½.
        let capacitor_mean = 0.5 * (capacitor + self.capacitor_prev);
        self.tree
            .right
            .incident(mean.reflected + mean.arriving - capacitor_mean);
        self.capacitor_prev = capacitor;
        mean.volts
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
