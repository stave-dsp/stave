//! A circuit as data: named [`Node`]s joined by [`Element`]s (R, C, L,
//! independent and controlled sources, devices, behavioural sources),
//! with the stimulus, analyses and probe a SPICE deck carries. One
//! netlist feeds ngspice and stave alike: [`Netlist::parse_spice`] reads
//! the SPICE subset `stave-test/spice/*.cir` uses,
//! [`Netlist::parse_kicad`] a KiCad schematic's netlist export
//! ([`kicad`]), and a circuit method solves it. [`tolerance`] draws
//! built units of a netlist from its parts' tolerances; [`ageing`] applies
//! explicit per-part drift assumptions to those units (DSP.md E10).
//!
//! ```
//! use stave_dsp::circuit::netlist::Netlist;
//!
//! let netlist = Netlist::parse_spice(
//!     "* RC low-pass
//!      V1 in 0 SIN(0 1 1k)
//!      R1 in out 1k
//!      C1 out 0 100n
//!      .tran 20.8333333u 5m
//!      .end",
//! )
//! .unwrap();
//! assert_eq!(netlist.elements().len(), 3);
//! assert!(netlist.node("out").is_some());
//! ```

pub mod ageing;
pub mod expr;
pub mod kicad;
pub mod spice;
pub mod tolerance;
pub mod variable;

use std::fmt;

use crate::circuit::devices::diode::Shockley;
use crate::circuit::devices::opamp::Amp;
use crate::circuit::devices::ota::BipolarPair;
use crate::circuit::devices::pentode::koren::Koren as PentodeKoren;
use crate::circuit::devices::pentode::Interelectrode as PentodeCapacitances;
use crate::circuit::devices::triode::{Interelectrode as TriodeCapacitances, Tube};
use crate::circuit::devices::{bjt, jfet, mosfet};
pub use expr::Expr;
pub use variable::{Taper, Variable};

/// A node of a [`Netlist`]; [`Node::GROUND`] is SPICE's `0`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node(usize);

impl Node {
    pub const GROUND: Self = Self(0);

    /// The node's number: 0 for ground, then in order of first use.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// One named element and its connections.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The SPICE name (`R1`, `D2`), lower-cased.
    pub name: String,
    pub kind: ElementKind,
}

/// What an element is. Two-terminal elements run from `a` (SPICE's
/// positive node) to `b`; a current is positive flowing from `a`
/// through the element to `b`.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    Resistor {
        a: Node,
        b: Node,
        ohms: f64,
    },
    Capacitor {
        a: Node,
        b: Node,
        farads: f64,
        /// `IC=`: the voltage at t = 0 under `UIC`.
        initial_volts: Option<f64>,
    },
    Inductor {
        a: Node,
        b: Node,
        henries: f64,
        /// `IC=`: the current at t = 0 under `UIC`.
        initial_amps: Option<f64>,
    },
    VoltageSource {
        a: Node,
        b: Node,
        source: Source,
    },
    CurrentSource {
        a: Node,
        b: Node,
        source: Source,
    },
    /// SPICE `E`: v(a, b) = gain · v(control).
    VoltageControlledVoltage {
        a: Node,
        b: Node,
        control: (Node, Node),
        gain: f64,
    },
    /// SPICE `F`: a current of gain · (the current through the named
    /// voltage source).
    CurrentControlledCurrent {
        a: Node,
        b: Node,
        control: String,
        gain: f64,
    },
    /// SPICE `G`: a current of gain · v(control) (siemens) from `a`
    /// through the element to `b`.
    VoltageControlledCurrent {
        a: Node,
        b: Node,
        control: (Node, Node),
        gain: f64,
    },
    /// A junction from `anode` to `cathode`; the model's
    /// [`series_ohms`](Shockley::series_ohms) is a resistor in series.
    Diode {
        anode: Node,
        cathode: Node,
        diode: Shockley,
    },
    /// SPICE `Q`: a BJT, its card's area factor applied. The ohmic
    /// resistances in the model become resistors to internal nodes;
    /// `substrate` is where its substrate junction's capacitance (`CJS`)
    /// returns (ground unless the element names a node).
    Bjt {
        collector: Node,
        base: Node,
        emitter: Node,
        substrate: Node,
        bjt: bjt::GummelPoon,
    },
    /// SPICE `J`: a JFET, its area factor applied.
    Jfet {
        drain: Node,
        gate: Node,
        source: Node,
        jfet: jfet::ShichmanHodges,
    },
    /// SPICE `M`: a level-1 MOSFET, the element's `W/L` folded into
    /// the model's `beta`.
    Mosfet {
        drain: Node,
        gate: Node,
        source: Node,
        bulk: Node,
        mosfet: mosfet::ShichmanHodges,
    },
    /// A triode by stave's device models (no SPICE spelling; built in
    /// code), with its interelectrode capacitances as fixed capacitors.
    Triode {
        plate: Node,
        grid: Node,
        cathode: Node,
        tube: Tube,
        capacitances: TriodeCapacitances,
    },
    /// A pentode or beam tetrode by stave's device models (no SPICE
    /// spelling — ngspice decks write the same law as `B` sources; built
    /// in code).
    Pentode {
        plate: Node,
        screen: Node,
        grid: Node,
        cathode: Node,
        tube: PentodeKoren,
        capacitances: PentodeCapacitances,
    },
    /// An op-amp (no SPICE primitive; built in code): its inputs draw no
    /// current and its output is a voltage source to ground. `supply` is
    /// the rails `(negative, positive)` in volts, which a
    /// [`Macromodel`](crate::circuit::devices::opamp::Macromodel) clips
    /// short of; the [`Ideal`](crate::circuit::devices::opamp::Ideal)
    /// ignores them.
    OpAmp {
        plus: Node,
        minus: Node,
        output: Node,
        amp: Amp,
        supply: (f64, f64),
    },
    /// An OTA (no SPICE primitive; built in code): a current into
    /// `output` set by v(plus, minus), and the bias pin a junction from
    /// `bias` to `negative` (the negative supply) whose current the chip
    /// mirrors as `Iabc`.
    Ota {
        plus: Node,
        minus: Node,
        output: Node,
        bias: Node,
        negative: Node,
        ota: BipolarPair,
    },
    /// SPICE `B … I = {expr}`.
    BehaviouralCurrent { a: Node, b: Node, expression: Expr },
    /// SPICE `B … V = {expr}`.
    BehaviouralVoltage { a: Node, b: Node, expression: Expr },
}

/// An independent source: its DC value, its AC magnitude (small-signal
/// analyses), and its transient signal (DC when absent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub dc: f64,
    pub ac: f64,
    pub signal: Option<Signal>,
}

/// A SPICE transient source function.
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    /// `SIN(offset amplitude hz [delay damping phase])`.
    Sine {
        offset: f64,
        amplitude: f64,
        hz: f64,
        delay: f64,
        damping: f64,
        phase_degrees: f64,
    },
    /// `PWL(t1 v1 t2 v2 …)`: linear between points, held outside them.
    Pwl(Vec<(f64, f64)>),
}

impl Source {
    /// A constant source.
    pub fn dc(volts_or_amps: f64) -> Self {
        Self {
            dc: volts_or_amps,
            ..Self::default()
        }
    }

    /// The value at `time` seconds, as ngspice's transient analysis
    /// evaluates it.
    pub fn value_at(&self, time: f64) -> f64 {
        match &self.signal {
            None => self.dc,
            Some(signal) => signal.value_at(time),
        }
    }
}

impl Signal {
    pub fn value_at(&self, time: f64) -> f64 {
        match self {
            Signal::Sine {
                offset,
                amplitude,
                hz,
                delay,
                damping,
                phase_degrees,
            } => {
                let phase = phase_degrees.to_radians();
                if time < *delay {
                    offset + amplitude * phase.sin()
                } else {
                    let t = time - delay;
                    offset
                        + amplitude
                            * (-t * damping).exp()
                            * (std::f64::consts::TAU * hz * t + phase).sin()
                }
            }
            Signal::Pwl(points) => {
                let Some(&(t0, v0)) = points.first() else {
                    return 0.0;
                };
                if time <= t0 {
                    return v0;
                }
                for pair in points.windows(2) {
                    let ((ta, va), (tb, vb)) = (pair[0], pair[1]);
                    if time <= tb {
                        return if tb > ta {
                            va + (vb - va) * (time - ta) / (tb - ta)
                        } else {
                            vb
                        };
                    }
                }
                points[points.len() - 1].1
            }
        }
    }
}

/// A voltage the deck reads out: v(plus, minus).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub plus: Node,
    pub minus: Node,
}

impl Probe {
    /// A node's voltage to ground.
    pub const fn node(node: Node) -> Self {
        Self {
            plus: node,
            minus: Node::GROUND,
        }
    }
}

/// `.tran step stop [start [max_step]] [UIC]`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tran {
    pub step_seconds: f64,
    pub stop_seconds: f64,
    /// Start from the elements' initial conditions, not the operating
    /// point.
    pub uic: bool,
}

/// `.ac dec points start stop`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Ac {
    pub points_per_decade: u32,
    pub start_hz: f64,
    pub stop_hz: f64,
}

/// A circuit: nodes, elements, and the deck's analyses and probe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Netlist {
    /// The deck's first line.
    pub title: String,
    names: Vec<String>,
    elements: Vec<Element>,
    pub tran: Option<Tran>,
    pub ac: Option<Ac>,
    /// The voltage the deck writes out (`wrdata … v(a, b)`).
    pub probe: Option<Probe>,
    /// The circuit's temperature in °C (`.temp` or `.options temp=`),
    /// which sets the junctions' thermal voltage; `None` is SPICE's
    /// 27 °C. Model cards are read at this temperature (`tnom` must
    /// match: temperature scaling is not modelled).
    pub temperature_celsius: Option<f64>,
    /// The conductance SPICE puts across every semiconductor junction
    /// (`.options gmin=`, siemens): `cd = Is·(exp(vd/(N·Vt)) − 1) +
    /// gmin·vd` in ngspice's `dioload.c`, `bjtload.c` and `jfetload.c`.
    /// `None` is SPICE's 1e-12 ([`Netlist::junction_gmin`]). Part of the
    /// equations a deck solves, so methods stamp it: a reverse-biased
    /// junction leaks `gmin·v` (a J201's gate–drain junction 4.3 pA at
    /// 4.3 V, 4.3 µV across a 1 MΩ gate leak).
    pub gmin_siemens: Option<f64>,
    /// Model-card parameters the parser accepted but no stave element
    /// models (a diode's junction capacitance, transit time),
    /// as `model: parameter` — so what a comparison leaves out is on
    /// record.
    pub unmodelled: Vec<String>,
    /// Elements declared variable ([`Netlist::add_variable`]).
    pub(crate) variables: Vec<Variable>,
}

impl Netlist {
    /// An empty netlist with only the ground node.
    pub fn new() -> Self {
        Self {
            names: vec!["0".into()],
            ..Self::default()
        }
    }

    /// SPICE's default `gmin`: the conductance across every junction when
    /// the deck sets none.
    pub const DEFAULT_GMIN_SIEMENS: f64 = 1e-12;

    /// The junction conductance methods stamp
    /// ([`gmin_siemens`](Self::gmin_siemens), SPICE's 1e-12 S unless the
    /// deck sets it).
    pub fn junction_gmin(&self) -> f64 {
        self.gmin_siemens.unwrap_or(Self::DEFAULT_GMIN_SIEMENS)
    }

    /// The node called `name`, created on first use (`0` and `gnd` are
    /// ground). Names are case-insensitive, as in SPICE.
    pub fn add_node(&mut self, name: &str) -> Node {
        if self.names.is_empty() {
            self.names.push("0".into());
        }
        let name = name.to_ascii_lowercase();
        if name == "gnd" {
            return Node::GROUND;
        }
        if let Some(i) = self.names.iter().position(|n| *n == name) {
            return Node(i);
        }
        self.names.push(name);
        Node(self.names.len() - 1)
    }

    /// The node called `name`, if the netlist has one.
    pub fn node(&self, name: &str) -> Option<Node> {
        let name = name.to_ascii_lowercase();
        if name == "gnd" {
            return Some(Node::GROUND);
        }
        self.names.iter().position(|n| *n == name).map(Node)
    }

    /// A node's name.
    pub fn name(&self, node: Node) -> &str {
        &self.names[node.0]
    }

    /// Nodes, ground included.
    pub fn node_count(&self) -> usize {
        self.names.len().max(1)
    }

    /// Every node in index order, ground first.
    pub fn nodes(&self) -> impl Iterator<Item = Node> + use<> {
        (0..self.node_count()).map(Node)
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub fn element(&self, name: &str) -> Option<&Element> {
        let name = name.to_ascii_lowercase();
        self.elements.iter().find(|e| e.name == name)
    }

    /// The same netlist with every device's capacitance taken out: junction
    /// depletion and transit-time charge, a MOSFET's overlaps, a tube's
    /// interelectrode capacitors (DSP.md R11) — the cards' static part.
    /// Capacitor elements stay.
    pub fn without_charge(&self) -> Self {
        use crate::circuit::devices::{pentode, triode};
        let mut out = self.clone();
        for e in &mut out.elements {
            match &mut e.kind {
                ElementKind::Diode { diode, .. } => {
                    diode.capacitance.zero_bias_farads = 0.0;
                    diode.transit_seconds = 0.0;
                }
                ElementKind::Bjt { bjt, .. } => *bjt = bjt.without_charge(),
                ElementKind::Jfet { jfet, .. } => {
                    jfet.gate_source_capacitance.zero_bias_farads = 0.0;
                    jfet.gate_drain_capacitance.zero_bias_farads = 0.0;
                }
                ElementKind::Mosfet { mosfet, .. } => {
                    mosfet.gate_source_overlap_farads = 0.0;
                    mosfet.gate_drain_overlap_farads = 0.0;
                    mosfet.gate_bulk_overlap_farads = 0.0;
                    mosfet.bulk_drain_capacitance.zero_bias_farads = 0.0;
                    mosfet.bulk_source_capacitance.zero_bias_farads = 0.0;
                }
                ElementKind::Triode { capacitances, .. } => *capacitances = triode::Interelectrode::NONE,
                ElementKind::Pentode { capacitances, .. } => *capacitances = pentode::Interelectrode::NONE,
                _ => {}
            }
        }
        out
    }

    /// True when some device stores charge ([`without_charge`](Self::without_charge)
    /// would change it).
    pub fn stores_charge(&self) -> bool {
        self.without_charge() != *self
    }

    /// Take the named element out (its nodes stay), returning it.
    pub fn remove(&mut self, name: &str) -> Option<Element> {
        let name = name.to_ascii_lowercase();
        let at = self.elements.iter().position(|e| e.name == name)?;
        Some(self.elements.remove(at))
    }

    pub fn element_mut(&mut self, name: &str) -> Option<&mut Element> {
        let name = name.to_ascii_lowercase();
        self.elements.iter_mut().find(|e| e.name == name)
    }

    /// Add an element; its name must be new.
    pub fn add(&mut self, name: &str, kind: ElementKind) -> Result<(), NetlistError> {
        let name = name.to_ascii_lowercase();
        if self.elements.iter().any(|e| e.name == name) {
            return Err(NetlistError::Duplicate { name });
        }
        if self.names.is_empty() {
            self.names.push("0".into());
        }
        self.elements.push(Element { name, kind });
        Ok(())
    }
}

/// Why a netlist could not be read or solved.
#[derive(Clone, Debug, PartialEq)]
pub enum NetlistError {
    /// A line the SPICE subset cannot read.
    Syntax { line: usize, message: String },
    /// A `D` element names a model the deck does not define.
    UnknownModel { line: usize, model: String },
    /// An element name used twice.
    Duplicate { name: String },
    /// A reference to an element that does not exist (an `F` source's
    /// controlling voltage source, a driven source).
    UnknownElement { name: String },
    /// An element this method does not solve, and why.
    Unsupported { element: String, reason: String },
    /// The circuit's equations have no unique solution (a floating node,
    /// a loop of voltage sources).
    Singular { analysis: &'static str },
    /// No output: the deck has no probe and none was given.
    NoProbe,
    /// Newton and source stepping both failed to find a DC solution.
    NoOperatingPoint,
}

impl fmt::Display for NetlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { line, message } => write!(f, "line {line}: {message}"),
            Self::UnknownModel { line, model } => write!(f, "line {line}: no .model {model}"),
            Self::Duplicate { name } => write!(f, "element {name} defined twice"),
            Self::UnknownElement { name } => write!(f, "no element {name}"),
            Self::Unsupported { element, reason } => write!(f, "{element}: {reason}"),
            Self::Singular { analysis } => {
                write!(f, "the {analysis} equations are singular (floating node or source loop?)")
            }
            Self::NoProbe => write!(f, "no probe: the deck writes no output and none was given"),
            Self::NoOperatingPoint => write!(f, "no operating point: Newton and source stepping failed"),
        }
    }
}

impl std::error::Error for NetlistError {}

#[cfg(test)]
mod tests;
