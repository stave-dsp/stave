//! Variable parts in the DK method (N6): each refresh corrects the
//! prepared state-space matrices by the Woodbury identity
//! ([`crate::circuit::variable`] has the design).
//!
//! A variable resistor is an MNA branch (`v(a) − v(b) − R·i = 0`): a
//! new `R` is the one entry `S[r][r] = −R`, so `E` is the unit column
//! `e_r` and `Δ = −(R − R₀)`. A variable capacitor's companion `2C/T`
//! is the stamp `(e_a − e_b)·(e_a − e_b)ᵀ`, `Δ = 2(C − C₀)/T`, and its
//! state rows scale by `2·(2C/T)`. The rows corrected are every reduced
//! matrix's: the controls (`Dx`, `Du`, `K`), the states (`Ax`, `Au`,
//! `Aq`) and the probe (`Ex`, `Eu`, `Eq`), all of them differences of
//! rows of `M = S⁻¹·[x | u | q]`.

use super::{Dk, NetlistError, Pin};
use crate::circuit::methods::woodbury::Woodbury;
use crate::circuit::variable::{Pot, Smoothing, Switch, VariableCapacitor, VariableResistor, Variables};
use crate::units::{Farads, Ohms};

/// Where a variable branch sits in the MNA system.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stamp {
    /// A resistor's branch row.
    Row(usize),
    /// A capacitor, by its index in `Dk::capacitors`.
    Capacitor(usize),
}

/// The DK method's variable parts and their prepared update.
#[derive(Clone, Debug, Default)]
pub(crate) struct Varying {
    pub variables: Variables,
    pub stamps: Vec<Stamp>,
    /// Each branch's value when the matrices were prepared.
    base: Vec<f64>,
    delta: Vec<f64>,
    woodbury: Option<Woodbury>,
    /// A resistor moved since the operating point was solved.
    pub stale: bool,
}

impl Varying {
    pub fn new(variables: Variables, stamps: Vec<Stamp>) -> Self {
        Self {
            variables,
            stamps,
            ..Self::default()
        }
    }
}

/// Row `M[a] − M[b]` of an `n`×`cols` matrix, at column `c`.
fn across(m: &[f64], cols: usize, a: Pin, b: Pin, c: usize) -> f64 {
    a.map_or(0.0, |r| m[r * cols + c]) - b.map_or(0.0, |r| m[r * cols + c])
}

impl Dk {
    /// The pot or rheostat called `name` (its [`Variable`](crate::circuit::netlist::Variable)
    /// declaration's name).
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

    /// Throw `switch` closed or open: at once, or over its ramp, its
    /// contacts bouncing as it closes if it was declared so. Realtime.
    pub fn set_closed(&mut self, switch: Switch, closed: bool) {
        self.varying.variables.set(switch.0, if closed { 1.0 } else { 0.0 });
    }

    /// Turn `pot` to `position` (0 to 1), gliding as
    /// [`set_smoothing`](Self::set_smoothing) says. Realtime.
    pub fn set_position(&mut self, pot: Pot, position: f64) {
        self.varying.variables.set(pot.0, position);
    }

    /// Set a variable resistor, gliding. Realtime.
    pub fn set_ohms(&mut self, resistor: VariableResistor, ohms: Ohms) {
        self.varying.variables.set(resistor.0, ohms.get());
    }

    /// Set a variable capacitor, gliding; its voltage and current carry
    /// over (charge does not). Realtime.
    pub fn set_farads(&mut self, capacitor: VariableCapacitor, farads: Farads) {
        self.varying.variables.set(capacitor.0, farads.get());
    }

    /// `pot`'s position in effect (mid-glide, the glide's).
    pub fn position(&self, pot: Pot) -> f64 {
        self.varying.variables.current(pot.0)
    }

    /// How set values are reached ([`Smoothing::DEFAULT`] until set).
    pub fn set_smoothing(&mut self, smoothing: Smoothing) {
        self.varying.variables.set_smoothing(smoothing, self.sample_rate);
    }

    /// Finish every glide now and refresh the matrices (realtime: the
    /// same low-rank update a glide makes).
    pub fn settle(&mut self) {
        if self.varying.variables.settle() {
            self.refresh();
        }
    }

    /// One sample of glide, refreshing when it is time.
    #[inline]
    pub(super) fn glide(&mut self) {
        if self.varying.variables.advance() {
            self.refresh();
        }
    }

    /// The branch values into the structure a full discretisation reads:
    /// the static matrix's branch entries and the capacitors' farads.
    /// Off the audio path (prepare), so no state is carried.
    pub(super) fn adopt_values(&mut self) {
        self.varying.variables.settle();
        let n = self.n;
        for (j, &stamp) in self.varying.stamps.iter().enumerate() {
            let value = self.varying.variables.values[j];
            match stamp {
                Stamp::Row(r) => {
                    if self.static_matrix[r * n + r] != -value {
                        self.static_matrix[r * n + r] = -value;
                        self.varying.stale = true;
                    }
                }
                Stamp::Capacitor(c) => self.capacitors[c].farads = value,
            }
        }
    }

    /// At discretise: the update's matrices around `s` (the system just
    /// solved, its branches at their current values) and `m = S⁻¹·rhs`.
    pub(super) fn prepare_variables(&mut self, s: Vec<f64>, m: &[f64], cols: usize) -> Result<(), NetlistError> {
        let (n, k) = (self.n, self.varying.stamps.len());
        let mut e = vec![0.0; n * k];
        for (j, &stamp) in self.varying.stamps.iter().enumerate() {
            match stamp {
                Stamp::Row(r) => e[r * k + j] = 1.0,
                Stamp::Capacitor(c) => {
                    let cap = &self.capacitors[c];
                    if let Some(a) = cap.a {
                        e[a * k + j] += 1.0;
                    }
                    if let Some(b) = cap.b {
                        e[b * k + j] -= 1.0;
                    }
                }
            }
        }
        let y = self.solve_system(s, &e, k, "transient")?;
        let pick = |mat: &[f64], width: usize, stamp: Stamp, c: usize| match stamp {
            Stamp::Row(r) => mat[r * width + c],
            Stamp::Capacitor(i) => across(mat, width, self.capacitors[i].a, self.capacitors[i].b, c),
        };
        let mut q = vec![0.0; k * k];
        let mut w = vec![0.0; k * cols];
        for (i, &stamp) in self.varying.stamps.iter().enumerate() {
            for j in 0..k {
                q[i * k + j] = pick(&y, k, stamp, j);
            }
            for c in 0..cols {
                w[i * cols + c] = -pick(m, cols, stamp, c);
            }
        }
        // The rows: controls, capacitors, inductors, the probe.
        let rows: Vec<(Pin, Pin)> = self
            .controls
            .iter()
            .copied()
            .chain(self.capacitors.iter().map(|c| (c.a, c.b)))
            .chain(self.inductors.iter().map(|l| (Some(l.row), None)))
            .chain(std::iter::once(self.probe))
            .collect();
        let mut u = vec![0.0; rows.len() * k];
        let mut base = vec![0.0; rows.len() * cols];
        for (r, &(a, b)) in rows.iter().enumerate() {
            for j in 0..k {
                u[r * k + j] = across(&y, k, a, b, j);
            }
            for c in 0..cols {
                base[r * cols + c] = across(m, cols, a, b, c);
            }
        }
        self.varying.woodbury = Some(Woodbury::new(k, rows.len(), cols, q, u, w, base));
        self.varying.base = self.varying.variables.values.clone();
        self.varying.delta = vec![0.0; k];
        Ok(())
    }

    /// The matrices for the branch values in effect, by one low-rank
    /// update from the prepared ones; capacitors keep their voltage and
    /// current. Realtime.
    pub(crate) fn refresh(&mut self) {
        let step = self.design_step();
        let Some(woodbury) = self.varying.woodbury.as_mut() else {
            return;
        };
        let values = &self.varying.variables.values;
        for (j, &stamp) in self.varying.stamps.iter().enumerate() {
            let change = values[j] - self.varying.base[j];
            self.varying.delta[j] = match stamp {
                Stamp::Row(_) => -change,
                Stamp::Capacitor(_) => 2.0 * change / step,
            };
        }
        if !woodbury.update(&self.varying.delta) {
            // Values the circuit has no solution at: hold the last good
            // matrices (a pot cannot get here; a resistor set to short a
            // source can).
            return;
        }
        // Capacitors: v and i carry over, so the history x = g·v + i moves
        // by Δg·v, v from this sample's history and the last.
        let n = self.n;
        for (j, &stamp) in self.varying.stamps.iter().enumerate() {
            let value = self.varying.variables.values[j];
            match stamp {
                Stamp::Row(r) => {
                    if self.static_matrix[r * n + r] != -value {
                        self.static_matrix[r * n + r] = -value;
                        self.varying.stale = true;
                    }
                }
                Stamp::Capacitor(c) => {
                    let old = 2.0 * self.capacitors[c].farads / self.design_step();
                    let new = 2.0 * value / self.design_step();
                    if old != new {
                        let volts = if self.samples == 0 {
                            self.x[c] / old
                        } else {
                            (self.x[c] + self.x_next[c]) / (2.0 * old)
                        };
                        self.x[c] += (new - old) * volts;
                        self.x_next[c] += (new - old) * volts;
                        self.capacitors[c].farads = value;
                    }
                }
            }
        }
        self.install();
    }

    /// The updated rows into the state-space matrices.
    fn install(&mut self) {
        let Some(woodbury) = self.varying.woodbury.as_ref() else {
            return;
        };
        let o = &woodbury.out;
        let (nc, nl) = (self.capacitors.len(), self.inductors.len());
        let (nx, nu, nq, nv) = (nc + nl, self.u.len(), self.nq, self.controls.len());
        let cols = nx + nu + nq;
        let split = |row: &[f64], scale: f64, x: &mut [f64], u: &mut [f64], q: &mut [f64], i: usize| {
            for (d, s) in x[i * nx..(i + 1) * nx].iter_mut().zip(&row[..nx]) {
                *d = scale * s;
            }
            for (d, s) in u[i * nu..(i + 1) * nu].iter_mut().zip(&row[nx..nx + nu]) {
                *d = scale * s;
            }
            for (d, s) in q[i * nq..(i + 1) * nq].iter_mut().zip(&row[nx + nu..]) {
                *d = scale * s;
            }
        };
        for i in 0..nv {
            split(&o[i * cols..(i + 1) * cols], 1.0, &mut self.dx, &mut self.du, &mut self.k, i);
        }
        for s in 0..nx {
            let scale = if s < nc {
                2.0 * (2.0 * self.capacitors[s].farads / self.design_step())
            } else {
                2.0 * (2.0 * self.inductors[s - nc].henries / self.design_step())
            };
            let row = &o[(nv + s) * cols..(nv + s + 1) * cols];
            split(row, scale, &mut self.ax, &mut self.au, &mut self.aq, s);
            self.ax[s * nx + s] -= 1.0;
        }
        // A stored charge's nonlinear part feeds its capacitor's state
        // (DSP.md R11; `discretise`).
        let t = self.design_step();
        for link in &self.charges {
            self.aq[link.capacitor * nq + link.output] += 4.0 / t;
        }
        let row = &o[(nv + nx) * cols..(nv + nx + 1) * cols];
        split(row, 1.0, &mut self.ex, &mut self.eu, &mut self.eq, 0);
    }

    /// The reduced matrices, for comparing an updated circuit with a
    /// freshly built one.
    #[cfg(test)]
    pub(crate) fn matrices(&self) -> [&[f64]; 9] {
        [&self.dx, &self.du, &self.k, &self.ax, &self.au, &self.aq, &self.ex, &self.eu, &self.eq]
    }

}
