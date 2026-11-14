//! Variable parts in a wave digital filter (N6): a part's leaf takes its
//! new port resistance, its tree re-adapts bottom-up, and the R-type
//! root's scattering rows follow the tree's new port conductance by the
//! Woodbury identity ([`crate::circuit::variable`] has the design).
//!
//! A tree's port `p` stamps `g_p·(e₊ − e₋)(e₊ − e₋)ᵀ` into the root's
//! nodal matrix and injects `g_p·β_p` through the same pins, so a new
//! `g_p` moves both the matrix (`E·Δ·Eᵀ`) and the right-hand side
//! (`E·Δ·F`, `F` picking `β_p`'s column): the update's `W` is
//! `F − Eᵀ·M₀`. Its rows are the root's outputs: the stateful ports'
//! scattering, the probe, and the devices' controls (or the diode ports'
//! arriving waves, under the scattering iterative method).

use super::{Kind, NetlistError, Pin, Wdf};
use crate::circuit::methods::prewarp;
use crate::circuit::methods::woodbury::Woodbury;
use crate::circuit::netlist::{Netlist, Variable};
use crate::circuit::variable::{BranchKind, Pot, Smoothing, Switch, VariableCapacitor, VariableResistor, Variables};
use crate::units::{Farads, Ohms};

/// A wave digital filter's variable parts and their prepared update.
#[derive(Clone, Debug, Default)]
pub(crate) struct Varying {
    pub variables: Variables,
    /// Each branch's leaf cell.
    pub leaves: Vec<usize>,
    /// The root ports whose trees hold a variable leaf.
    pub ports: Vec<usize>,
    /// Their conductances when the root was prepared.
    base: Vec<f64>,
    delta: Vec<f64>,
    woodbury: Option<Woodbury>,
    /// Each output row's own column among the kept ones.
    own: Vec<Option<usize>>,
}

/// One output row of the root over its kept columns: `scale·v(a, b)`,
/// less one in the column `own` (a port's own incident wave).
pub(super) struct Row {
    pub a: Pin,
    pub b: Pin,
    pub scale: f64,
    pub own: Option<usize>,
}

fn across(m: &[f64], cols: usize, a: Pin, b: Pin, c: usize) -> f64 {
    a.map_or(0.0, |r| m[r * cols + c]) - b.map_or(0.0, |r| m[r * cols + c])
}

impl Varying {
    /// The netlist's variables on the leaves `leaf_of` (element index,
    /// cell) built; a pot or rheostat must keep a positive resistance.
    pub fn new(netlist: &Netlist, leaf_of: &[(usize, usize)]) -> Result<Self, NetlistError> {
        for v in netlist.variables() {
            if let Variable::Pot { end_ohms, .. } | Variable::Rheostat { end_ohms, .. } = v {
                if *end_ohms <= 0.0 {
                    return Err(NetlistError::Unsupported {
                        element: v.name().to_string(),
                        reason: "a wave port's resistance must stay positive at every position: \
                                 give the pot an end resistance (Variable::with_end_ohms)"
                            .into(),
                    });
                }
            }
        }
        let variables = Variables::new(netlist)?;
        let elements = netlist.elements();
        let leaves = variables
            .branches
            .iter()
            .map(|b| {
                leaf_of
                    .iter()
                    .find(|&&(i, _)| elements[i].name == b.element)
                    .map(|&(_, cell)| cell)
                    .ok_or_else(|| NetlistError::UnknownElement {
                        name: b.element.clone(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            variables,
            leaves,
            ..Self::default()
        })
    }
}

impl Wdf {
    /// The pot or rheostat called `name`.
    pub fn pot(&self, name: &str) -> Result<Pot, NetlistError> {
        self.varying.variables.find_pot(name)
    }

    /// The variable resistor on the element called `name`.
    pub fn variable_resistor(&self, name: &str) -> Result<VariableResistor, NetlistError> {
        self.varying.variables.find_resistor(name)
    }

    /// The variable capacitor on the element called `name`.
    pub fn variable_capacitor(&self, name: &str) -> Result<VariableCapacitor, NetlistError> {
        self.varying.variables.find_capacitor(name)
    }

    /// The switch on the element called `name` (H6).
    pub fn switch(&self, name: &str) -> Result<Switch, NetlistError> {
        self.varying.variables.find_switch(name)
    }

    /// Throw `switch` closed or open (as [`Dk::set_closed`](crate::circuit::methods::dk::Dk::set_closed)).
    /// Realtime.
    pub fn set_closed(&mut self, switch: Switch, closed: bool) {
        let value = if closed { 1.0 } else { 0.0 };
        self.varying.variables.set(switch.0, value);
        self.operating.varying.variables.set(switch.0, value);
    }

    /// Turn `pot` to `position` (0 to 1), gliding. Realtime.
    pub fn set_position(&mut self, pot: Pot, position: f64) {
        self.varying.variables.set(pot.0, position);
        self.operating.varying.variables.set(pot.0, position);
    }

    /// Set a variable resistor (positive: a wave port's resistance),
    /// gliding. Realtime.
    pub fn set_ohms(&mut self, resistor: VariableResistor, ohms: Ohms) {
        debug_assert!(ohms.get() > 0.0, "a wave port's resistance is positive");
        self.varying.variables.set(resistor.0, ohms.get());
        self.operating.varying.variables.set(resistor.0, ohms.get());
    }

    /// Set a variable capacitor, gliding; its voltage and current carry
    /// over. Realtime.
    pub fn set_farads(&mut self, capacitor: VariableCapacitor, farads: Farads) {
        debug_assert!(farads.get() > 0.0, "a wave port's capacitance is positive");
        self.varying.variables.set(capacitor.0, farads.get());
        self.operating.varying.variables.set(capacitor.0, farads.get());
    }

    /// `pot`'s position in effect.
    pub fn position(&self, pot: Pot) -> f64 {
        self.varying.variables.current(pot.0)
    }

    /// How set values are reached ([`Smoothing::DEFAULT`] until set).
    pub fn set_smoothing(&mut self, smoothing: Smoothing) {
        self.varying.variables.set_smoothing(smoothing, self.sample_rate);
    }

    /// Finish every glide now and refresh the root (realtime).
    pub fn settle(&mut self) {
        if self.varying.variables.settle() {
            self.refresh();
        }
    }

    #[inline]
    pub(super) fn glide(&mut self) {
        if self.varying.variables.advance() {
            self.refresh();
        }
    }

    /// The branch values into the leaves, before a full discretisation
    /// (prepare: no state carried); the operating copy follows.
    pub(super) fn adopt_values(&mut self) {
        self.varying.variables.settle();
        for j in 0..self.varying.leaves.len() {
            self.put_leaf(j, false);
        }
        self.operating.varying.variables.mirror(&self.varying.variables);
    }

    /// Branch `j`'s value into its leaf; a capacitor's voltage and
    /// current carried over when `carry`.
    fn put_leaf(&mut self, j: usize, carry: bool) {
        let cell = self.varying.leaves[j];
        let value = self.varying.variables.values[j];
        let fs = self.design_rate();
        let samples = self.samples;
        let c = &mut self.forest.cells[cell];
        match (self.varying.variables.branches[j].kind, &mut c.kind) {
            (BranchKind::Resistor, _) => c.ohms = value,
            (BranchKind::Capacitor, Kind::Capacitor { farads, .. }) => {
                if *farads != value {
                    *farads = value;
                    if carry {
                        // v = (a + b)/2, i = (a − b)/(2R); the next wave is v + R'·i.
                        let new = 1.0 / (2.0 * value * fs);
                        let (volts, amps) = if samples == 0 {
                            (c.state, 0.0)
                        } else {
                            ((c.state + c.beta) / 2.0, (c.state - c.beta) / (2.0 * c.ohms))
                        };
                        c.state = volts + new * amps;
                    }
                }
            }
            (BranchKind::Capacitor, _) => {}
        }
    }

    /// At discretise: the update's matrices around the root matrix `x`
    /// and `m = X⁻¹·rhs` (`cols` wide), for the output `rows` over the
    /// `kept` columns.
    pub(super) fn prepare_variables(
        &mut self,
        x: &[f64],
        m: &[f64],
        cols: usize,
        kept: &[usize],
        rows: &[Row],
    ) -> Result<(), NetlistError> {
        let n = self.n;
        if self.varying.ports.is_empty() {
            let mut ports = Vec::new();
            for (p, port) in self.ports.iter().enumerate() {
                let under = self.forest.subtree(port.cell);
                if self.varying.leaves.iter().any(|leaf| under.contains(leaf)) {
                    ports.push(p);
                }
            }
            self.varying.ports = ports;
        }
        let k = self.varying.ports.len();
        let mut e = vec![0.0; n * k];
        for (j, &p) in self.varying.ports.iter().enumerate() {
            let port = self.ports[p];
            if let Some(r) = port.plus {
                e[r * k + j] += 1.0;
            }
            if let Some(r) = port.minus {
                e[r * k + j] -= 1.0;
            }
        }
        let y = crate::circuit::methods::dk::matrix::solve_columns(x, n, &e, k)
            .ok_or(NetlistError::Singular { analysis: "wave digital" })?;
        let width = kept.len();
        let mut q = vec![0.0; k * k];
        let mut w = vec![0.0; k * width];
        for (i, &p) in self.varying.ports.iter().enumerate() {
            let port = self.ports[p];
            for j in 0..k {
                q[i * k + j] = across(&y, k, port.plus, port.minus, j);
            }
            for (c, &col) in kept.iter().enumerate() {
                let picked = if col == p { 1.0 } else { 0.0 };
                w[i * width + c] = picked - across(m, cols, port.plus, port.minus, col);
            }
        }
        let mut u = vec![0.0; rows.len() * k];
        let mut base = vec![0.0; rows.len() * width];
        for (r, row) in rows.iter().enumerate() {
            for j in 0..k {
                u[r * k + j] = row.scale * across(&y, k, row.a, row.b, j);
            }
            for (c, &col) in kept.iter().enumerate() {
                base[r * width + c] = row.scale * across(m, cols, row.a, row.b, col);
            }
        }
        self.varying.woodbury = Some(Woodbury::new(k, rows.len(), width, q, u, w, base));
        self.varying.base = self.port_conductances();
        self.varying.delta = vec![0.0; k];
        self.varying.own = rows.iter().map(|r| r.own.and_then(|o| kept.iter().position(|&c| c == o))).collect();
        Ok(())
    }

    /// The rate the trees are adapted at (1.5·T's under ADAA, prewarped,
    /// as `discretise` adapts them).
    pub(super) fn design_rate(&self) -> f64 {
        let scale = if self.adaa_active() {
            self.antialiasing.design_step_scale()
        } else {
            1.0
        };
        1.0 / prewarp::design_step(scale / f64::from(self.sample_rate), self.prewarp)
    }

    fn port_conductances(&self) -> Vec<f64> {
        self.varying
            .ports
            .iter()
            .map(|&p| 1.0 / self.forest.cells[self.ports[p].cell].ohms)
            .collect()
    }

    /// The leaves at the values in effect, their trees re-adapted, and
    /// the root's rows by one low-rank update. Realtime.
    pub(crate) fn refresh(&mut self) {
        if self.varying.woodbury.is_none() {
            return;
        }
        for j in 0..self.varying.leaves.len() {
            self.put_leaf(j, true);
        }
        self.forest.resize(self.design_rate());
        for (i, &p) in self.varying.ports.iter().enumerate() {
            let g = 1.0 / self.forest.cells[self.ports[p].cell].ohms;
            self.varying.delta[i] = g - self.varying.base[i];
        }
        let Some(woodbury) = self.varying.woodbury.as_mut() else {
            return;
        };
        if !woodbury.update(&self.varying.delta) {
            return;
        }
        let o = &woodbury.out;
        let width = self.z.len();
        let (ns, nl) = (self.stateful.len(), self.reflective.len() + self.nu);
        let nn = width - nl;
        // Rows: the stateful ports' scattering, the probe, the devices'.
        for i in 0..ns {
            let own = self.varying.own[i];
            for c in 0..width {
                let one = if Some(c) == own { 1.0 } else { 0.0 };
                self.scatter[i * width + c] = o[i * width + c] - one;
            }
        }
        self.probe_row.copy_from_slice(&o[ns * width..(ns + 1) * width]);
        for i in 0..self.p.len() {
            let r = ns + 1 + i;
            let own = self.varying.own[r];
            self.predict[i * nl..(i + 1) * nl].copy_from_slice(&o[r * width..r * width + nl]);
            for c in nl..width {
                let one = if Some(c) == own { 1.0 } else { 0.0 };
                self.k[i * nn + c - nl] = o[r * width + c] - one;
            }
        }
        self.work.recheck(&self.k);
    }

    /// The root's rows and the forest's resistances, for comparing an
    /// updated filter with a freshly built one.
    #[cfg(test)]
    pub(crate) fn matrices(&self) -> Vec<Vec<f64>> {
        let cells = self.forest.cells.iter().flat_map(|c| [c.ohms, c.share]).collect();
        vec![self.scatter.clone(), self.probe_row.clone(), self.predict.clone(), self.k.clone(), cells]
    }
}
