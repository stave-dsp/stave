//! **Layer 4 — circuit.** Models derived from schematics, in separate
//! concerns so any circuit can be solved any supported way:
//!
//! - [`devices`] — one module per device family (diode, BJT, JFET,
//!   MOSFET, triode, magnetic), one trait per family that every
//!   published model of it implements, parts as constants;
//! - [`netlist`] — a circuit as data ([`Netlist`]): nodes, elements, and
//!   the SPICE subset the ngspice decks are written in;
//! - [`methods`] — how a circuit is solved: the nodal DK method for any
//!   netlist, wave digital filters (trees under an R-type root, also
//!   from any netlist), and the hand-derivation tools (Newton with SPICE
//!   junction limiting, load lines, Wright omega);
//! - [`topologies`] — named circuits, each a hand-derived solution of
//!   its schematic with its ngspice case in stave-test, checked against
//!   [`Circuit<Dk>`](Circuit) on the same netlist.
//!
//! *Claim:* a model solves the schematic it names. *Verified by:*
//! ngspice on the same netlist (`stave-test/tests/matches_ngspice.rs`
//! for the topologies, `dk_matches_ngspice.rs` for `Circuit<Dk>`,
//! netlists in `stave-test/spice/`), and high-precision solutions where
//! SPICE has no model (`matches_reference.rs`). Circuits speak volts;
//! [`AudioAdapter`](crate::AudioAdapter) maps them to full scale.
//!
//! ```
//! use stave_dsp::circuit::methods::dk::Dk;
//! use stave_dsp::circuit::netlist::Netlist;
//! use stave_dsp::circuit::Circuit;
//! use stave_dsp::Stage;
//!
//! let netlist = Netlist::parse_spice(
//!     "* diode clipper
//!      V1 in 0 0
//!      R1 in out 2.2k
//!      C1 out 0 10n
//!      D1 out 0 DI
//!      D2 0 out DI
//!      .model DI D(Is=2.52n N=1.752)
//!      .control
//!      wrdata out v(out)
//!      .endc",
//! )
//! .unwrap();
//! let mut clipper = Circuit::<Dk>::new(&netlist).unwrap();
//! clipper.drive("V1").unwrap();
//! clipper.prepare(96_000.0);
//! let volts = (0..96).map(|_| clipper.tick(5.0)).last().unwrap();
//! assert!((0.5..0.8).contains(&volts), "a forward drop: {volts}");
//! assert_eq!(clipper.stats().capped, 0);
//! ```

pub mod audio;
pub mod devices;
pub mod methods;
pub mod netlist;
pub mod topologies;
pub mod variable;

use crate::kernel::node::{Stage, Tail};
use methods::dk::Dk;
use methods::wdf::Wdf;
pub use netlist::{Netlist, NetlistError};
pub use variable::{Pot, Smoothing, Switch, VariableCapacitor, VariableResistor};

/// A way of solving a [`Netlist`]: what `Circuit<M>` runs. Each method
/// documents which netlists it accepts.
pub trait Method: Sized {
    /// Build from `netlist`, or say why this method cannot solve it.
    fn build(netlist: &Netlist) -> Result<Self, NetlistError>;
    fn prepare(&mut self, sample_rate: f32);
    fn reset(&mut self);
    /// One sample in volts (or amps, for a current source) in and out.
    fn step(&mut self, input: f64) -> f64;
    fn tail(&self) -> Tail;
}

impl Method for Dk {
    fn build(netlist: &Netlist) -> Result<Self, NetlistError> {
        Dk::new(netlist)
    }
    fn prepare(&mut self, sample_rate: f32) {
        Dk::prepare(self, sample_rate);
    }
    fn reset(&mut self) {
        Dk::reset(self);
    }
    fn step(&mut self, input: f64) -> f64 {
        Dk::step(self, input)
    }
    fn tail(&self) -> Tail {
        Dk::tail(self)
    }
}

impl Method for Wdf {
    fn build(netlist: &Netlist) -> Result<Self, NetlistError> {
        Wdf::new(netlist)
    }
    fn prepare(&mut self, sample_rate: f32) {
        Wdf::prepare(self, sample_rate);
    }
    fn reset(&mut self) {
        Wdf::reset(self);
    }
    fn step(&mut self, input: f64) -> f64 {
        Wdf::step(self, input)
    }
    fn tail(&self) -> Tail {
        Wdf::tail(self)
    }
}

/// One runtime type for every netlist: a [`Netlist`] solved by the
/// method `M` — `Circuit::<Dk>::new(&netlist)`, `Circuit::<Wdf>::new(&netlist)`.
/// The method is a type parameter, never part of a name; `Circuit<M>`
/// dereferences to it for the method's own controls ([`Dk::drive`],
/// [`Dk::stats`], [`Dk::analog_response`], [`Wdf::structure`]).
///
/// Parts a netlist declares variable ([`netlist::Variable`]: pots,
/// rheostats, variable resistors and capacitors) turn while it runs —
/// [`Dk::pot`], [`Dk::set_position`], [`Dk::set_smoothing`] — by a
/// low-rank update of the prepared matrices, never a rebuild
/// ([`variable`] has the design and its measurements). Each block's
/// solver account, [`Dk::take_health`], is what a plugin publishes as
/// telemetry (stave-core's `SolverTaps`).
///
/// # Models
/// The netlist it is given ([`Netlist`]), whatever its elements: no
/// circuit of its own.
///
/// # Derivation
/// The method `M`'s: for [`Dk`], the nodal DK state space of the
/// netlist's MNA equations, trapezoidal companions, Newton on the
/// nonlinear ports; for [`Wdf`], adapted leaves by the bilinear
/// transform, series and parallel adaptors, an R-type root from MNA
/// (Werner, Smith & Abel, DAFx-15) and the devices as one multi-port
/// nonlinear root solved by the K-method. Both discretise by the
/// bilinear transform, so on one netlist they agree to rounding.
///
/// # Verified
/// `crates/stave-test/tests/dk_matches_ngspice.rs::diode_clipper_matches_ngspice`
/// and its siblings (RC, coupler, op-amp clipper, common cathode, power
/// supply, tone stack, triode and push-pull transfers): every deck at
/// the hand topology's budget or tighter, with healthy solves; for one,
/// `crates/stave-test/tests/dk_matches_ngspice.rs::common_cathode_matches_ngspice`:
/// 1e-5 normalized RMS at 4×. `Circuit<Wdf>`:
/// `crates/stave-test/tests/wdf_matches_ngspice.rs::transient_decks_match_ngspice`
/// (fifteen decks at DK's budgets),
/// `crates/stave-test/tests/wdf_matches_ngspice.rs::tone_stack_matches_ngspice_and_yeh_smith`
/// (1e-10 dB, an R-type root) and
/// `crates/stave-test/tests/wdf_matches_ngspice.rs::bridged_t_matches_ngspice`,
/// and every circuit's R17 matrix at DK's T0 budgets in
/// `crates/stave-test/tests/characterise.rs::every_circuit_on_wdf`. `Circuit<Dk>` with stateful ADAA:
/// `crates/stave-test/tests/adaa_matches_ngspice.rs::opamp_clipper_with_adaa_matches_ngspice`
/// and its siblings (the in-band error ADAA adds, 1.2e-4 to 4.4e-3), and
/// `crates/stave-dsp/tests/aliasing_budgets.rs::dk_opamp_clipper_adaa_44k_holds_its_alias_budgets`
/// and its siblings (the alias figures, with and without ADAA).
///
/// # Cost
/// Datasheet `"Circuit<Dk> diode clipper x10"` (about 6
/// iterations/solve), `"Circuit<Dk> op-amp clipper"`,
/// `"Circuit<Dk> cathode x5 (B src)"`, `"Circuit<Dk> cathode x5 (Koren)"`
/// and `"Circuit<Dk> tone stack"`: 2–3× the hand derivations. The same
/// netlists as WDFs: `"Circuit<Wdf> diode clipper x10"`,
/// `"Circuit<Wdf> op-amp clipper"`, `"Circuit<Wdf> cathode x5 (Koren)"`,
/// `"Circuit<Wdf> tone stack"`, `"Circuit<Wdf> fuzz face"`, and the
/// scattering iterative method's `"Circuit<Wdf> diode clipper x10 (SIM)"`. `Circuit<Dk>` with
/// ADAA: `"Circuit<Dk> op-amp clipper 1x ADAA"` (closed form, one solve
/// a sample) and `"Circuit<Dk> Tube Screamer (JRC4558) 1x ADAA"`
/// (quadrature, two), each with its 2× and 4× lines beside the plain
/// 2×, 4× and 8× ones.
///
/// # Aliasing
/// None of its own by default; a nonlinear netlist expects to run
/// oversampled. [`Dk::set_antialiasing`] turns on first-order stateful
/// ADAA (DSP.md A1): closed form where the nonlinear subsystem is a
/// one-port of diodes (at 4× the op-amp clipper's aliasing is 21 dB below
/// plain 8×), quadrature otherwise (always lower aliasing, but dearer
/// than oversampling to the same level).
///
/// # Valid ranges
/// Its devices' and its method's: any positive rate; Newton caps and
/// continuations reported in [`Dk::health`] and [`Wdf::health`], never
/// silent.
#[derive(Clone, Debug)]
pub struct Circuit<M> {
    method: M,
}

impl<M: Method> Circuit<M> {
    pub fn new(netlist: &Netlist) -> Result<Self, NetlistError> {
        Ok(Self {
            method: M::build(netlist)?,
        })
    }

    /// One sample in `f64`: the driven source's value in, the probe's out.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        self.method.step(input)
    }
}

impl<M> std::ops::Deref for Circuit<M> {
    type Target = M;
    fn deref(&self) -> &M {
        &self.method
    }
}

impl<M> std::ops::DerefMut for Circuit<M> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.method
    }
}

impl<M: Method> Stage for Circuit<M> {
    fn prepare(&mut self, sample_rate: f32) {
        self.method.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.method.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        crate::kernel::denormal::normal64(self.method.step(f64::from(x)))
    }
    fn tail(&self) -> Tail {
        self.method.tail()
    }
}
