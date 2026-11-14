//! A triode with its plate fed from the supply through a load resistor
//! and its cathode to ground through a resistor bypassed by a capacitor;
//! the grid driven through the source's resistance (output impedance
//! plus grid stopper), output at the plate. Grid, plate and cathode are
//! solved together each sample (3-D Newton), so a model with grid
//! current loads the source when the grid goes positive; the bypass
//! capacitor is SPICE's trapezoidal companion model; the stage starts at
//! its DC operating point, as a SPICE transient does.

use crate::circuit::audio::Quiescent;
use crate::circuit::devices::triode::Triode;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};
use crate::kernel::node::{Stage, Tail};
use crate::units::Seconds;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CommonCathodeConfig<T> {
    pub tube: T,
    /// Between the input and the grid; 0 is an ideal source.
    pub source_ohms: f64,
    pub supply_volts: f64,
    pub plate_load_ohms: f64,
    pub cathode_ohms: f64,
    pub bypass_farads: f64,
    pub newton: NewtonSettings,
}

/// A triode common-cathode stage, solved per sample (see the module docs).
///
/// # Models
/// A triode common-cathode stage: plate load to the supply, bypassed
/// cathode resistor, grid driven through the source resistance; any
/// [`Triode`] model (`Koren::T12AX7`, `Dempwolf::RSD_1` …).
///
/// # Derivation
/// Grid, plate and cathode solved together per sample (3-D Newton with
/// junction limiting), the bypass capacitor as SPICE's trapezoidal
/// companion, started at its DC operating point.
///
/// # Verified
/// `crates/stave-test/tests/matches_ngspice.rs::common_cathode_matches_ngspice`:
/// Koren 12AX7, 1e-5 normalized RMS at 4× about an operating point that
/// agrees to 1 µV;
/// `crates/stave-test/tests/matches_ngspice.rs::common_cathode_grid_current_matches_ngspice`:
/// Dempwolf with grid current through 33 kΩ, 2e-5;
/// `crates/stave-test/tests/solver_conformance.rs::common_cathode_converges_at_the_host_rate`.
///
/// # Cost
/// Datasheet `"CommonCathode x5 (Koren)"`.
///
/// # Aliasing
/// None of its own; the references hold at 4×: run it oversampled.
///
/// # Valid ranges
/// Any positive rate; parts as a real stage takes them (tested at 300
/// V, 100 kΩ, 1.5 kΩ, 22 µF); grid drive to 3 V peak tested; Newton
/// capped by its [`NewtonSettings`].
#[derive(Clone)]
pub struct CommonCathode<T> {
    config: CommonCathodeConfig<T>,
    sample_rate: f32,
    /// Trapezoidal companion conductance of the bypass cap, 2C/T.
    companion_siemens: f64,
    nodes: NodeVoltages,
    cap_amps: f64,
    stats: NewtonStats,
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct NodeVoltages {
    grid: f64,
    plate: f64,
    cathode: f64,
}

impl NodeVoltages {
    /// No current flows: the plate sits at the supply, grid and cathode
    /// at ground. A physical state every operating point is reachable from.
    fn cutoff<T>(c: &CommonCathodeConfig<T>) -> Self {
        Self {
            grid: 0.0,
            plate: c.supply_volts,
            cathode: 0.0,
        }
    }
}

impl<T: Triode> CommonCathode<T> {
    pub fn new(config: CommonCathodeConfig<T>) -> Self {
        let mut stage = Self {
            config,
            sample_rate: 48_000.0,
            companion_siemens: 2.0 * config.bypass_farads * 48_000.0,
            nodes: NodeVoltages::cutoff(&config),
            cap_amps: 0.0,
            stats: NewtonStats::default(),
        };
        stage.settle_at_operating_point();
        stage
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    pub fn grid_volts(&self) -> f64 {
        self.nodes.grid
    }

    pub fn plate_volts(&self) -> f64 {
        self.nodes.plate
    }

    pub fn cathode_volts(&self) -> f64 {
        self.nodes.cathode
    }

    /// One sample in f64: input volts in, plate volts out, at the
    /// solve's own precision.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let cap = Companion {
            siemens: self.companion_siemens,
            previous_cathode: self.nodes.cathode,
            previous_amps: self.cap_amps,
        };
        let solution = solve(&self.config, &cap, self.nodes, input);
        self.stats.record(
            solution.iterations,
            solution.final_step < self.config.newton.tolerance,
            solution.final_step,
        );
        let solved = solution.nodes;
        self.cap_amps =
            self.companion_siemens * (solved.cathode - self.nodes.cathode) - self.cap_amps;
        self.nodes = solved;
        solved.plate
    }

    /// Component values change in place; the circuit moves toward its
    /// new operating point through its own dynamics.
    pub fn configure(&mut self, config: &CommonCathodeConfig<T>) {
        self.config = *config;
        self.companion_siemens = 2.0 * config.bypass_farads * f64::from(self.sample_rate);
    }

    /// The DC operating point for the current component values (input
    /// at 0 V, bypass capacitor open), without disturbing the running state.
    fn operating_point(&self) -> NodeVoltages {
        let open = Companion {
            siemens: 0.0,
            previous_cathode: 0.0,
            previous_amps: 0.0,
        };
        solve(&self.config, &open, NodeVoltages::cutoff(&self.config), 0.0).nodes
    }

    pub fn quiescent_plate_volts(&self) -> f64 {
        self.operating_point().plate
    }

    fn settle_at_operating_point(&mut self) {
        self.nodes = self.operating_point();
        self.cap_amps = 0.0;
    }
}

/// The bypass capacitor's trapezoidal companion: conductance 2C/T and
/// the previous step's voltage and current.
#[derive(Clone)]
struct Companion {
    siemens: f64,
    previous_cathode: f64,
    previous_amps: f64,
}

#[derive(Clone)]
struct Solution {
    nodes: NodeVoltages,
    iterations: u32,
    final_step: f64,
}

/// Kirchhoff at grid, plate and cathode for input voltage `input`, the
/// grid equation multiplied through by the source resistance Rs so an
/// ideal source (Rs = 0) is the same equation:
///
/// ```text
///     (Vin − Vg) − Rs·Ig = 0
///     (Vb − Vp)/Ra − Ip = 0
///     Ip + Ig − Vk/Rk − (Gc·(Vk − Vk₋₁) − Ic₋₁) = 0
/// ```
#[derive(Clone)]
struct Residual {
    values: [f64; 3],
    jacobian: [[f64; 3]; 3],
}

fn residual<T: Triode>(
    c: &CommonCathodeConfig<T>,
    cap: &Companion,
    v: &NodeVoltages,
    input: f64,
) -> Residual {
    let i = c.tube.currents(v.grid - v.cathode, v.plate - v.cathode);
    let (gm, gp, gg) = (
        i.plate_per_grid_volt,
        i.plate_per_plate_volt,
        i.grid_per_grid_volt,
    );
    let rs = c.source_ohms;
    let capacitor = cap.siemens * (v.cathode - cap.previous_cathode) - cap.previous_amps;
    Residual {
        values: [
            input - v.grid - rs * i.grid,
            (c.supply_volts - v.plate) / c.plate_load_ohms - i.plate,
            i.plate + i.grid - v.cathode / c.cathode_ohms - capacitor,
        ],
        // Columns: grid, plate, cathode.
        jacobian: [
            [-1.0 - rs * gg, 0.0, rs * gg],
            [-gm, -1.0 / c.plate_load_ohms - gp, gm + gp],
            [
                gm + gg,
                gp,
                -gm - gp - gg - 1.0 / c.cathode_ohms - cap.siemens,
            ],
        ],
    }
}

impl Residual {
    fn norm(&self) -> f64 {
        self.values.iter().map(|r| r * r).sum::<f64>().sqrt()
    }

    /// The Newton step J⁻¹·r, by Cramer's rule (3×3).
    fn step(&self) -> [f64; 3] {
        let det3 = |m: &[[f64; 3]; 3]| {
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        };
        let det = det3(&self.jacobian);
        core::array::from_fn(|col| {
            let mut m = self.jacobian;
            for (row, r) in m.iter_mut().zip(self.values) {
                row[col] = r;
            }
            det3(&m) / det
        })
    }
}

/// Damped Newton: a full step is halved until it reduces the residual
/// (backtracking), so a grid driven far past cutoff or into heavy
/// conduction cannot overshoot. Iterates are never clamped: the answer
/// is wherever the circuit's equations put it.
fn solve<T: Triode>(
    c: &CommonCathodeConfig<T>,
    cap: &Companion,
    guess: NodeVoltages,
    input: f64,
) -> Solution {
    let mut v = guess;
    let mut r = residual(c, cap, &v, input);
    let (mut iterations, mut worst_step) = (0, f64::INFINITY);
    let newton = &c.newton;
    while iterations < newton.max_iterations && worst_step >= newton.tolerance {
        let [dg, dp, dk] = r.step();
        let mut scale = 1.0;
        let (next, next_r) = loop {
            let n = NodeVoltages {
                grid: v.grid - scale * dg,
                plate: v.plate - scale * dp,
                cathode: v.cathode - scale * dk,
            };
            let nr = residual(c, cap, &n, input);
            if nr.norm() < r.norm() || scale <= newton.min_damping {
                break (n, nr);
            }
            scale *= 0.5;
        };
        worst_step = (next.grid - v.grid)
            .abs()
            .max((next.plate - v.plate).abs())
            .max((next.cathode - v.cathode).abs());
        (v, r) = (next, next_r);
        iterations += 1;
    }
    Solution {
        nodes: v,
        iterations,
        final_step: worst_step,
    }
}

impl<T: Triode> Quiescent for CommonCathode<T> {
    fn quiescent_output_volts(&self) -> f64 {
        self.quiescent_plate_volts()
    }
}

impl<T: Triode> Stage for CommonCathode<T> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.companion_siemens = 2.0 * self.config.bypass_farads * f64::from(sample_rate);
        self.settle_at_operating_point();
        self.stats = NewtonStats::default();
    }

    fn reset(&mut self) {
        self.settle_at_operating_point();
    }

    /// Input volts in, plate volts out ([`tick_f64`](CommonCathode::tick_f64)
    /// rounded: a plate near 200 V in f32 resolves 15 µV, about −100 dB
    /// under a small signal; read `tick_f64` where that matters).
    fn tick(&mut self, input: f32) -> f32 {
        self.tick_f64(f64::from(input)) as f32
    }

    /// The bypass capacitor recovers through the cathode resistor:
    /// τ ≤ Rk·Ck (feedback through gm only shortens it).
    fn tail(&self) -> Tail {
        let tau = self.config.cathode_ohms * self.config.bypass_farads;
        Tail::from_time_constant(Seconds::new(tau), self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::devices::triode::koren::Koren;

    fn stage() -> CommonCathode<Koren> {
        CommonCathode::new(CommonCathodeConfig {
            tube: Koren::T12AX7,
            source_ohms: 0.0,
            supply_volts: 300.0,
            plate_load_ohms: 100e3,
            cathode_ohms: 1.5e3,
            bypass_farads: 22e-6,
            newton: NewtonSettings::default(),
        })
    }

    #[test]
    fn the_operating_point_balances_both_nodes() {
        let s = stage();
        let ip = Koren::T12AX7
            .plate(-s.cathode_volts(), s.plate_volts() - s.cathode_volts())
            .amps;
        assert!(((300.0 - s.plate_volts()) / 100e3 - ip).abs() < 1e-12);
        assert!((s.cathode_volts() / 1.5e3 - ip).abs() < 1e-12);
        assert!(
            (0.5..3.0).contains(&s.cathode_volts()),
            "{}",
            s.cathode_volts()
        );
    }

    #[test]
    fn silence_holds_the_operating_point() {
        let mut s = stage();
        s.prepare(48_000.0);
        let quiescent = s.plate_volts();
        for _ in 0..4_800 {
            s.tick(0.0);
        }
        assert!((s.plate_volts() - quiescent).abs() < 1e-9);
        assert_eq!(s.stats().capped, 0);
    }

    /// The old `Triode` core (fixed −1.5 V bias on a Koren load line)
    /// against this stage set to the same operating point: cathode at
    /// 1.5 V, bypass large enough to be a short, supply raised by the
    /// cathode's 1.5 V, both driven by an ideal source (the Koren law has
    /// no grid current to load one). Plate swing (quiescent minus plate)
    /// for a grid sine.
    fn swings(amplitude: f64) -> (Vec<f64>, Vec<f64>) {
        use crate::circuit::topologies::triode_stage::{
            core_transfer_at, solve_load_line, TriodeParams,
        };
        let p = TriodeParams {
            source_ohms: 0.0,
            ..TriodeParams::default()
        };
        let vpk0 = solve_load_line(&p, p.grid_bias, 0.7 * p.supply_v);
        let rest_amps = (p.supply_v - vpk0) / p.r_plate;
        let mut stage = CommonCathode::new(CommonCathodeConfig {
            tube: Koren::T12AX7,
            source_ohms: 0.0,
            supply_volts: p.supply_v - p.grid_bias,
            plate_load_ohms: p.r_plate,
            cathode_ohms: -p.grid_bias / rest_amps,
            bypass_farads: 10.0,
            newton: NewtonSettings::default(),
        });
        stage.prepare(48_000.0);
        let quiescent = stage.plate_volts();
        let (mut old, mut new) = (vec![], vec![]);
        for n in 0..4_800 {
            let grid = amplitude * (core::f64::consts::TAU * 100.0 * n as f64 / 48_000.0).sin();
            old.push(core_transfer_at(&p, vpk0, p.grid_bias + grid));
            new.push(quiescent - f64::from(stage.tick(grid as f32)));
        }
        (old, new)
    }

    fn worst_difference(amplitude: f64) -> f64 {
        let (old, new) = swings(amplitude);
        old.iter()
            .zip(&new)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    }

    /// Same circuit, same answer, whether or not the grid swings
    /// positive: the old core's hand-tuned grid clamp (46 V of error at
    /// 3 V) is gone, replaced by grid current through the source.
    #[test]
    fn the_triode_core_is_this_stage() {
        assert!(worst_difference(1.0) < 1e-4);
        assert!(worst_difference(3.0) < 1e-3);
    }

    /// Highest grid voltage over two cycles of a 3 V, 1 kHz input
    /// through 33 kΩ.
    fn grid_peak<T: Triode>(tube: T) -> f64 {
        let mut s = CommonCathode::new(CommonCathodeConfig {
            tube,
            source_ohms: 33e3,
            supply_volts: 300.0,
            plate_load_ohms: 100e3,
            cathode_ohms: 1.5e3,
            bypass_farads: 22e-6,
            newton: NewtonSettings::default(),
        });
        s.prepare(48_000.0);
        (0..96)
            .map(|n| {
                s.tick((3.0 * (core::f64::consts::TAU * n as f64 / 48.0).sin()) as f32);
                s.grid_volts()
            })
            .fold(f64::MIN, f64::max)
    }

    /// Grid current loads the source: past 0 V the grid conducts, so the
    /// grid stops short of the input's peak; a model without grid
    /// current lets the grid follow the input exactly.
    #[test]
    fn grid_current_holds_the_grid_near_the_cathode() {
        use crate::circuit::devices::triode::dempwolf::Dempwolf;
        let without = grid_peak(Koren::T12AX7);
        let with = grid_peak(Dempwolf::RSD_1);
        assert!((without - 3.0).abs() < 0.01, "{without}");
        assert!(with < 1.7, "{with}");
    }
}
