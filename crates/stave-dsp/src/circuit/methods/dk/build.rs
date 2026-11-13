//! From a [`Netlist`] to the MNA structure: unknowns numbered, the
//! rate-independent stamps made, sources and devices given their columns.

use super::devices::{Device, Nonlinear, SHUNT};
use super::solve::{Health, Settings, Workspace};
use super::{pin, Capacitor, Dk, Inductor, Input, Pin};
use crate::circuit::devices::opamp::{Amp, Macromodel};
use crate::circuit::netlist::expr::{Dual, Tape};
use crate::circuit::netlist::{ElementKind, Netlist, NetlistError, Probe};
use crate::kernel::node::Tail;

pub(super) struct Builder<'a> {
    netlist: &'a Netlist,
    n: usize,
    matrix: Vec<(usize, usize, f64)>,
    bu: Vec<(usize, usize, f64)>,
    bq: Vec<(usize, usize, f64)>,
    controls: Vec<(Pin, Pin)>,
    capacitors: Vec<Capacitor>,
    inductors: Vec<Inductor>,
    inputs: Vec<Input>,
    input_ac: Vec<f64>,
    source_names: Vec<String>,
    devices: Vec<Nonlinear>,
    nq: usize,
}

impl<'a> Builder<'a> {
    pub fn new(netlist: &'a Netlist) -> Self {
        Self {
            netlist,
            n: 0,
            matrix: Vec::new(),
            bu: Vec::new(),
            bq: Vec::new(),
            controls: Vec::new(),
            capacitors: Vec::new(),
            inductors: Vec::new(),
            inputs: Vec::new(),
            input_ac: Vec::new(),
            source_names: Vec::new(),
            devices: Vec::new(),
            nq: 0,
        }
    }

    fn add(&mut self, r: Pin, c: Pin, x: f64) {
        if let (Some(r), Some(c)) = (r, c) {
            self.matrix.push((r, c, x));
        }
    }

    fn conductance(&mut self, a: Pin, b: Pin, g: f64) {
        self.add(a, a, g);
        self.add(b, b, g);
        self.add(a, b, -g);
        self.add(b, a, -g);
    }

    /// A branch current `row` from `a` to `b`, and its row's v(a) − v(b).
    fn branch(&mut self, a: Pin, b: Pin, row: usize) {
        self.add(a, Some(row), 1.0);
        self.add(b, Some(row), -1.0);
        self.add(Some(row), a, 1.0);
        self.add(Some(row), b, -1.0);
    }

    fn input(&mut self, name: &str, input: Input, ac: f64, column: &[(Pin, f64)]) {
        let k = self.inputs.len();
        for &(row, x) in column {
            if let Some(row) = row {
                self.bu.push((row, k, x));
            }
        }
        self.inputs.push(input);
        self.input_ac.push(ac);
        self.source_names.push(name.to_string());
    }

    /// The global control for v(a, b), shared with v(b, a) by sign.
    fn control(&mut self, a: Pin, b: Pin) -> (usize, f64) {
        if let Some(i) = self.controls.iter().position(|&c| c == (a, b)) {
            return (i, 1.0);
        }
        if let Some(i) = self.controls.iter().position(|&c| c == (b, a)) {
            return (i, -1.0);
        }
        self.controls.push((a, b));
        (self.controls.len() - 1, 1.0)
    }

    /// A device terminal behind `ohms` of ohmic resistance: the internal
    /// node, joined to the external pin; the pin itself without one.
    fn terminal(&mut self, external: Pin, internal: Option<usize>, ohms: f64) -> Pin {
        match internal {
            Some(node) => {
                self.conductance(external, Some(node), 1.0 / ohms);
                Some(node)
            }
            None => external,
        }
    }

    fn device(&mut self, device: Device, controls: &[(Pin, Pin)], outputs: &[&[(Pin, f64)]]) {
        let controls = controls.iter().map(|&(a, b)| self.control(a, b)).collect();
        let output = self.nq;
        for (o, column) in outputs.iter().enumerate() {
            for &(row, x) in column.iter() {
                if let Some(row) = row {
                    self.bq.push((row, output + o, x));
                }
            }
        }
        self.nq += outputs.len();
        self.devices.push(Nonlinear {
            device,
            controls,
            output,
        });
    }

    fn expression(name: &str, expression: &crate::circuit::netlist::Expr) -> Result<Device, NetlistError> {
        let tape = Tape::compile(expression).ok_or_else(|| NetlistError::Unsupported {
            element: name.to_string(),
            reason: "reads more than four node voltages".into(),
        })?;
        let stack = vec![Dual::default(); tape.depth().max(1)];
        Ok(Device::Expression { tape, stack })
    }

    pub fn build(mut self, probe: Probe, settings: Settings) -> Result<Dk, NetlistError> {
        let netlist = self.netlist;
        let elements = netlist.elements();
        let nodes = netlist.node_count() - 1;
        // Internal nodes (behind a device's ohmic resistances, a
        // macromodel's gain node), then branch rows.
        let mut internal = vec![[None; 3]; elements.len()];
        let mut next = nodes;
        for (i, e) in elements.iter().enumerate() {
            for (k, needs) in internal_nodes(&e.kind).into_iter().enumerate() {
                if needs {
                    internal[i][k] = Some(next);
                    next += 1;
                }
            }
        }
        let node_total = next;
        let mut row = vec![0; elements.len()];
        for (i, e) in elements.iter().enumerate() {
            if matches!(
                e.kind,
                ElementKind::VoltageSource { .. }
                    | ElementKind::VoltageControlledVoltage { .. }
                    | ElementKind::BehaviouralVoltage { .. }
                    | ElementKind::Inductor { .. }
                    | ElementKind::OpAmp { .. }
            ) {
                row[i] = next;
                next += 1;
            }
        }
        self.n = next;
        for (i, e) in elements.iter().enumerate() {
            let name = e.name.as_str();
            match &e.kind {
                ElementKind::Resistor { a, b, ohms } => {
                    if *ohms == 0.0 || !ohms.is_finite() {
                        return Err(unsupported(name, "a resistor needs finite, nonzero ohms"));
                    }
                    self.conductance(pin(*a), pin(*b), 1.0 / ohms);
                }
                ElementKind::Capacitor {
                    a,
                    b,
                    farads,
                    initial_volts,
                } => self.capacitors.push(Capacitor {
                    a: pin(*a),
                    b: pin(*b),
                    farads: *farads,
                    initial_volts: initial_volts.unwrap_or(0.0),
                }),
                ElementKind::Inductor {
                    a,
                    b,
                    henries,
                    initial_amps,
                } => {
                    self.branch(pin(*a), pin(*b), row[i]);
                    self.inductors.push(Inductor {
                        row: row[i],
                        henries: *henries,
                        initial_amps: initial_amps.unwrap_or(0.0),
                    });
                }
                ElementKind::VoltageSource { a, b, source } => {
                    self.branch(pin(*a), pin(*b), row[i]);
                    let column = [(Some(row[i]), 1.0)];
                    self.input(name, Input::Source(source.clone()), source.ac, &column);
                }
                ElementKind::CurrentSource { a, b, source } => {
                    let column = [(pin(*a), -1.0), (pin(*b), 1.0)];
                    self.input(name, Input::Source(source.clone()), source.ac, &column);
                }
                ElementKind::VoltageControlledVoltage {
                    a,
                    b,
                    control,
                    gain,
                } => {
                    let r = Some(row[i]);
                    self.branch(pin(*a), pin(*b), row[i]);
                    self.add(r, pin(control.0), -gain);
                    self.add(r, pin(control.1), *gain);
                }
                ElementKind::CurrentControlledCurrent {
                    a,
                    b,
                    control,
                    gain,
                } => {
                    let c = control.to_ascii_lowercase();
                    let j = elements
                        .iter()
                        .position(|e| e.name == c && matches!(e.kind, ElementKind::VoltageSource { .. }))
                        .ok_or(NetlistError::UnknownElement { name: c })?;
                    self.add(pin(*a), Some(row[j]), *gain);
                    self.add(pin(*b), Some(row[j]), -gain);
                }
                ElementKind::VoltageControlledCurrent {
                    a,
                    b,
                    control,
                    gain,
                } => {
                    let (a, b, c, d) = (pin(*a), pin(*b), pin(control.0), pin(control.1));
                    self.add(a, c, *gain);
                    self.add(a, d, -gain);
                    self.add(b, c, -gain);
                    self.add(b, d, *gain);
                }
                ElementKind::Diode {
                    anode,
                    cathode,
                    diode,
                } => {
                    let junction = self.terminal(pin(*anode), internal[i][0], diode.series_ohms);
                    let k = pin(*cathode);
                    self.conductance(junction, k, SHUNT);
                    self.device(
                        Device::Diode(diode.junction()),
                        &[(junction, k)],
                        &[&[(junction, -1.0), (k, 1.0)]],
                    );
                }
                ElementKind::Triode {
                    plate,
                    grid,
                    cathode,
                    tube,
                } => {
                    let (p, g, k) = (pin(*plate), pin(*grid), pin(*cathode));
                    self.device(
                        Device::Triode(*tube),
                        &[(g, k), (p, k)],
                        &[&[(p, -1.0), (k, 1.0)], &[(g, -1.0), (k, 1.0)]],
                    );
                }
                ElementKind::Pentode {
                    plate,
                    screen,
                    grid,
                    cathode,
                    tube,
                } => {
                    let (p, s, g, k) = (pin(*plate), pin(*screen), pin(*grid), pin(*cathode));
                    self.device(
                        Device::Pentode(*tube),
                        &[(g, k), (s, k), (p, k)],
                        &[&[(p, -1.0), (k, 1.0)], &[(s, -1.0), (k, 1.0)]],
                    );
                }
                ElementKind::OpAmp {
                    plus,
                    minus,
                    output,
                    amp,
                    supply,
                } => {
                    let r = Some(row[i]);
                    let (plus, minus, out) = (pin(*plus), pin(*minus), pin(*output));
                    // The output: a branch current into `output` from ground.
                    self.add(out, r, 1.0);
                    match amp {
                        // The nullor: v(+) − v(−) = 0.
                        Amp::Ideal(_) => {
                            self.add(r, plus, 1.0);
                            self.add(r, minus, -1.0);
                        }
                        // A unity buffer of the gain node.
                        Amp::Macromodel(m) => {
                            let n = internal[i][0];
                            self.add(r, out, 1.0);
                            self.add(r, n, -1.0);
                            let stage = m.prepare(supply.0, supply.1);
                            self.conductance(n, None, stage.leak);
                            self.capacitors.push(Capacitor {
                                a: n,
                                b: None,
                                farads: Macromodel::COMPENSATION_FARADS,
                                initial_volts: 0.0,
                            });
                            self.device(Device::OpAmp(stage), &[(plus, minus), (n, None)], &[&[(n, 1.0)]]);
                        }
                    }
                }
                ElementKind::Ota {
                    plus,
                    minus,
                    output,
                    bias,
                    negative,
                    ota,
                } => {
                    let (plus, minus, out) = (pin(*plus), pin(*minus), pin(*output));
                    let (b, neg) = (pin(*bias), pin(*negative));
                    self.conductance(b, neg, SHUNT);
                    self.device(
                        Device::Ota(*ota),
                        &[(plus, minus), (b, neg)],
                        &[&[(out, 1.0)], &[(b, -1.0), (neg, 1.0)]],
                    );
                }
                ElementKind::Bjt {
                    collector,
                    base,
                    emitter,
                    bjt,
                } => {
                    let c = self.terminal(pin(*collector), internal[i][0], bjt.collector_ohms);
                    let b = self.terminal(pin(*base), internal[i][1], bjt.base_ohms);
                    let e = self.terminal(pin(*emitter), internal[i][2], bjt.emitter_ohms);
                    self.conductance(b, e, SHUNT);
                    self.conductance(b, c, SHUNT);
                    self.device(
                        Device::Bjt(*bjt),
                        &[(b, e), (b, c)],
                        &[&[(c, -1.0), (e, 1.0)], &[(b, -1.0), (e, 1.0)]],
                    );
                }
                ElementKind::Jfet {
                    drain,
                    gate,
                    source,
                    jfet,
                } => {
                    let d = self.terminal(pin(*drain), internal[i][0], jfet.drain_ohms);
                    let g = pin(*gate);
                    let s = self.terminal(pin(*source), internal[i][2], jfet.source_ohms);
                    self.conductance(g, s, SHUNT);
                    self.conductance(g, d, SHUNT);
                    self.device(
                        Device::Jfet(*jfet),
                        &[(g, s), (g, d)],
                        &[&[(d, -1.0), (s, 1.0)], &[(g, -1.0), (s, 1.0)]],
                    );
                }
                ElementKind::Mosfet {
                    drain,
                    gate,
                    source,
                    bulk,
                    mosfet,
                } => {
                    let d = self.terminal(pin(*drain), internal[i][0], mosfet.drain_ohms);
                    let g = pin(*gate);
                    let s = self.terminal(pin(*source), internal[i][2], mosfet.source_ohms);
                    let b = pin(*bulk);
                    // vbs is read at the intrinsic source, as SPICE does:
                    // a bulk tied to the source still sees −Id·RS.
                    let tied = b == s;
                    let device = Device::Mosfet {
                        mosfet: *mosfet,
                        bulk: !tied,
                    };
                    let column: &[(Pin, f64)] = &[(d, -1.0), (s, 1.0)];
                    if tied {
                        self.device(device, &[(g, s), (d, s)], &[column]);
                    } else {
                        self.device(device, &[(g, s), (d, s), (b, s)], &[column]);
                    }
                }
                ElementKind::BehaviouralCurrent { a, b, expression } => {
                    let (a, b) = (pin(*a), pin(*b));
                    let device = Self::expression(name, expression)?;
                    let column = [(a, -1.0), (b, 1.0)];
                    if expression.controls().is_empty() {
                        let Device::Expression { tape, stack } = device else { unreachable!() };
                        self.input(name, Input::Expression { tape, stack }, 0.0, &column);
                    } else {
                        let controls = control_pins(expression);
                        self.device(device, &controls, &[&column]);
                    }
                }
                ElementKind::BehaviouralVoltage { a, b, expression } => {
                    self.branch(pin(*a), pin(*b), row[i]);
                    let device = Self::expression(name, expression)?;
                    let column = [(Some(row[i]), 1.0)];
                    if expression.controls().is_empty() {
                        let Device::Expression { tape, stack } = device else { unreachable!() };
                        self.input(name, Input::Expression { tape, stack }, 0.0, &column);
                    } else {
                        let controls = control_pins(expression);
                        self.device(device, &controls, &[&column]);
                    }
                }
            }
        }
        let n = self.n;
        let (nu, nq, nv) = (self.inputs.len(), self.nq, self.controls.len());
        let dense = |triplets: &[(usize, usize, f64)], cols: usize| {
            let mut m = vec![0.0; n * cols];
            for &(r, c, x) in triplets {
                m[r * cols + c] += x;
            }
            m
        };
        let nx = self.capacitors.len() + self.inductors.len();
        Ok(Dk {
            n,
            nodes: node_total,
            static_matrix: dense(&self.matrix, n),
            bu: dense(&self.bu, nu),
            bq: dense(&self.bq, nq),
            controls: self.controls,
            capacitors: self.capacitors,
            inductors: self.inductors,
            inputs: self.inputs,
            input_ac: self.input_ac,
            devices: self.devices,
            nq,
            probe: (pin(probe.plus), pin(probe.minus)),
            driven: None,
            uic: netlist.tran.is_some_and(|t| t.uic),
            source_names: self.source_names,
            dc_w: vec![0.0; n],
            dc_v: vec![0.0; nv],
            sample_rate: 48_000.0,
            step: 1.0 / 48_000.0,
            dx: vec![0.0; nv * nx],
            du: vec![0.0; nv * nu],
            k: vec![0.0; nv * nq],
            ax: vec![0.0; nx * nx],
            au: vec![0.0; nx * nu],
            aq: vec![0.0; nx * nq],
            ex: vec![0.0; nx],
            eu: vec![0.0; nu],
            eq: vec![0.0; nq],
            tail: Tail::None,
            x: vec![0.0; nx],
            x_next: vec![0.0; nx],
            u: vec![0.0; nu],
            p: vec![0.0; nv],
            work: Workspace::new(nv, nq),
            samples: 0,
            output: 0.0,
            settings,
            health: Health::default(),
        })
    }
}

/// Which of an element's internal node slots it needs: behind a diode's
/// series resistance (anode), a BJT's (collector, base, emitter) or a
/// FET's (drain, —, source) ohmic resistances when positive, and a
/// macromodel op-amp's gain node.
fn internal_nodes(kind: &ElementKind) -> [bool; 3] {
    let ohmic = |r: [f64; 3]| r.map(|ohms| ohms > 0.0);
    match kind {
        ElementKind::Diode { diode, .. } => ohmic([diode.series_ohms, 0.0, 0.0]),
        ElementKind::OpAmp { amp, .. } => [matches!(amp, Amp::Macromodel(_)), false, false],
        ElementKind::Bjt { bjt, .. } => ohmic([bjt.collector_ohms, bjt.base_ohms, bjt.emitter_ohms]),
        ElementKind::Jfet { jfet, .. } => ohmic([jfet.drain_ohms, 0.0, jfet.source_ohms]),
        ElementKind::Mosfet { mosfet, .. } => ohmic([mosfet.drain_ohms, 0.0, mosfet.source_ohms]),
        _ => [false; 3],
    }
}

/// An expression's controls as MNA pins, in its tape's order.
fn control_pins(expression: &crate::circuit::netlist::Expr) -> Vec<(Pin, Pin)> {
    expression
        .controls()
        .into_iter()
        .map(|(a, b)| (pin(a), pin(b)))
        .collect()
}

fn unsupported(element: &str, reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: element.to_string(),
        reason: reason.to_string(),
    }
}
