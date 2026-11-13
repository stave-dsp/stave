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
//! # Nonlinear charge
//!
//! A junction's charge `Q(v)` (depletion, transit time) is integrated as
//! SPICE integrates it — the trapezoidal rule on the charge, `i_n =
//! (2/T)·(Q(v_n) − Q(v_{n−1})) − i_{n−1}` — split so the precomputed
//! matrices survive:
//!
//! ```text
//!     Q(v) = C₀·v + Qn(v)        C₀ = dQ/dv at the operating point
//!     i_n  = (2/T)·C₀·v_n + (2/T)·Qn(v_n) − H_n
//!     H_{n+1} = (4/T)·C₀·v_n + (4/T)·Qn(v_n) − H_n
//! ```
//!
//! `C₀` is an ordinary capacitor (its `2C₀/T` in `S`, `H` its history
//! state), and `Qn(v)` is one more device output, read across the
//! junction's controlling voltage, whose column in `Bq` is `∓2/T` and
//! which feeds `H` through `Aq` with `4/T`. So `S⁻¹` is still computed
//! once per rate, the Newton solve sees the charge's Jacobian `(2/T)·(C(v)
//! − C₀)` through `K` like any device current's, and the states stay one
//! per capacitor. The trade-offs:
//!
//! - The nonlinear subsystem grows by one output per stored charge (its
//!   controls do not, except a BJT's external base–collector and
//!   substrate junctions and a MOSFET's bulk–drain junction, which sit
//!   across voltages its currents do not read): the Newton step costs
//!   `O(nv²·nq)`.
//! - `C₀` is the operating point's capacitance, so the small-signal
//!   response ([`Dk::analog_response`]) is the linear part alone, exact
//!   for the linearised circuit as ngspice's `.ac` is; the residual
//!   cross-capacitances (a BJT's `∂qbe/∂vbc`) enter through `K·Jq` like
//!   any device's. Re-solving the operating point (a new driven source)
//!   re-sizes `C₀` and re-discretises.
//! - Splitting a junction into a fixed capacitor at the operating point
//!   plus a correction that is only *evaluated*, not integrated, would
//!   save the outputs but lose charge conservation away from the
//!   operating point; the charge form keeps it exactly, as SPICE does.
//! - The trapezoidal rule's undamped mode at Nyquist is SPICE's too:
//!   a junction whose capacitance swings over a sample rings as it would
//!   in ngspice at that step.
//!
//! Fixed capacitances (a tube's interelectrode capacitors, a MOSFET's
//! overlaps) are linear capacitors in the network.
//!
//! # Accepts
//!
//! Any netlist of R, C, L, independent V and I sources (DC, SIN, PWL, or
//! a behavioural source of time alone), E, F and G controlled sources,
//! Shockley diodes (series resistance as a resistor, breakdown
//! included), Gummel–Poon BJTs, Shichman–Hodges JFETs and square-law
//! MOSFETs (their ohmic resistances as resistors to internal nodes; a
//! junction shunted like a diode's), each with the charge its card stores
//! (module docs), stave triodes and pentodes with their interelectrode
//! capacitances, op-amps
//! (the ideal nullor, or the macromodel's gain node, compensation
//! capacitor and output buffer), OTAs, and behavioural sources of node
//! voltages (`B … I = {…}` or
//! `V = {…}`, at most four node voltages each), provided its MNA matrix
//! is nonsingular (no floating node, no loop of voltage sources). Every
//! diode, BJT and JFET junction carries SPICE's `gmin` (the deck's
//! `.options gmin=`, else 1e-12 S) as a linear conductance, as ngspice's
//! device loads add it, so a reverse-biased junction leaks as it does
//! there. The
//! operating point is solved with capacitors open and inductors shorted,
//! by Newton and then source stepping; a deck with `.tran … UIC` starts
//! from its `IC=` values instead.
//!
//! # Verified
//!
//! `stave-test/tests/dk_matches_ngspice.rs`: every ngspice deck whose
//! devices exist runs through `Circuit<Dk>` and meets the budget of the
//! hand-derived topology on the same deck, and agrees with that
//! topology; `transistors_match_ngspice.rs` holds the BJT, JFET and
//! MOSFET decks (DC families and real circuits) to ngspice.
//!
//! # Cost
//!
//! `stave-dsp/tests/datasheet.rs` lists `Circuit<Dk>` lines beside the
//! hand-derived topologies'.
//!
//! # Anti-aliasing
//!
//! None by default. [`Dk::set_antialiasing`] turns on first-order
//! stateful ADAA (Holters 2019; [`adaa`] has the update,
//! [`methods::adaa`](crate::circuit::methods::adaa) the derivation and the
//! measured alias levels): the matrices designed for `1.5·T`, the states
//! and inputs averaged, the solve's outputs replaced by their mean over
//! the sample — in closed form for a one-port of diodes, by Lobatto
//! quadrature otherwise ([`Mean`]).

pub mod adaa;
pub mod devices;
pub mod matrix;
mod noise;
pub mod solve;
mod variable;

pub use adaa::Mean;
pub use noise::{Noise, Supply, BOLTZMANN, ELECTRON_CHARGE};
pub(crate) use noise::{NoiseSite, Site};
use noise::NoiseState;

use devices::Nonlinear;
pub use matrix::Phasor;
use matrix::accumulate;
pub use solve::{Health, Settings};
use solve::{Prediction, Workspace};

use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::newton::NewtonStats;
use crate::circuit::methods::prewarp;
use crate::circuit::netlist::expr::{Dual, Tape};
use crate::circuit::netlist::{Netlist, NetlistError, Node, Probe, Source};
use crate::kernel::node::Tail;

/// An index into the MNA unknowns, or ground.
pub(crate) type Pin = Option<usize>;

/// An independent source's value over time.
#[derive(Clone, Debug)]
pub(crate) enum Input {
    Source(Source),
    Expression { tape: Tape, stack: Vec<Dual> },
    /// A noise source ([`Dk::set_noise`]), by its index among the active
    /// ones: zero at the operating point and in every other method.
    Noise(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct Capacitor {
    pub a: Pin,
    pub b: Pin,
    pub farads: f64,
    pub initial_volts: f64,
}

/// A stored charge's place: its device and slot there, the capacitor
/// carrying its linear part, its output in `q`, and the global control
/// it sits across (positive in the capacitor's `a → b` sense).
#[derive(Copy, Clone, Debug)]
pub(crate) struct ChargeLink {
    device: usize,
    slot: usize,
    capacitor: usize,
    output: usize,
    control: (usize, f64),
}

#[derive(Clone, Debug)]
pub(crate) struct Inductor {
    pub row: usize,
    pub henries: f64,
    pub initial_amps: f64,
}

/// A netlist discretised by the nodal DK method: the runtime behind
/// `Circuit<Dk>`.
///
/// **Models** the netlist it was built from, nothing else.
/// **Derivation** MNA, trapezoidal companions, the nonlinear subsystem
/// separated (module docs). **Verified** `dk_matches_ngspice.rs`.
/// **Cost** the datasheet's `Circuit<Dk>` lines. **Aliasing** none of its
/// own by default: a nonlinear netlist aliases as its devices do; run it
/// oversampled, or turn on first-order stateful ADAA
/// ([`set_antialiasing`](Self::set_antialiasing), DSP.md A1).
/// **Valid ranges** wherever its devices' models are.
#[derive(Clone, Debug)]
pub struct Dk {
    // Structure, fixed at build.
    pub(crate) n: usize,
    /// Node unknowns (netlist nodes and internal ones) come first.
    pub(crate) nodes: usize,
    pub(crate) static_matrix: Vec<f64>,
    pub(crate) bu: Vec<f64>,
    pub(crate) bq: Vec<f64>,
    pub(crate) controls: Vec<(Pin, Pin)>,
    pub(crate) capacitors: Vec<Capacitor>,
    pub(crate) inductors: Vec<Inductor>,
    pub(crate) inputs: Vec<Input>,
    pub(crate) input_ac: Vec<f64>,
    pub(crate) devices: Vec<Nonlinear>,
    pub(crate) charges: Vec<ChargeLink>,
    pub(crate) nq: usize,
    pub(crate) probe: (Pin, Pin),
    pub(crate) driven: Option<usize>,
    pub(crate) uic: bool,
    pub(crate) source_names: Vec<String>,
    // The operating point, fixed at build.
    pub(crate) dc_w: Vec<f64>,
    pub(crate) dc_v: Vec<f64>,
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
    /// The solves since the last `take_health` (or prepare), and before.
    health: Health,
    antialiasing: Antialiasing,
    /// The frequency every reactance is prewarped to (R12), `None` for
    /// the plain trapezoidal rule.
    prewarp: Option<f64>,
    adaa: adaa::State,
    earlier: Health,
    /// Variable parts (N6).
    pub(crate) varying: variable::Varying,
    /// Where noise can enter (H9): every resistor and junction.
    pub(crate) noise_sites: Vec<NoiseSite>,
    /// The noise in effect, if any.
    noise: Option<NoiseState>,
    /// The netlist's own sources: the inputs before any noise columns.
    pub(crate) base_inputs: usize,
    /// Sources held from outside ([`Dk::set_supply`]).
    held: Vec<Option<f64>>,
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
        dk.settle_operating_point()?;
        Ok(dk)
    }

    /// The MNA structure alone — no operating point, no discretisation —
    /// for a method that solves the same stamps its own way (`Wdf`).
    pub(crate) fn assemble(netlist: &Netlist, settings: Settings) -> Result<Self, NetlistError> {
        let probe = netlist.probe.ok_or(NetlistError::NoProbe)?;
        Builder::new(netlist).build(probe, settings)
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
        // The driven source rests at 0 (silence): the operating point moves,
        // and the junction capacitances sized there with it.
        self.settle_operating_point()
    }

    /// Solve the operating point, size the charges' linear capacitors
    /// there, discretise and return to it.
    fn settle_operating_point(&mut self) -> Result<(), NetlistError> {
        self.solve_operating_point()?;
        self.linearise_charges();
        self.discretise()?;
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

    /// Anti-alias the nonlinear subsystem (DSP.md A1): with
    /// [`Antialiasing::Adaa`], first-order stateful ADAA (Holters 2019,
    /// [`methods::adaa`](crate::circuit::methods::adaa)) — the solve's
    /// outputs averaged over each sample's line of predictions, in closed
    /// form or by quadrature ([`adaa_mean`](Self::adaa_mean)), the linear
    /// part delayed half a sample and designed for `1.5·T`. Re-prepares
    /// at the current rate.
    ///
    /// ```
    /// use stave_dsp::circuit::methods::adaa::Antialiasing;
    /// use stave_dsp::circuit::methods::dk::{Dk, Mean};
    /// use stave_dsp::circuit::netlist::Netlist;
    ///
    /// let netlist = Netlist::parse_spice(
    ///     "* clipper
    ///      V1 in 0 0
    ///      R1 in out 2.2k
    ///      C1 out 0 10n
    ///      D1 out 0 DI
    ///      D2 0 out DI
    ///      .model DI D(Is=2.52n N=1.752)
    ///      .control
    ///      wrdata out v(out)
    ///      .endc",
    /// )
    /// .unwrap();
    /// let mut dk = Dk::new(&netlist).unwrap();
    /// dk.drive("V1").unwrap();
    /// dk.set_antialiasing(Antialiasing::Adaa);
    /// // One junction voltage, both diodes through one port: closed form.
    /// assert_eq!(dk.adaa_mean(), Some(Mean::ClosedForm));
    /// let peak = (0..480)
    ///     .map(|n| dk.step(5.0 * (n as f64 * 0.13).sin()))
    ///     .fold(0.0f64, |m, v| m.max(v.abs()));
    /// assert!((0.5..0.9).contains(&peak), "a forward drop: {peak}");
    /// assert_eq!(dk.health().newton.capped, 0);
    /// ```
    pub fn set_antialiasing(&mut self, antialiasing: Antialiasing) {
        self.antialiasing = antialiasing;
        self.prepare(self.sample_rate);
    }

    pub fn antialiasing(&self) -> Antialiasing {
        self.antialiasing
    }

    /// How ADAA forms the solve's mean for this circuit, `None` with ADAA
    /// off.
    pub fn adaa_mean(&self) -> Option<Mean> {
        (self.antialiasing == Antialiasing::Adaa).then(|| self.adaa.kernel.mean())
    }

    /// Prewarp every reactance to `hz` (DSP.md R12,
    /// [`methods::prewarp`](crate::circuit::methods::prewarp)): the
    /// response is the analog one at `hz` and bent further below it;
    /// `None` is the plain trapezoidal rule. Re-prepares at the current
    /// rate.
    ///
    /// ```
    /// use stave_dsp::circuit::methods::dk::Dk;
    /// use stave_dsp::circuit::netlist::Netlist;
    ///
    /// let netlist = Netlist::parse_spice(
    ///     "* rc
    ///      V1 in 0 DC 0 AC 1
    ///      R1 in out 2.2k
    ///      C1 out 0 10n
    ///      .control
    ///      wrdata out v(out)
    ///      .endc",
    /// )
    /// .unwrap();
    /// let mut dk = Dk::new(&netlist).unwrap();
    /// let analog = 1.0 / (1.0 + (std::f64::consts::TAU * 16e3 * 22e-6).powi(2)).sqrt();
    /// let plain = dk.frequency_response(16e3).unwrap().magnitude();
    /// dk.set_prewarp(Some(16e3));
    /// let warped = dk.frequency_response(16e3).unwrap().magnitude();
    /// assert!((20.0 * (plain / analog).log10()).abs() > 3.5);
    /// assert!((warped / analog - 1.0).abs() < 1e-9);
    /// ```
    pub fn set_prewarp(&mut self, hz: Option<f64>) {
        self.prewarp = hz;
        self.prepare(self.sample_rate);
    }

    pub fn prewarp(&self) -> Option<f64> {
        self.prewarp
    }

    /// The step the linear part is discretised for: the sample period,
    /// prewarped, stretched by 1.5 under ADAA.
    fn design_step(&self) -> f64 {
        prewarp::design_step(self.step * self.antialiasing.design_step_scale(), self.prewarp)
    }

    /// Newton work, cap hits, continuation fallbacks and the worst
    /// residual since prepare.
    pub fn health(&self) -> Health {
        let mut all = self.earlier;
        all.merge(&self.health);
        all
    }

    /// The same account since the last call (or prepare), starting a new
    /// one: what a plugin publishes once a block (N8). Realtime.
    pub fn take_health(&mut self) -> Health {
        let window = self.health;
        self.earlier.merge(&window);
        self.health = Health::default();
        window
    }

    /// [`Health::newton`]: the counts every topology reports.
    pub fn stats(&self) -> NewtonStats {
        self.health().newton
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
        self.varying.variables.prepare(sample_rate);
        if let Some(noise) = self.noise.as_mut() {
            noise.prepare(&self.noise_sites, f64::from(sample_rate));
        }
        // Variable parts where they were set; a moved resistor moves the
        // operating point (kept as it was if the new one cannot be found).
        self.adopt_values();
        if self.varying.stale && self.solve_operating_point().is_ok() {
            self.varying.stale = false;
        }
        if self.discretise().is_err() {
            // The build discretised this circuit, so only a pathological rate
            // gets here: keep the last good rate rather than a broken one.
            debug_assert!(false, "circuit singular at {sample_rate} Hz");
            (self.sample_rate, self.step) = previous;
            let _ = self.discretise();
        }
        self.reset();
        self.health = Health::default();
        self.earlier = Health::default();
    }

    /// Back to t = 0 at the operating point (or the initial conditions).
    pub fn reset(&mut self) {
        self.settle();
        if let Some(noise) = self.noise.as_mut() {
            noise.restart();
        }
        let t = self.design_step();
        self.samples = 0;
        if self.uic {
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
        } else {
            self.work.v.copy_from_slice(&self.dc_v);
        }
        self.work.evaluate(&mut self.devices, 0.0);
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
        // A charge at rest: H = (2/T)·(C₀·v + Qn(v)), its current zero.
        for link in &self.charges {
            self.x[link.capacitor] += 2.0 / t * self.work.q[link.output];
        }
        self.output = if self.uic { 0.0 } else { self.operating_output() };
        if self.antialiasing == Antialiasing::Adaa {
            self.reset_adaa();
        }
    }

    /// Every source's value at `time` into `u`, the driven one at 0
    /// (its value is set by `step`), a held one at its held value, the
    /// noise sources' next draws.
    fn inputs_at(&mut self, time: f64) {
        let Self {
            inputs,
            u,
            driven,
            held,
            noise,
            noise_sites,
            varying,
            work,
            ..
        } = self;
        for (k, source) in inputs.iter_mut().enumerate() {
            u[k] = if *driven == Some(k) {
                0.0
            } else if let Some(Some(volts)) = held.get(k) {
                *volts
            } else {
                match source {
                    Input::Source(s) => s.value_at(time),
                    Input::Expression { tape, stack } => tape.eval(&[], time, stack).value,
                    Input::Noise(j) => match noise.as_mut() {
                        Some(n) => n.draw(*j, noise_sites, &varying.variables.values, &work.q, &work.v),
                        None => 0.0,
                    },
                }
            };
        }
    }

    /// One sample: the driven source takes `input`, every other source
    /// its value at the new time; returns the probe's voltage.
    pub fn step(&mut self, input: f64) -> f64 {
        if self.varying.variables.moving() {
            self.glide();
        }
        self.samples += 1;
        let time = self.samples as f64 * self.step;
        self.inputs_at(time);
        if let Some(k) = self.driven {
            self.u[k] = input;
        }
        let (nx, nu) = (self.x.len(), self.u.len());
        self.p.iter_mut().for_each(|p| *p = 0.0);
        accumulate(&self.dx, &self.x, &mut self.p, nx);
        accumulate(&self.du, &self.u, &mut self.p, nu);
        if self.antialiasing == Antialiasing::Adaa {
            self.output = self.step_adaa(time);
            return self.output;
        }
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
        let angle = prewarp::digital_angle(hz, prewarp::design_step(self.step, self.prewarp));
        self.response_at(angle)
    }
}

mod build;
mod discretise;

use build::Builder;

pub(crate) fn pin(node: Node) -> Pin {
    node.index().checked_sub(1)
}

#[inline]
pub(crate) fn read(w: &[f64], p: Pin) -> f64 {
    p.map_or(0.0, |i| w[i])
}

#[cfg(test)]
mod tests;
