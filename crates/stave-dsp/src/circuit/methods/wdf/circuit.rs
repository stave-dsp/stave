//! A [`Netlist`] as a wave digital filter: the runtime behind
//! `Circuit<Wdf>` ([`Wdf`]).

use super::forest::{Edge, Forest, Kind};
use crate::circuit::methods::dk::devices::Nonlinear;
use crate::circuit::methods::dk::matrix::{complex_solve, solve_columns, Phasor};
use crate::circuit::methods::dk::solve::{Prediction, Workspace};
use crate::circuit::methods::dk::{pin, read, Dk, Health, Input, Pin, Settings};
use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::newton::NewtonStats;
use crate::circuit::methods::prewarp;
use crate::circuit::netlist::{ElementKind, Netlist, NetlistError, Node};
use crate::kernel::node::Tail;

mod adaa;
mod scattering;
mod variable;

pub use scattering::NonlinearRoot;

/// Where an independent source's value goes each sample.
#[derive(Clone, Debug)]
enum Target {
    /// A voltage source adapted into a tree: its cell's volts.
    Leaf(usize),
    /// A source absorbed in the R-type root: its input column.
    Root(usize),
}

#[derive(Clone, Debug)]
struct Slot {
    name: String,
    input: Input,
    ac: f64,
    target: Target,
}

/// A port of the R-type root: a tree's root cell between two MNA pins.
#[derive(Copy, Clone, Debug)]
struct Port {
    cell: usize,
    plus: Pin,
    minus: Pin,
}

/// How a netlist was laid out as a wave digital filter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Structure {
    /// Adapted leaves: resistors, capacitors, inductors, voltage sources
    /// in series with them.
    pub leaves: usize,
    pub series: usize,
    pub parallel: usize,
    pub inverters: usize,
    /// Ports of the R-type root (trees, and macromodel compensation
    /// capacitors).
    pub ports: usize,
    /// Ports whose tree sends a wave up (holds state or a source).
    pub reflective_ports: usize,
    /// Ports whose tree takes a wave down (holds state).
    pub stateful_ports: usize,
    /// The root's nodal unknowns (node voltages and absorbed branch
    /// currents).
    pub root_unknowns: usize,
    /// Elements absorbed into the root (sources, controlled sources,
    /// op-amps, devices).
    pub absorbed: usize,
    /// The nonlinear root's size: distinct controlling voltages.
    pub nonlinear_ports: usize,
}

impl Structure {
    /// True when the root is no more than one port facing the devices:
    /// the netlist is a tree of series and parallel adaptors.
    pub fn is_tree(&self) -> bool {
        self.ports <= 1
    }
}

/// A netlist as a wave digital filter: the runtime behind
/// `Circuit<Wdf>`.
///
/// # Structure
///
/// 1. **Leaves.** Every resistor, capacitor and inductor is an adapted
///    one-port (capacitor and inductor by the bilinear transform), and so
///    is every independent voltage source that ends up in series with
///    one.
/// 2. **Trees.** Leaves on one node pair join by parallel adaptors, and
///    leaves meeting at a node nothing else touches join by series
///    adaptors, with polarity inverters where a subtree meets its adaptor
///    the other way up — repeated until neither applies (the
///    series–parallel reduction of the network graph). A node the probe
///    reads, ground, and every node a root element touches stay.
/// 3. **The root.** One R-type adaptor ([`RType`](super::RType)'s derivation) joins
///    the trees' roots, with everything no tree can hold *absorbed* into
///    its nodal matrix: ideal voltage and current sources, controlled
///    sources, op-amps (the ideal one as a nullor; the macromodel's
///    gain node, leak and output buffer, its compensation capacitor a
///    leaf port), and the conductances devices lend the network.
/// 4. **The nonlinear root.** Every device (diode, BJT, FET, triode,
///    pentode, OTA, the macromodel's gain stage, behavioural sources of
///    node voltages) is a port of one multi-port nonlinearity at the
///    root, solved by the K-method on the R-type adaptor's scattering
///    (Werner, Nangia, Smith & Abel, "Resolving wave digital filters with
///    multiple/multiport nonlinearities", DAFx-15): the adaptor's rows
///    give the devices' controlling voltages as `v = p + K·q(v)`, where
///    `p` is linear in the waves the trees send up and `q` are the
///    devices' currents — the same equation, and the same damped Newton
///    with junction limiting and continuation, as the DK method's.
///    [`set_nonlinear_root`](Self::set_nonlinear_root) chooses instead
///    the Scattering Iterative Method ([`NonlinearRoot::Scattering`]) for
///    a netlist whose devices are all diodes: each a wave port with its
///    own scalar solve, swept through the R-type root.
///
/// Each sample: the trees send their waves up (a resistor-only tree
/// sends nothing and is skipped), the nonlinear root solves, the R-type
/// adaptor's rows send waves down into every tree that holds state.
/// A subtree that neither reflects nor stores (resistors alone) is a
/// conductance in the root and costs nothing per sample.
///
/// # Accepts
///
/// Every netlist `Circuit<Dk>` accepts (its elements, a nonsingular
/// MNA matrix, an operating point), with positive, finite resistances,
/// capacitances and inductances — a wave port's resistance must be, for
/// the adaptors to be passive. Its devices are solved statically: their
/// cards' charge storage and a tube's interelectrode capacitors (DSP.md
/// R11) are left out ([`Netlist::without_charge`]), which only
/// `Circuit<Dk>` integrates. Anything else is a
/// [`NetlistError::Unsupported`] naming the element. The topology
/// never refuses a netlist: what no tree holds the R-type root absorbs.
///
/// The operating point (and the initial conditions under `UIC`) is the
/// DK method's DC analysis of the same netlist; the WDF starts every
/// wave there.
///
/// ```
/// use stave_dsp::circuit::methods::wdf::Wdf;
/// use stave_dsp::circuit::netlist::Netlist;
/// use stave_dsp::circuit::Circuit;
/// use stave_dsp::Stage;
///
/// // A bridged-T notch: a bridge, so no tree holds it.
/// let netlist = Netlist::parse_spice(
///     "* bridged-T
///      V1 i 0 0
///      C1 i m 10n
///      C2 m o 10n
///      R1 i o 100k
///      R2 m 0 1k
///      RL o 0 100k
///      .control
///      wrdata out v(o)
///      .endc",
/// )
/// .unwrap();
/// let mut notch = Circuit::<Wdf>::new(&netlist).unwrap();
/// assert!(!notch.structure().is_tree());
/// notch.drive("V1").unwrap();
/// notch.prepare(48_000.0);
/// let deep = notch.analog_response(1_590.0).unwrap().magnitude();
/// assert!(deep < 0.05, "the notch: {deep}");
/// let _ = notch.tick(0.1);
/// ```
///
/// **Models** the netlist it was built from, nothing else.
/// **Derivation** as above. **Verified** `wdf_matches_ngspice.rs`.
/// **Cost** the datasheet's `Circuit<Wdf>` lines. **Aliasing** none of
/// its own by default; run a nonlinear netlist oversampled, or turn on
/// first-order ADAA at the R-type root
/// ([`set_antialiasing`](Self::set_antialiasing), Albertini et al.
/// 2021; DSP.md A1), which matches `Circuit<Dk>`'s ADAA to the solves'
/// tolerance. **Valid ranges** wherever its devices' models are.
#[derive(Clone, Debug)]
pub struct Wdf {
    forest: Forest,
    ports: Vec<Port>,
    slots: Vec<Slot>,
    driven: Option<usize>,
    /// The DK method on the same netlist: the operating point and tail.
    operating: Dk,
    // The root, fixed at build.
    n: usize,
    static_matrix: Vec<f64>,
    bu: Vec<f64>,
    bq: Vec<f64>,
    nu: usize,
    nq: usize,
    controls: Vec<(Pin, Pin)>,
    devices: Vec<Nonlinear>,
    probe: (Pin, Pin),
    uic: bool,
    absorbed: usize,
    /// Each capacitor's cell and MNA pins, each inductor's cell and its
    /// branch row in the operating point: where their waves start.
    capacitors: Vec<(usize, Pin, Pin)>,
    inductors: Vec<(usize, usize)>,
    // Discretised at prepare.
    sample_rate: f32,
    step: f64,
    /// Ports whose tree reflects (the columns) and holds state (the rows).
    reflective: Vec<usize>,
    stateful: Vec<usize>,
    /// The same ports' root cells.
    reflective_cells: Vec<usize>,
    stateful_cells: Vec<usize>,
    /// Over `z = [β of reflective ports; u; q]`: the stateful ports'
    /// scattering rows, the probe's row, the controls' rows.
    scatter: Vec<f64>,
    probe_row: Vec<f64>,
    predict: Vec<f64>,
    k: Vec<f64>,
    // Running state.
    z: Vec<f64>,
    p: Vec<f64>,
    values: Vec<f64>,
    work: Workspace,
    root: NonlinearRoot,
    /// The root linearised at the operating point (`q = L·p`), on the
    /// copy a small-signal response is read from.
    linearised: Option<Vec<f64>>,
    /// The diodes as wave ports, under [`NonlinearRoot::Scattering`].
    scattering: Vec<scattering::Port>,
    /// The scalar root's device outputs and slopes.
    q1: Vec<f64>,
    jq1: Vec<f64>,
    samples: u64,
    output: f64,
    settings: Settings,
    /// The solves since the last `take_health` (or prepare), and before.
    health: Health,
    antialiasing: Antialiasing,
    /// The frequency every reactance is prewarped to (R12).
    prewarp: Option<f64>,
    adaa: adaa::State,
    earlier: Health,
    /// Variable parts (N6).
    pub(crate) varying: variable::Varying,
}

fn unsupported(name: &str, reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: name.to_string(),
        reason: reason.to_string(),
    }
}

/// The nodes an element joins (a behavioural source's controls too).
fn terminals(kind: &ElementKind) -> Vec<Node> {
    use ElementKind as K;
    match kind {
        K::Resistor { a, b, .. }
        | K::Capacitor { a, b, .. }
        | K::Inductor { a, b, .. }
        | K::VoltageSource { a, b, .. }
        | K::CurrentSource { a, b, .. }
        | K::CurrentControlledCurrent { a, b, .. } => vec![*a, *b],
        K::VoltageControlledVoltage { a, b, control, .. } | K::VoltageControlledCurrent { a, b, control, .. } => {
            vec![*a, *b, control.0, control.1]
        }
        K::Diode { anode, cathode, .. } => vec![*anode, *cathode],
        K::Bjt {
            collector,
            base,
            emitter,
            ..
        } => vec![*collector, *base, *emitter],
        K::Jfet { drain, gate, source, .. } => vec![*drain, *gate, *source],
        K::Mosfet {
            drain,
            gate,
            source,
            bulk,
            ..
        } => vec![*drain, *gate, *source, *bulk],
        K::Triode {
            plate, grid, cathode, ..
        } => vec![*plate, *grid, *cathode],
        K::Pentode {
            plate,
            screen,
            grid,
            cathode,
            ..
        } => vec![*plate, *screen, *grid, *cathode],
        K::OpAmp {
            plus, minus, output, ..
        } => vec![*plus, *minus, *output],
        K::Ota {
            plus,
            minus,
            output,
            bias,
            negative,
            ..
        } => vec![*plus, *minus, *output, *bias, *negative],
        K::BehaviouralCurrent { a, b, expression } | K::BehaviouralVoltage { a, b, expression } => {
            let mut nodes = vec![*a, *b];
            for (x, y) in expression.controls() {
                nodes.extend([x, y]);
            }
            nodes
        }
    }
}

fn positive(name: &str, value: f64, what: &str) -> Result<f64, NetlistError> {
    if value > 0.0 && value.is_finite() {
        Ok(value)
    } else {
        Err(unsupported(
            name,
            &format!("a wave port needs a positive, finite {what} (got {value})"),
        ))
    }
}

impl Wdf {
    /// Build from `netlist`, probing `netlist.probe`; prepared at 48 kHz.
    pub fn new(netlist: &Netlist) -> Result<Self, NetlistError> {
        Self::with_settings(netlist, Settings::DEFAULT)
    }

    pub fn with_settings(netlist: &Netlist, settings: Settings) -> Result<Self, NetlistError> {
        // Devices are solved statically here: their stored charge (DSP.md
        // R11) is the DK method's alone.
        let netlist = &netlist.without_charge();
        let operating = Dk::with_settings(netlist, settings)?;
        let elements = netlist.elements();
        // V sources an F element reads stay in the root, where their
        // current is an unknown.
        let sensed: Vec<String> = elements
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::CurrentControlledCurrent { control, .. } => Some(control.to_ascii_lowercase()),
                _ => None,
            })
            .collect();
        let mut forest = Forest::default();
        let mut edges = Vec::new();
        let mut leaf_of = Vec::new(); // (element index, cell)
        let mut pinned = vec![false; netlist.node_count()];
        pinned[0] = true;
        if let Some(probe) = netlist.probe {
            pinned[probe.plus.index()] = true;
            pinned[probe.minus.index()] = true;
        }
        let mut inductors = 0;
        for (i, e) in elements.iter().enumerate() {
            let name = e.name.as_str();
            let edge = match &e.kind {
                ElementKind::Resistor { a, b, ohms } => {
                    forest.leaf(Kind::Resistor, positive(name, *ohms, "resistance")?, *a, *b)
                }
                ElementKind::Capacitor {
                    a,
                    b,
                    farads,
                    initial_volts,
                } => {
                    let kind = Kind::Capacitor {
                        farads: positive(name, *farads, "capacitance")?,
                        initial_volts: *initial_volts,
                    };
                    forest.leaf(kind, 0.0, *a, *b)
                }
                ElementKind::Inductor {
                    a,
                    b,
                    henries,
                    initial_amps,
                } => {
                    let kind = Kind::Inductor {
                        henries: positive(name, *henries, "inductance")?,
                        initial_amps: *initial_amps,
                        ordinal: inductors,
                    };
                    inductors += 1;
                    forest.leaf(kind, 0.0, *a, *b)
                }
                ElementKind::VoltageSource { a, b, .. } if !sensed.contains(&e.name) => {
                    forest.leaf(Kind::Source, 0.0, *a, *b)
                }
                kind => {
                    for node in terminals(kind) {
                        pinned[node.index()] = true;
                    }
                    continue;
                }
            };
            leaf_of.push((i, edge.cell));
            edges.push(edge);
        }
        forest.reduce(edges, &pinned);
        // A voltage source left on its own is ideal: the root absorbs it.
        let alone: Vec<usize> = forest
            .roots
            .iter()
            .filter(|r| forest.cells[r.cell].kind == Kind::Source)
            .map(|r| r.cell)
            .collect();
        forest.roots.retain(|r| !alone.contains(&r.cell));
        let varying = variable::Varying::new(netlist, &leaf_of)?;
        // The root: the netlist less its leaves, stamped by the DK builder.
        let mut root = netlist.clone();
        root.variables.clear();
        let mut slots = Vec::new();
        for &(i, cell) in &leaf_of {
            let e = &elements[i];
            if alone.contains(&cell) {
                continue;
            }
            root.remove(&e.name);
            if let ElementKind::VoltageSource { source, .. } = &e.kind {
                slots.push(Slot {
                    name: e.name.clone(),
                    input: Input::Source(source.clone()),
                    ac: source.ac,
                    target: Target::Leaf(cell),
                });
            }
        }
        let absorbed = root.elements().len();
        let mna = Dk::assemble(&root, settings)?;
        for (j, name) in mna.source_names.iter().enumerate() {
            slots.push(Slot {
                name: name.clone(),
                input: mna.inputs[j].clone(),
                ac: mna.input_ac[j],
                target: Target::Root(j),
            });
        }
        let mut ports: Vec<Port> = forest
            .roots
            .iter()
            .map(|&Edge { cell, plus, minus }| Port {
                cell,
                plus: pin(plus),
                minus: pin(minus),
            })
            .collect();
        let mut capacitors = Vec::new();
        let mut coils = Vec::new();
        for (i, cell) in forest.cells.iter().enumerate() {
            match cell.kind {
                Kind::Capacitor { .. } => capacitors.push((i, pin(cell.terminals.0), pin(cell.terminals.1))),
                Kind::Inductor { ordinal, .. } => coils.push((i, operating.inductors[ordinal].row)),
                _ => {}
            }
        }
        // A macromodel's compensation capacitor: a leaf port of its own.
        for c in &mna.capacitors {
            let e = forest.leaf(
                Kind::Capacitor {
                    farads: c.farads,
                    initial_volts: None,
                },
                0.0,
                Node::GROUND,
                Node::GROUND,
            );
            ports.push(Port {
                cell: e.cell,
                plus: c.a,
                minus: c.b,
            });
            capacitors.push((e.cell, c.a, c.b));
        }
        let (nu, nq, nv) = (mna.inputs.len(), mna.nq, mna.controls.len());
        let mut wdf = Self {
            forest,
            ports,
            slots,
            driven: None,
            operating,
            n: mna.n,
            static_matrix: mna.static_matrix,
            bu: mna.bu,
            bq: mna.bq,
            nu,
            nq,
            controls: mna.controls,
            devices: mna.devices,
            probe: mna.probe,
            uic: mna.uic,
            absorbed,
            capacitors,
            inductors: coils,
            sample_rate: 48_000.0,
            step: 1.0 / 48_000.0,
            reflective: Vec::new(),
            stateful: Vec::new(),
            reflective_cells: Vec::new(),
            stateful_cells: Vec::new(),
            scatter: Vec::new(),
            probe_row: Vec::new(),
            predict: Vec::new(),
            k: Vec::new(),
            z: Vec::new(),
            p: vec![0.0; nv],
            values: Vec::new(),
            work: Workspace::new(nv, nq),
            root: NonlinearRoot::Newton,
            linearised: None,
            scattering: Vec::new(),
            q1: vec![0.0; nq],
            jq1: vec![0.0; nq],
            samples: 0,
            output: 0.0,
            settings,
            health: Health::default(),
            antialiasing: Antialiasing::None,
            prewarp: None,
            adaa: adaa::State::new(nu, nv, nq),
            earlier: Health::default(),
            varying,
        };
        wdf.values = vec![0.0; wdf.slots.len()];
        if wdf.operating.dc_v.len() != nv {
            return Err(unsupported("netlist", "the root's devices differ from the netlist's"));
        }
        wdf.discretise()?;
        wdf.reset();
        Ok(wdf)
    }

    /// Drive the named independent source from [`step`](Self::step)'s
    /// input; the operating point moves to the source at rest (0).
    pub fn drive(&mut self, source: &str) -> Result<(), NetlistError> {
        let name = source.to_ascii_lowercase();
        let slot = self
            .slots
            .iter()
            .position(|s| s.name == name)
            .ok_or(NetlistError::UnknownElement { name })?;
        self.operating.drive(source)?;
        self.driven = Some(slot);
        self.reset();
        Ok(())
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
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

    /// The probe's voltage after the last sample.
    pub fn output(&self) -> f64 {
        self.output
    }

    /// How the netlist was laid out.
    pub fn structure(&self) -> Structure {
        let (series, parallel, inverters, leaves) = self.forest.census();
        Structure {
            leaves,
            series,
            parallel,
            inverters,
            ports: self.ports.len(),
            reflective_ports: self.reflective.len(),
            stateful_ports: self.stateful.len(),
            root_unknowns: self.n,
            absorbed: self.absorbed,
            nonlinear_ports: self.controls.len(),
        }
    }

    /// Re-adapt for `sample_rate` and return to the operating point.
    pub fn prepare(&mut self, sample_rate: f32) {
        let previous = self.sample_rate;
        self.sample_rate = sample_rate;
        self.step = 1.0 / f64::from(sample_rate);
        self.varying.variables.prepare(sample_rate);
        self.adopt_values();
        if self.discretise().is_err() {
            debug_assert!(false, "circuit singular at {sample_rate} Hz");
            self.sample_rate = previous;
            self.step = 1.0 / f64::from(previous);
            let _ = self.discretise();
        }
        self.operating.prepare(self.sample_rate);
        self.reset();
        self.health = Health::default();
        self.earlier = Health::default();
    }

    /// Back to t = 0 at the operating point (or the initial conditions).
    pub fn reset(&mut self) {
        self.settle();
        self.samples = 0;
        let uic = self.uic;
        let dc = &self.operating.dc_w;
        for cell in &mut self.forest.cells {
            cell.alpha = 0.0;
            cell.beta = 0.0;
            cell.state = 0.0;
        }
        for &(i, a, b) in &self.capacitors {
            let cell = &mut self.forest.cells[i];
            cell.state = match (uic, cell.kind) {
                (true, Kind::Capacitor { initial_volts, .. }) => initial_volts.unwrap_or(0.0),
                _ => read(dc, a) - read(dc, b),
            };
        }
        for &(i, row) in &self.inductors {
            let cell = &mut self.forest.cells[i];
            let amps = match (uic, cell.kind) {
                (true, Kind::Inductor { initial_amps, .. }) => initial_amps.unwrap_or(0.0),
                _ => dc[row],
            };
            cell.state = cell.ohms * amps;
        }
        if uic {
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
            self.output = 0.0;
        } else {
            self.work.v.copy_from_slice(&self.operating.dc_v);
            self.output = self.operating.output();
        }
        self.work.evaluate(&mut self.devices, 0.0);
        self.reset_scattering();
        if self.adaa_active() {
            self.reset_adaa();
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
        for (k, slot) in self.slots.iter_mut().enumerate() {
            self.values[k] = if self.driven == Some(k) {
                input
            } else {
                match &mut slot.input {
                    Input::Source(s) => s.value_at(time),
                    Input::Expression { tape, stack } => tape.eval(&[], time, stack).value,
                    Input::Noise(_) => 0.0,
                }
            };
        }
        self.advance(time)
    }

    /// One sample from the sources' values in `self.values`.
    fn advance(&mut self, time: f64) -> f64 {
        if self.adaa_active() && self.linearised.is_none() {
            return self.advance_adaa(time);
        }
        let na = self.reflective.len();
        for (slot, &value) in self.slots.iter().zip(&self.values) {
            match slot.target {
                Target::Leaf(cell) => self.forest.cells[cell].state = value,
                Target::Root(j) => self.z[na + j] = value,
            }
        }
        self.forest.up();
        for (z, &cell) in self.z.iter_mut().zip(&self.reflective_cells) {
            *z = self.forest.cells[cell].beta;
        }
        let nl = na + self.nu;
        let cols = self.z.len();
        if !self.devices.is_empty() {
            for (i, p) in self.p.iter_mut().enumerate() {
                let row = &self.predict[i * nl..(i + 1) * nl];
                *p = row.iter().zip(&self.z[..nl]).map(|(m, z)| m * z).sum();
            }
            if let Some(l) = &self.linearised {
                let nv = self.p.len();
                for (j, z) in self.z[nl..].iter_mut().enumerate() {
                    *z = l[j * nv..(j + 1) * nv].iter().zip(&self.p).map(|(l, p)| l * p).sum();
                }
            } else if self.root == NonlinearRoot::Scattering {
                self.scattering_root(nl);
            } else {
                if self.p.len() != 1 || !self.scalar_root(time) {
                    self.work.solve(
                        Prediction { p: &self.p, k: &self.k },
                        &mut self.devices,
                        time,
                        &self.settings,
                        &mut self.health,
                    );
                }
                self.z[nl..].copy_from_slice(&self.work.q);
            }
        }
        let z = &self.z;
        let y: f64 = self.probe_row.iter().zip(z).map(|(m, z)| m * z).sum();
        for (row, &cell) in self.scatter.chunks_exact(cols.max(1)).zip(&self.stateful_cells) {
            let b: f64 = row.iter().zip(z).map(|(m, z)| m * z).sum();
            self.forest.cells[cell].alpha = b;
        }
        self.forest.down();
        self.output = y;
        y
    }

    /// The nonlinear root's residual `v − p − K·q(v)` and its slope for
    /// one controlling voltage; `q(v)` is left in `self.q1`.
    fn residual1(&mut self, v: f64, time: f64) -> (f64, f64) {
        self.jq1.iter_mut().for_each(|x| *x = 0.0);
        for d in &mut self.devices {
            d.evaluate(&[v], &mut self.q1, &mut self.jq1, 1, time);
        }
        let (mut r, mut slope) = (v - self.p[0], 1.0);
        for (j, &k) in self.k.iter().enumerate() {
            r -= k * self.q1[j];
            slope -= k * self.jq1[j];
        }
        (r, slope)
    }

    /// One controlling voltage — a single nonlinear port, the classic
    /// wave digital root — is solved as a scalar equation: damped Newton
    /// with the devices' junction limiting, warm-started, held to the
    /// same convergence test as the general solve. False (nothing
    /// recorded) when it does not converge: the general solve, with its
    /// continuation, takes the sample over.
    fn scalar_root(&mut self, time: f64) -> bool {
        let s = self.settings.newton;
        let mut v = self.work.v[0];
        let (mut r, mut slope) = self.residual1(v, time);
        let (mut iterations, mut step) = (0, f64::INFINITY);
        while iterations < s.max_iterations && (step.is_nan() || step.abs() >= s.tolerance) {
            let full = (r / slope).clamp(-s.max_step, s.max_step);
            if !full.is_finite() {
                return false;
            }
            let mut damping = 1.0;
            let (next, next_r, next_slope) = loop {
                let mut candidate = [v - damping * full];
                for d in &self.devices {
                    d.limit(&mut candidate, &[v]);
                }
                let (cr, cs) = self.residual1(candidate[0], time);
                if cr.abs() < r.abs() || damping <= s.min_damping {
                    break (candidate[0], cr, cs);
                }
                damping *= 0.5;
            };
            step = next - v;
            (v, r, slope) = (next, next_r, next_slope);
            iterations += 1;
        }
        let converged = step.abs() < s.tolerance && r.abs() <= 1e-6 * (1.0 + v.abs());
        if converged {
            self.work.v[0] = v;
            self.work.q.copy_from_slice(&self.q1);
            self.health.record(iterations, true, step, r);
        }
        converged
    }

    /// The R-type root's rows for the current rate.
    fn discretise(&mut self) -> Result<(), NetlistError> {
        // Under ADAA the trees' reactances are adapted for 1.5·T; a
        // prewarp adapts them for its design step.
        let fs = self.design_rate();
        self.forest.adapt(fs);
        let n = self.n;
        let p = self.ports.len();
        let (nu, nq) = (self.nu, self.nq);
        let mut x = self.static_matrix.clone();
        let stamp = |x: &mut [f64], a: Pin, b: Pin, g: f64| {
            for (r, sr) in [(a, 1.0), (b, -1.0)] {
                for (c, sc) in [(a, 1.0), (b, -1.0)] {
                    if let (Some(r), Some(c)) = (r, c) {
                        x[r * n + c] += sr * sc * g;
                    }
                }
            }
        };
        let g: Vec<f64> = self.ports.iter().map(|q| 1.0 / self.forest.cells[q.cell].ohms).collect();
        for (port, &g) in self.ports.iter().zip(&g) {
            stamp(&mut x, port.plus, port.minus, g);
        }
        // Nodes reduced away inside trees: decoupled, held at zero.
        for i in 0..n {
            if (0..n).all(|c| x[i * n + c] == 0.0 && x[c * n + i] == 0.0) {
                x[i * n + i] = 1.0;
            }
        }
        // The devices' columns: their output currents (Newton), or each
        // diode's Norton injection g·a (scattering).
        let diode_columns = if self.root == NonlinearRoot::Scattering {
            Some(self.scattering_matrix(&mut x)?)
        } else {
            None
        };
        let nn = diode_columns.as_ref().map_or(nq, Vec::len);
        // Columns: every port's β (Norton injection g·β), the inputs, the devices'.
        let cols = p + nu + nn;
        let mut rhs = vec![0.0; n * cols];
        for (k, port) in self.ports.iter().enumerate() {
            if let Some(r) = port.plus {
                rhs[r * cols + k] += g[k];
            }
            if let Some(r) = port.minus {
                rhs[r * cols + k] -= g[k];
            }
        }
        for r in 0..n {
            for k in 0..nu {
                rhs[r * cols + p + k] = self.bu[r * nu + k];
            }
        }
        match &diode_columns {
            None => {
                for r in 0..n {
                    for k in 0..nq {
                        rhs[r * cols + p + nu + k] = self.bq[r * nq + k];
                    }
                }
            }
            Some(columns) => {
                for (k, column) in columns.iter().enumerate() {
                    for &(pin, g) in column {
                        if let Some(r) = pin {
                            rhs[r * cols + p + nu + k] += g;
                        }
                    }
                }
            }
        }
        let m = solve_columns(&x, n, &rhs, cols).ok_or(NetlistError::Singular { analysis: "wave digital" })?;
        let across = |a: Pin, b: Pin, c: usize| read_col(&m, cols, a, c) - read_col(&m, cols, b, c);
        let cells = &self.forest.cells;
        self.reflective = (0..p).filter(|&k| cells[self.ports[k].cell].reflective).collect();
        self.stateful = (0..p).filter(|&k| cells[self.ports[k].cell].stateful).collect();
        self.reflective_cells = self.reflective.iter().map(|&k| self.ports[k].cell).collect();
        self.stateful_cells = self.stateful.iter().map(|&k| self.ports[k].cell).collect();
        let na = self.reflective.len();
        // Kept columns, as indices into the full `cols`.
        let kept: Vec<usize> = self.reflective.iter().copied().chain(p..cols).collect();
        let width = kept.len();
        self.scatter = Vec::with_capacity(self.stateful.len() * width);
        for &i in &self.stateful {
            let port = self.ports[i];
            for &c in &kept {
                let own = if c == i { 1.0 } else { 0.0 };
                self.scatter.push(2.0 * across(port.plus, port.minus, c) - own);
            }
        }
        self.probe_row = kept.iter().map(|&c| across(self.probe.0, self.probe.1, c)).collect();
        let nl = na + nu;
        // The devices' rows: their controls (Newton: `v = p + K·q`), or
        // the waves arriving at each diode port (scattering: `p + K·a`).
        let rows: Vec<(Pin, Pin, f64, Option<usize>)> = match &diode_columns {
            None => self.controls.iter().map(|&(a, b)| (a, b, 1.0, None)).collect(),
            Some(_) => self
                .scattering
                .iter()
                .enumerate()
                .map(|(j, d)| (d.plus, d.minus, 2.0, Some(p + nu + j)))
                .collect(),
        };
        self.predict = Vec::with_capacity(rows.len() * nl);
        self.k = Vec::with_capacity(rows.len() * nn);
        for &(a, b, scale, own) in &rows {
            for &c in &kept[..nl] {
                self.predict.push(scale * across(a, b, c));
            }
            for &c in &kept[nl..] {
                let own = if Some(c) == own { 1.0 } else { 0.0 };
                self.k.push(scale * across(a, b, c) - own);
            }
        }
        self.p = vec![0.0; rows.len()];
        self.z = vec![0.0; width];
        if diode_columns.is_none() {
            self.adaa.kernel.find_one_port(&self.bq, n, &self.k, &self.devices);
        }
        if !self.varying.leaves.is_empty() {
            let outputs: Vec<variable::Row> = self
                .stateful
                .iter()
                .map(|&i| variable::Row {
                    a: self.ports[i].plus,
                    b: self.ports[i].minus,
                    scale: 2.0,
                    own: Some(i),
                })
                .chain(std::iter::once(variable::Row {
                    a: self.probe.0,
                    b: self.probe.1,
                    scale: 1.0,
                    own: None,
                }))
                .chain(rows.iter().map(|&(a, b, scale, own)| variable::Row { a, b, scale, own }))
                .collect();
            self.prepare_variables(&x, &m, cols, &kept, &outputs)?;
        }
        Ok(())
    }

    /// How long the linearised circuit rings (the DK method's estimate on
    /// the same netlist: the same discretisation).
    pub fn tail(&self) -> Tail {
        self.operating.tail()
    }

    /// The source a response is taken from, and its scale: the driven
    /// one, else the deck's `AC` source.
    fn response_input(&self) -> Option<(usize, f64)> {
        match self.driven {
            Some(k) => Some((k, 1.0)),
            None => self.slots.iter().position(|s| s.ac != 0.0).map(|k| (k, self.slots[k].ac)),
        }
    }

    /// Replace the nonlinear root by its linearisation at the operating
    /// point (the Newton form's `q = Jq·v`, `v = p + K·q`, so
    /// `q = Jq·(I − K·Jq)⁻¹·p`), for reading a small-signal response.
    fn linearise(&mut self) {
        if self.root == NonlinearRoot::Scattering {
            let newton = self.set_nonlinear_root(NonlinearRoot::Newton);
            debug_assert!(newton.is_ok(), "the Newton root takes any netlist the scattering one does");
        }
        let (nv, nq) = (self.p.len(), self.nq);
        if self.uic {
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
        } else {
            self.work.v.copy_from_slice(&self.operating.dc_v);
        }
        self.work.evaluate(&mut self.devices, 0.0);
        let jq = self.work.jacobian_of_outputs().to_vec();
        // Mᵀ = (I − K·Jq)ᵀ; Lᵀ = M⁻ᵀ·Jqᵀ.
        let mut mt = vec![0.0; nv * nv];
        for i in 0..nv {
            for c in 0..nv {
                let kj: f64 = (0..nq).map(|j| self.k[i * nq + j] * jq[j * nv + c]).sum();
                mt[c * nv + i] = if i == c { 1.0 } else { 0.0 } - kj;
            }
        }
        let mut jqt = vec![0.0; nv * nq];
        for j in 0..nq {
            for i in 0..nv {
                jqt[i * nq + j] = jq[j * nv + i];
            }
        }
        let lt = solve_columns(&mt, nv, &jqt, nq).unwrap_or_else(|| vec![0.0; nv * nq]);
        let mut l = vec![0.0; nq * nv];
        for i in 0..nv {
            for j in 0..nq {
                l[j * nv + i] = lt[i * nq + j];
            }
        }
        self.linearised = Some(l);
    }

    /// The running filter's own state space, read by stepping it: from
    /// each unit state (and the unit input) one sample on, with every
    /// other source silent. `(A, B, C, D)` with `x' = A·x + B·u`,
    /// `y = C·x + D·u`, over the stored waves.
    fn measured_state_space(&self, input: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>, f64) {
        let states: Vec<usize> = (0..self.forest.cells.len())
            .filter(|&i| matches!(self.forest.cells[i].kind, Kind::Capacitor { .. } | Kind::Inductor { .. }))
            .collect();
        let nx = states.len();
        let mut probe = self.clone();
        // Under ADAA: the plain filter on the trees adapted for 1.5·T,
        // which `response_at` maps by Holters' eq. (13).
        probe.antialiasing = Antialiasing::None;
        if !probe.devices.is_empty() {
            probe.linearise();
        }
        let mut run = |x: Option<usize>, u: f64| -> (Vec<f64>, f64) {
            for (j, &s) in states.iter().enumerate() {
                probe.forest.cells[s].state = if Some(j) == x { 1.0 } else { 0.0 };
            }
            probe.values.iter_mut().for_each(|v| *v = 0.0);
            probe.values[input] = u;
            let y = probe.advance(0.0);
            (states.iter().map(|&s| probe.forest.cells[s].state).collect(), y)
        };
        let (mut a, mut c) = (vec![0.0; nx * nx], vec![0.0; nx]);
        for k in 0..nx {
            let (next, y) = run(Some(k), 0.0);
            for i in 0..nx {
                a[i * nx + k] = next[i];
            }
            c[k] = y;
        }
        let (b, d) = run(None, 1.0);
        (a, b, c, d)
    }

    /// The discrete response at `hz` from the input source (the driven
    /// one, else the deck's `AC` source, scaled by its magnitude) to the
    /// probe, read from the running filter itself — its devices
    /// linearised at the operating point. `None` with no input source.
    pub fn frequency_response(&self, hz: f64) -> Option<Phasor> {
        self.response_at(std::f64::consts::TAU * hz * self.step)
    }

    /// The analog response at `hz`: the discrete response at the
    /// frequency the bilinear transform maps there, exact for a linear
    /// circuit.
    pub fn analog_response(&self, hz: f64) -> Option<Phasor> {
        self.response_at(prewarp::digital_angle(hz, prewarp::design_step(self.step, self.prewarp)))
    }

    fn response_at(&self, angle: f64) -> Option<Phasor> {
        let (input, scale) = self.response_input()?;
        let (a, b, c, d) = self.measured_state_space(input);
        let nx = c.len();
        let mut zi = Phasor::new(angle.cos(), -angle.sin());
        let outer = if self.adaa_active() {
            let (inner, outer) = adaa::holters(zi);
            zi = inner;
            outer
        } else {
            Phasor::new(1.0, 0.0)
        };
        let mut m: Vec<Phasor> = (0..nx * nx)
            .map(|i| {
                let one = if i / nx == i % nx { 1.0 } else { 0.0 };
                Phasor::new(one, 0.0).sub(zi.mul(Phasor::new(a[i], 0.0)))
            })
            .collect();
        let mut x: Vec<Phasor> = b.iter().map(|&b| Phasor::new(b, 0.0)).collect();
        if !complex_solve(&mut m, nx, &mut x) {
            return None;
        }
        let cx = c
            .iter()
            .zip(&x)
            .fold(Phasor::default(), |s, (&c, &x)| s.add(Phasor::new(c, 0.0).mul(x)));
        let h = outer.mul(Phasor::new(d, 0.0).add(zi.mul(cx)));
        Some(Phasor::new(scale * h.re, scale * h.im))
    }
}

#[inline]
fn read_col(m: &[f64], cols: usize, p: Pin, c: usize) -> f64 {
    p.map_or(0.0, |r| m[r * cols + c])
}

#[cfg(test)]
mod tests;
