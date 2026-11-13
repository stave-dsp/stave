//! The R-type adaptor (N4): any number of ports joined by an arbitrary
//! topology — a bridge, a bridged-T, a tone stack's loops — which no
//! tree of series and parallel adaptors can express.
//!
//! # Derivation
//!
//! Werner, Smith & Abel, "Wave digital filter adaptors for arbitrary
//! topologies and multiport linear elements" (DAFx-15): replace every
//! port by its Thévenin equivalent (the arriving wave `aₖ` behind the
//! port resistance `Rₖ`) and solve the topology by modified nodal
//! analysis. Written with Norton equivalents, the nodal equations are
//!
//! ```text
//!     X·w = N·G·a        X = N·G·Nᵀ (+ the rows of absorbed op-amps)
//!     b   = 2·Nᵀ·w − a   so   S = 2·Nᵀ·X⁻¹·N·G − I
//! ```
//!
//! with `N` the ports' incidence on the nodes (the datum, node 0,
//! removed) and `G = diag(1/Rₖ)`. An ideal op-amp is absorbed as a
//! nullor: one branch current at its output and the row
//! `v(+) − v(−) = 0` (Werner et al., "Modeling circuits with arbitrary
//! topologies and active linear multiports using wave digital filters",
//! IEEE TCAS-I 2018, §V).
//!
//! An R-type adaptor inside a tree adapts the port facing its parent:
//! choosing that port's resistance as the Thévenin resistance the other
//! ports present across it zeroes `S₀₀`, so the wave it sends up never
//! waits on the one coming down.

use super::OnePort;
use crate::circuit::methods::dk::matrix::solve_columns;

/// The nodal matrix of a topology: `ports` (node pairs, `plus` to
/// `minus`, node 0 the datum) with conductances `g`, and `op_amps`
/// (`(plus, minus, output)`) absorbed as nullors. Unknowns: `nodes − 1`
/// node voltages, then one output current per op-amp.
fn nodal(nodes: usize, ports: &[(usize, usize)], g: &[f64], op_amps: &[(usize, usize, usize)]) -> (usize, Vec<f64>) {
    let n = nodes - 1 + op_amps.len();
    let mut x = vec![0.0; n * n];
    let mut add = |r: usize, c: usize, v: f64| {
        if r > 0 && c > 0 {
            x[(r - 1) * n + (c - 1)] += v;
        }
    };
    for (&(p, m), &g) in ports.iter().zip(g) {
        add(p, p, g);
        add(m, m, g);
        add(p, m, -g);
        add(m, p, -g);
    }
    for (k, &(plus, minus, out)) in op_amps.iter().enumerate() {
        let row = nodes + k;
        add(out, row, 1.0);
        add(row, plus, 1.0);
        add(row, minus, -1.0);
    }
    (n, x)
}

/// `w[a] − w[b]` over node indices (0 the datum).
fn across(w: &[f64], cols: usize, col: usize, a: usize, b: usize) -> f64 {
    let at = |i: usize| if i == 0 { 0.0 } else { w[(i - 1) * cols + col] };
    at(a) - at(b)
}

/// The scattering matrix (`P`×`P`, row-major) of the R-type adaptor
/// joining `ports` of resistances `ohms`, with `op_amps` absorbed; `None`
/// when the topology's equations are singular (a floating node, or an
/// op-amp with no feedback).
pub fn scattering(
    nodes: usize,
    ports: &[(usize, usize)],
    ohms: &[f64],
    op_amps: &[(usize, usize, usize)],
) -> Option<Vec<f64>> {
    let p = ports.len();
    let g: Vec<f64> = ohms.iter().map(|r| 1.0 / r).collect();
    let (n, x) = nodal(nodes, ports, &g, op_amps);
    // Right-hand side N·G: column k injects gₖ into plus, out of minus.
    let mut rhs = vec![0.0; n * p];
    for (k, &(a, b)) in ports.iter().enumerate() {
        if a > 0 {
            rhs[(a - 1) * p + k] += g[k];
        }
        if b > 0 {
            rhs[(b - 1) * p + k] -= g[k];
        }
    }
    let w = solve_columns(&x, n, &rhs, p)?;
    let mut s = vec![0.0; p * p];
    for (i, &(a, b)) in ports.iter().enumerate() {
        for k in 0..p {
            s[i * p + k] = 2.0 * across(&w, p, k, a, b) - if i == k { 1.0 } else { 0.0 };
        }
    }
    Some(s)
}

/// The resistance seen across `ports[at]` with every other port in
/// place (its own removed): the value that adapts it.
pub fn thevenin(
    nodes: usize,
    ports: &[(usize, usize)],
    ohms: &[f64],
    op_amps: &[(usize, usize, usize)],
    at: usize,
) -> Option<f64> {
    let mut g: Vec<f64> = ohms.iter().map(|r| 1.0 / r).collect();
    g[at] = 0.0;
    let (n, x) = nodal(nodes, ports, &g, op_amps);
    let (a, b) = ports[at];
    let mut rhs = vec![0.0; n];
    if a > 0 {
        rhs[a - 1] += 1.0;
    }
    if b > 0 {
        rhs[b - 1] -= 1.0;
    }
    let w = solve_columns(&x, n, &rhs, 1)?;
    let r = across(&w, 1, 0, a, b);
    (r.is_finite() && r > 0.0).then_some(r)
}

/// An R-type adaptor in a tree: port 0 faces the parent (adapted), the
/// rest are its children, each any [`OnePort`].
///
/// ```
/// use stave_dsp::circuit::methods::wdf::{elements, OnePort, RType, Resistor};
///
/// // A Wheatstone bridge: 1 V across top (1) and bottom (0), arms to
/// // nodes 2 and 3, a bridge resistor between them.
/// let r = |ohms| Box::new(Resistor { ohms }) as Box<dyn OnePort + Send>;
/// let mut bridge = RType::new(
///     4,
///     (1, 0),
///     vec![((1, 2), r(1e3)), ((2, 0), r(2e3)), ((1, 3), r(2e3)), ((3, 0), r(1e3)), ((2, 3), r(5e3))],
/// )
/// .unwrap();
/// elements::sample(&mut bridge, &mut elements::VoltageSource { volts: 1.0 });
/// // By symmetry the bridge sits between 2/3 V and 1/3 V.
/// assert!(bridge.port_volts(5) > 0.0 && bridge.port_volts(5) < 1.0 / 3.0);
/// ```
pub struct RType {
    nodes: usize,
    ports: Vec<(usize, usize)>,
    op_amps: Vec<(usize, usize, usize)>,
    children: Vec<Box<dyn OnePort + Send>>,
    ohms: Vec<f64>,
    s: Vec<f64>,
    a: Vec<f64>,
    b: Vec<f64>,
}

impl RType {
    /// `nodes` counts the datum (node 0); `up` is the parent's port and
    /// each child sits on its node pair. `None` when the topology is
    /// singular or the parent's port cannot be adapted (nothing behind
    /// it). Children are taken as prepared; [`prepare`](OnePort::prepare)
    /// re-prepares them and re-adapts.
    pub fn new(
        nodes: usize,
        up: (usize, usize),
        children: Vec<((usize, usize), Box<dyn OnePort + Send>)>,
    ) -> Option<Self> {
        Self::with_op_amps(nodes, up, children, Vec::new())
    }

    /// [`new`](Self::new) with ideal op-amps `(plus, minus, output)`
    /// absorbed as nullors.
    pub fn with_op_amps(
        nodes: usize,
        up: (usize, usize),
        children: Vec<((usize, usize), Box<dyn OnePort + Send>)>,
        op_amps: Vec<(usize, usize, usize)>,
    ) -> Option<Self> {
        let mut ports = vec![up];
        let mut kids = Vec::with_capacity(children.len());
        for (pair, child) in children {
            ports.push(pair);
            kids.push(child);
        }
        let p = ports.len();
        let mut r = Self {
            nodes,
            ports,
            op_amps,
            children: kids,
            ohms: vec![1.0; p],
            s: vec![0.0; p * p],
            a: vec![0.0; p],
            b: vec![0.0; p],
        };
        r.adapt().then_some(r)
    }

    /// Re-derive the scattering matrix from the children's resistances
    /// (after a rate or value change); false when singular.
    pub fn adapt(&mut self) -> bool {
        for (k, c) in self.children.iter().enumerate() {
            self.ohms[k + 1] = c.resistance();
        }
        let Some(up) = thevenin(self.nodes, &self.ports, &self.ohms, &self.op_amps, 0) else {
            return false;
        };
        self.ohms[0] = up;
        match scattering(self.nodes, &self.ports, &self.ohms, &self.op_amps) {
            Some(s) => {
                self.s = s;
                true
            }
            None => false,
        }
    }

    /// The scattering matrix, port 0 the parent's: `b = S·a`.
    pub fn scattering(&self) -> &[f64] {
        &self.s
    }

    /// Port resistances, port 0 the adapted one.
    pub fn resistances(&self) -> &[f64] {
        &self.ohms
    }

    pub fn child_mut(&mut self, k: usize) -> &mut (dyn OnePort + Send) {
        self.children[k].as_mut()
    }

    /// Port `k`'s voltage after the last sample (port 0 the parent's).
    pub fn port_volts(&self, k: usize) -> f64 {
        0.5 * (self.a[k] + self.b[k])
    }
}

impl OnePort for RType {
    fn resistance(&self) -> f64 {
        self.ohms[0]
    }
    fn reflected(&mut self) -> f64 {
        let p = self.ports.len();
        for (k, c) in self.children.iter_mut().enumerate() {
            self.a[k + 1] = c.reflected();
        }
        // S₀₀ = 0: the parent's own wave plays no part.
        self.b[0] = (1..p).map(|k| self.s[k] * self.a[k]).sum();
        self.b[0]
    }
    fn incident(&mut self, wave: f64) {
        let p = self.ports.len();
        self.a[0] = wave;
        for (k, c) in self.children.iter_mut().enumerate() {
            let row = &self.s[(k + 1) * p..(k + 2) * p];
            let b: f64 = row.iter().zip(&self.a).map(|(s, a)| s * a).sum();
            self.b[k + 1] = b;
            c.incident(b);
        }
    }
    fn prepare(&mut self, sample_rate: f32) {
        for c in &mut self.children {
            c.prepare(sample_rate);
        }
        let adapted = self.adapt();
        debug_assert!(adapted, "an R-type adaptor that adapted at build cannot fail at a new rate");
    }
}

#[cfg(test)]
mod tests;
