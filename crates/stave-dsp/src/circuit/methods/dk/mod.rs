//! The nodal DK method (Yeh 2009; Holters & Zölzer 2015, as in ACME.jl):
//! any netlist modified nodal analysis can solve, discretised once per
//! prepare into a state space whose nonlinear subsystem is separated
//! out, so each sample's Newton solve runs on the devices' controlling
//! voltages alone.
//!
//! # Derivation
//!
//! The netlist's MNA unknowns `w` (node voltages, then the branch
//! currents of voltage sources and inductors) satisfy, with every
//! capacitor and inductor replaced by its trapezoidal companion,
//!
//! ```text
//!     S·w = Bx·x + Bu·u + Bq·q          (one sample)
//! ```
//!
//! where `x` are the companions' history states, `u` the independent
//! sources and `q` the nonlinear devices' outputs (junction and plate
//! currents, behavioural sources' values). The devices read controlling
//! voltages `v = Nv·w`. With `S⁻¹` computed at prepare:
//!
//! ```text
//!     v  = Dx·x + Du·u + K·q(v)          p = Dx·x + Du·u: the linear prediction
//!     x' = Ax·x + Au·u + Aq·q            the companions' update
//!     y  = Ex·x + Eu·u + Eq·q            the probe
//! ```
//!
//! Only `v = p + K·q(v)` is nonlinear: its size is the number of
//! distinct controlling voltages (one for a diode pair, two for a
//! triode), not the circuit's. [`solve`] documents the solve.
//!
//! The trapezoidal rule is the bilinear transform, so for the linear
//! part the discrete response at `ω` is the analog response at
//! `(2/T)·tan(ωT/2)`: [`Dk::analog_response`] is exact for a linear
//! circuit, which is how the tone stack is held to ngspice's AC analysis.
//!
//! # Accepts
//!
//! Any netlist of R, C, L, independent V and I sources (DC, SIN, PWL, or
//! a behavioural source of time alone), E and F controlled sources,
//! Shockley diodes (series resistance as a resistor), stave triodes,
//! and behavioural sources of node voltages (`B … I = {…}` or
//! `V = {…}`, at most four node voltages each), provided its MNA matrix
//! is nonsingular (no floating node, no loop of voltage sources). The
//! operating point is solved with capacitors open and inductors shorted,
//! by Newton and then source stepping; a deck with `.tran … UIC` starts
//! from its `IC=` values instead.
//!
//! # Verified
//!
//! `stave-test/tests/dk_matches_ngspice.rs`: every ngspice deck whose
//! devices exist runs through `Circuit<Dk>` and meets the budget of the
//! hand-derived topology on the same deck, and agrees with that
//! topology.
//!
//! # Cost
//!
//! `stave-dsp/tests/datasheet.rs` lists `Circuit<Dk>` lines beside the
//! hand-derived topologies'.

pub mod devices;
pub mod matrix;
pub mod solve;

use devices::Nonlinear;
pub use matrix::Phasor;
use matrix::accumulate;
pub use solve::{Health, Settings};
use solve::{Prediction, Workspace};

use crate::circuit::methods::newton::NewtonStats;
use crate::circuit::netlist::expr::{Dual, Tape};
use crate::circuit::netlist::{Netlist, NetlistError, Node, Probe, Source};
use crate::kernel::node::Tail;

/// An index into the MNA unknowns, or ground.
type Pin = Option<usize>;

/// An independent source's value over time.
#[derive(Clone, Debug)]
enum Input {
    Source(Source),
    Expression { tape: Tape, stack: Vec<Dual> },
}

#[derive(Clone, Debug)]
struct Capacitor {
    a: Pin,
    b: Pin,
    farads: f64,
    initial_volts: f64,
}

#[derive(Clone, Debug)]
struct Inductor {
    row: usize,
    henries: f64,
    initial_amps: f64,
}

/// A netlist discretised by the nodal DK method: the runtime behind
/// `Circuit<Dk>`.
///
/// **Models** the netlist it was built from, nothing else.
/// **Derivation** MNA, trapezoidal companions, the nonlinear subsystem
/// separated (module docs). **Verified** `dk_matches_ngspice.rs`.
/// **Cost** the datasheet's `Circuit<Dk>` lines. **Aliasing** none of its
/// own: a nonlinear netlist aliases as its devices do; run it
/// oversampled. **Valid ranges** wherever its devices' models are.
#[derive(Clone, Debug)]
pub struct Dk {
    // Structure, fixed at build.
    n: usize,
    /// Node unknowns (netlist nodes and internal ones) come first.
    nodes: usize,
    static_matrix: Vec<f64>,
    bu: Vec<f64>,
    bq: Vec<f64>,
    controls: Vec<(Pin, Pin)>,
    capacitors: Vec<Capacitor>,
    inductors: Vec<Inductor>,
    inputs: Vec<Input>,
    input_ac: Vec<f64>,
    devices: Vec<Nonlinear>,
    nq: usize,
    probe: (Pin, Pin),
    driven: Option<usize>,
    uic: bool,
    source_names: Vec<String>,
    // The operating point, fixed at build.
    dc_w: Vec<f64>,
    dc_v: Vec<f64>,
    // Discretised at prepare.
    sample_rate: f32,
    step: f64,
    dx: Vec<f64>,
    du: Vec<f64>,
    k: Vec<f64>,
    ax: Vec<f64>,
    au: Vec<f64>,
    aq: Vec<f64>,
    ex: Vec<f64>,
    eu: Vec<f64>,
    eq: Vec<f64>,
    tail: Tail,
    // Running state.
    x: Vec<f64>,
    x_next: Vec<f64>,
    u: Vec<f64>,
    p: Vec<f64>,
    work: Workspace,
    samples: u64,
    output: f64,
    settings: Settings,
    health: Health,
}

/// The conductance a DC solve adds from every node to ground when the
/// circuit has a node only capacitors reach (SPICE's `gmin`).
const GMIN: f64 = 1e-12;

impl Dk {
    /// Build from `netlist`, probing `netlist.probe`; the operating point
    /// is solved here, so a circuit with none is an error, not a silent
    /// start from zero. Prepared at 48 kHz.
    pub fn new(netlist: &Netlist) -> Result<Self, NetlistError> {
        Self::with_settings(netlist, Settings::DEFAULT)
    }

    pub fn with_settings(netlist: &Netlist, settings: Settings) -> Result<Self, NetlistError> {
        let probe = netlist.probe.ok_or(NetlistError::NoProbe)?;
        let mut dk = Builder::new(netlist).build(probe, settings)?;
        dk.solve_operating_point()?;
        dk.discretise()?;
        dk.reset();
        Ok(dk)
    }

    /// Drive the named independent source from [`step`](Self::step)'s
    /// input (its own signal no longer applies). Undriven, the netlist's
    /// own stimulus runs on the circuit's clock and the input is ignored.
    pub fn drive(&mut self, source: &str) -> Result<(), NetlistError> {
        let name = source.to_ascii_lowercase();
        let index = self
            .source_names
            .iter()
            .position(|n| *n == name)
            .ok_or(NetlistError::UnknownElement { name })?;
        self.driven = Some(index);
        // The driven source rests at 0 (silence): the operating point moves.
        self.solve_operating_point()?;
        self.reset();
        Ok(())
    }

    /// Read a different voltage (re-prepares at the current rate).
    pub fn set_probe(&mut self, probe: Probe) {
        self.probe = (pin(probe.plus), pin(probe.minus));
        self.prepare(self.sample_rate);
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// Newton work, cap hits and continuation fallbacks since prepare.
    pub fn health(&self) -> Health {
        self.health
    }

    /// [`Health::newton`]: the counts every topology reports.
    pub fn stats(&self) -> NewtonStats {
        self.health.newton
    }

    /// The probe's voltage after the last sample (the operating point's
    /// after prepare; 0 under `UIC`).
    pub fn output(&self) -> f64 {
        self.output
    }

    /// `(MNA unknowns, states, controlling voltages, device outputs)`:
    /// the nonlinear subsystem's size is the third.
    pub fn dimensions(&self) -> (usize, usize, usize, usize) {
        (self.n, self.x.len(), self.controls.len(), self.nq)
    }

    /// Discretise for `sample_rate` and return to the operating point.
    pub fn prepare(&mut self, sample_rate: f32) {
        let previous = (self.sample_rate, self.step);
        self.sample_rate = sample_rate;
        self.step = 1.0 / f64::from(sample_rate);
        if self.discretise().is_err() {
            // The build discretised this circuit, so only a pathological rate
            // gets here: keep the last good rate rather than a broken one.
            debug_assert!(false, "circuit singular at {sample_rate} Hz");
            (self.sample_rate, self.step) = previous;
            let _ = self.discretise();
        }
        self.reset();
        self.health = Health::default();
    }

    /// Back to t = 0 at the operating point (or the initial conditions).
    pub fn reset(&mut self) {
        let t = self.step;
        self.samples = 0;
        for (s, c) in self.capacitors.iter().enumerate() {
            let g = 2.0 * c.farads / t;
            self.x[s] = if self.uic {
                g * c.initial_volts
            } else {
                g * (read(&self.dc_w, c.a) - read(&self.dc_w, c.b))
            };
        }
        let offset = self.capacitors.len();
        for (s, l) in self.inductors.iter().enumerate() {
            let r = 2.0 * l.henries / t;
            self.x[offset + s] = r * if self.uic { l.initial_amps } else { self.dc_w[l.row] };
        }
        if self.uic {
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
            self.output = 0.0;
        } else {
            self.work.v.copy_from_slice(&self.dc_v);
            self.output = self.operating_output();
        }
        self.work.evaluate(&mut self.devices, 0.0);
    }

    /// One sample: the driven source takes `input`, every other source
    /// its value at the new time; returns the probe's voltage.
    pub fn step(&mut self, input: f64) -> f64 {
        self.samples += 1;
        let time = self.samples as f64 * self.step;
        for (k, source) in self.inputs.iter_mut().enumerate() {
            self.u[k] = if self.driven == Some(k) {
                input
            } else {
                match source {
                    Input::Source(s) => s.value_at(time),
                    Input::Expression { tape, stack } => tape.eval(&[], time, stack).value,
                }
            };
        }
        let (nx, nu) = (self.x.len(), self.u.len());
        self.p.iter_mut().for_each(|p| *p = 0.0);
        accumulate(&self.dx, &self.x, &mut self.p, nx);
        accumulate(&self.du, &self.u, &mut self.p, nu);
        self.work.solve(
            Prediction {
                p: &self.p,
                k: &self.k,
            },
            &mut self.devices,
            time,
            &self.settings,
            &mut self.health,
        );
        let q = &self.work.q;
        let mut y = [0.0];
        accumulate(&self.ex, &self.x, &mut y, nx);
        accumulate(&self.eu, &self.u, &mut y, nu);
        accumulate(&self.eq, q, &mut y, self.nq);
        self.x_next.iter_mut().for_each(|x| *x = 0.0);
        accumulate(&self.ax, &self.x, &mut self.x_next, nx);
        accumulate(&self.au, &self.u, &mut self.x_next, nu);
        accumulate(&self.aq, q, &mut self.x_next, self.nq);
        for x in &mut self.x_next {
            *x = crate::kernel::denormal::flush64(*x);
        }
        std::mem::swap(&mut self.x, &mut self.x_next);
        self.output = y[0];
        y[0]
    }

    /// How long the linearised circuit rings, from its slowest pole.
    pub fn tail(&self) -> Tail {
        self.tail
    }

    /// The discrete small-signal response from the input source (the
    /// driven one, else the deck's `AC` source, scaled by its magnitude)
    /// to the probe at `hz`, linearised at the operating point. `None`
    /// with no input source.
    pub fn frequency_response(&self, hz: f64) -> Option<Phasor> {
        let angle = std::f64::consts::TAU * hz * self.step;
        self.response_at(angle)
    }

    /// The analog response at `hz`: the discrete response at the
    /// frequency the bilinear transform maps there, exact for a linear
    /// circuit (module docs).
    pub fn analog_response(&self, hz: f64) -> Option<Phasor> {
        let angle = 2.0 * (std::f64::consts::PI * hz * self.step).atan();
        self.response_at(angle)
    }
}

mod build;
mod discretise;

use build::Builder;

fn pin(node: Node) -> Pin {
    node.index().checked_sub(1)
}

#[inline]
fn read(w: &[f64], p: Pin) -> f64 {
    p.map_or(0.0, |i| w[i])
}

#[cfg(test)]
mod tests;
