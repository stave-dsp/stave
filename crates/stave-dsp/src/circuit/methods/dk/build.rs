//! From a [`Netlist`] to the MNA structure: unknowns numbered, the
//! rate-independent stamps made, sources and devices given their columns.

use super::devices::{Device, Nonlinear};
use super::solve::{Health, Settings, Workspace};
use super::{pin, Capacitor, Dk, Inductor, Input, Pin};
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
        // Internal nodes (a diode's series resistance), then branch rows.
        let mut internal = vec![None; elements.len()];
        let mut next = nodes;
        for (i, e) in elements.iter().enumerate() {
            if let ElementKind::Diode { series_ohms, .. } = e.kind {
                if series_ohms > 0.0 {
                    internal[i] = Some(next);
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
                ElementKind::Diode {
                    anode,
                    cathode,
                    diode,
                    series_ohms,
                } => {
                    let junction = match internal[i] {
                        Some(node) => {
                            self.conductance(pin(*anode), Some(node), 1.0 / series_ohms);
                            Some(node)
                        }
                        None => pin(*anode),
                    };
                    let k = pin(*cathode);
                    self.device(Device::Diode(*diode), &[(junction, k)], &[&[(junction, -1.0), (k, 1.0)]]);
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
