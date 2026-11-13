//! First-order stateful ADAA for the DK method (Holters, DAFx 2019):
//! the sample's device outputs `q` replaced by their mean over the line
//! from the last prediction to this one, the states and inputs averaged
//! to the same instant, the matrices designed for `1.5·T`
//! ([`methods::adaa`](crate::circuit::methods::adaa) has the derivation).
//!
//! ```text
//!     p[n] = Dx·x[n−1] + Du·u[n]                      (as without ADAA)
//!     q̃    = mean of q(p) over p[n−1] → p[n]
//!     x̄    = ½(x[n−1] + x[n−2]),  ū = ½(u[n] + u[n−1])
//!     y[n] = Ex·x̄ + Eu·ū + Eq·q̃
//!     x[n] = Ax·x̄ + Au·ū + Aq·q̃
//! ```
//!
//! The mean itself (`Kernel`) is the nonlinear subsystem's alone, so
//! the wave digital method's R-type root (`Circuit<Wdf>`), which faces
//! the same `v = p + K·q(v)`, uses it too.

use super::devices::Nonlinear;
use super::matrix::accumulate;
use super::solve::Prediction;
use super::Dk;
use crate::circuit::methods::adaa::{one_port_mean, LOBATTO};

/// How a circuit method forms its nonlinear subsystem's mean over a
/// sample when ADAA is on: decided at prepare from the subsystem
/// ([`Dk::adaa_mean`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mean {
    /// One controlling voltage and every output through one port, each
    /// device with a closed-form antiderivative (diodes, a single one
    /// behind its series resistance included): exact, one solve a sample.
    ClosedForm,
    /// Anything else, a junction with stored charge included (held to
    /// ngspice: `methods::adaa`'s Measured): the line integral by
    /// Gauss–Lobatto quadrature on `nodes` points, the two ends being
    /// this sample's and the last sample's solves, so `nodes − 1` solves
    /// a sample.
    Quadrature { nodes: usize },
}

/// The Lobatto rule the quadrature uses: three nodes (Simpson's), one
/// extra solve a sample. More nodes help the hardest circuits (Fuzz Face
/// at 4×: −21.1 dB with three, −27.6 with five) but cost a solve each,
/// and at any count the quadrature costs more than oversampling to the
/// same alias level (`aliasing_budgets.rs`).
pub(crate) const QUADRATURE_NODES: usize = 3;

/// The one-port's combination of outputs: `i = Σ weights[k]·q[k]` and
/// `K·q = κ·i`; `reference`'s column carries every output's effect.
#[derive(Clone, Debug)]
struct OnePort {
    kappa: f64,
    weights: Vec<f64>,
    reference: usize,
}

/// The mean of a nonlinear subsystem `v = p + K·q(v)` over each sample's
/// line of predictions, with its history. A sample runs
/// [`begin`](Self::begin), then for each interior node
/// [`node`](Self::node), the caller's solve there, [`add`](Self::add);
/// then the caller's solve at `p` and [`finish`](Self::finish).
#[derive(Clone, Debug)]
pub(crate) struct Kernel {
    one_port: Option<OnePort>,
    p_prev: Vec<f64>,
    q_prev: Vec<f64>,
    v_prev: f64,
    current_prev: f64,
    q_mean: Vec<f64>,
    p_node: Vec<f64>,
}

impl Kernel {
    pub fn new(nv: usize, nq: usize) -> Self {
        Self {
            one_port: None,
            p_prev: vec![0.0; nv],
            q_prev: vec![0.0; nq],
            v_prev: 0.0,
            current_prev: 0.0,
            q_mean: vec![0.0; nq],
            p_node: vec![0.0; nv],
        }
    }

    pub fn mean(&self) -> Mean {
        if self.one_port.is_some() {
            Mean::ClosedForm
        } else {
            Mean::Quadrature {
                nodes: QUADRATURE_NODES,
            }
        }
    }

    /// Find the closed-form one-port, if the subsystem is one: one
    /// controlling voltage, every device a diode, every output injected
    /// through the same port (parallel columns of the `n`×`nq` output
    /// matrix `bq`). `k` is the `1`×`nq` loop matrix.
    pub fn find_one_port(&mut self, bq: &[f64], n: usize, k: &[f64], devices: &[Nonlinear]) {
        self.one_port = None;
        let nq = self.q_mean.len();
        if self.p_node.len() != 1 || nq == 0 || !devices.iter().all(Nonlinear::has_closed_form_mean) {
            return;
        }
        let column = |c: usize| (0..n).map(move |r| bq[r * nq + c]);
        let norm = |c: usize| column(c).map(|x| x * x).sum::<f64>();
        let reference = (0..nq).max_by(|&a, &b| norm(a).total_cmp(&norm(b))).unwrap_or(0);
        let base = norm(reference);
        if base == 0.0 {
            return;
        }
        let mut weights = vec![0.0; nq];
        for (c, w) in weights.iter_mut().enumerate() {
            let scale = column(c).zip(column(reference)).map(|(a, b)| a * b).sum::<f64>() / base;
            let parallel = column(c)
                .zip(column(reference))
                .all(|(a, b)| (a - scale * b).abs() <= 1e-12 * (a.abs() + b.abs()).max(1e-300));
            if !parallel {
                return;
            }
            *w = scale;
        }
        self.one_port = Some(OnePort {
            kappa: k[reference],
            weights,
            reference,
        });
    }

    /// At rest: the last sample solved `v` with outputs `q`, its
    /// prediction the one `v` solves exactly, `p = v − K·q`.
    pub fn reset(&mut self, v: &[f64], q: &[f64], k: &[f64]) {
        let nq = q.len();
        self.q_prev.copy_from_slice(q);
        for (i, p) in self.p_prev.iter_mut().enumerate() {
            *p = v[i] - (0..nq).map(|j| k[i * nq + j] * q[j]).sum::<f64>();
        }
        self.v_prev = v.first().copied().unwrap_or(0.0);
        self.current_prev = self.current(q);
    }

    fn current(&self, q: &[f64]) -> f64 {
        self.one_port
            .as_ref()
            .map_or(0.0, |o| o.weights.iter().zip(q).map(|(w, q)| w * q).sum())
    }

    /// Start a sample at prediction `p`: how many interior nodes to solve
    /// (none for the closed form, or when the prediction barely moved —
    /// the ends' mean is then the line's to within the step squared).
    pub fn begin(&mut self, p: &[f64]) -> usize {
        if self.one_port.is_some() {
            return 0;
        }
        let moved = p.iter().zip(&self.p_prev).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
        let scale = 1.0 + p.iter().fold(0.0f64, |m, p| m.max(p.abs()));
        let (_, weights) = LOBATTO[QUADRATURE_NODES - 3];
        for (m, q) in self.q_mean.iter_mut().zip(&self.q_prev) {
            *m = weights[0] * q;
        }
        if moved > 1e-9 * scale {
            QUADRATURE_NODES - 2
        } else {
            0
        }
    }

    /// The prediction at interior node `j` of the line to `p`.
    pub fn node(&mut self, j: usize, p: &[f64]) -> &[f64] {
        let (nodes, _) = LOBATTO[QUADRATURE_NODES - 3];
        let tau = nodes[j + 1];
        for (i, node) in self.p_node.iter_mut().enumerate() {
            *node = self.p_prev[i] + tau * (p[i] - self.p_prev[i]);
        }
        &self.p_node
    }

    /// The outputs solved at interior node `j`.
    pub fn add(&mut self, j: usize, q: &[f64]) {
        let (_, weights) = LOBATTO[QUADRATURE_NODES - 3];
        for (m, q) in self.q_mean.iter_mut().zip(q) {
            *m += weights[j + 1] * q;
        }
    }

    /// End the sample with the solve at `p` (`v`, `q`): the mean, and
    /// the history moved on.
    pub fn finish(&mut self, p: &[f64], v: &[f64], q: &[f64], devices: &[Nonlinear], interior: usize) -> &[f64] {
        if let Some(one_port) = &self.one_port {
            let (v0, v1) = (self.v_prev, v[0]);
            let current = self.current(q);
            let (mut mean, mut secant) = (0.0, 0.0);
            for d in devices {
                let (m, s) = d.closed_form_mean(v0, v1);
                let w = one_port.weights[d.output];
                mean += w * m;
                secant += w * s;
            }
            let i = one_port_mean(mean, secant, 0.5 * (current + self.current_prev), one_port.kappa);
            let reference = one_port.reference;
            self.q_mean.iter_mut().for_each(|q| *q = 0.0);
            self.q_mean[reference] = i;
            self.v_prev = v1;
            self.current_prev = current;
        } else if interior == 0 {
            for (m, (a, b)) in self.q_mean.iter_mut().zip(self.q_prev.iter().zip(q)) {
                *m = 0.5 * (a + b);
            }
        } else {
            let (_, weights) = LOBATTO[QUADRATURE_NODES - 3];
            let last = weights[weights.len() - 1];
            for (m, q) in self.q_mean.iter_mut().zip(q) {
                *m += last * q;
            }
        }
        self.p_prev.copy_from_slice(p);
        self.q_prev.copy_from_slice(q);
        &self.q_mean
    }
}

/// The DK method's ADAA history and scratch, sized at build.
#[derive(Clone, Debug)]
pub(super) struct State {
    pub kernel: Kernel,
    x_prev: Vec<f64>,
    u_prev: Vec<f64>,
    x_mean: Vec<f64>,
    u_mean: Vec<f64>,
}

impl State {
    pub fn new(nx: usize, nu: usize, nv: usize, nq: usize) -> Self {
        Self {
            kernel: Kernel::new(nv, nq),
            x_prev: vec![0.0; nx],
            u_prev: vec![0.0; nu],
            x_mean: vec![0.0; nx],
            u_mean: vec![0.0; nu],
        }
    }
}

impl Dk {
    /// ADAA's history at rest: the last sample is the operating point
    /// (or the initial conditions). Called at the end of `reset`.
    pub(super) fn reset_adaa(&mut self) {
        self.adaa.x_prev.copy_from_slice(&self.x);
        self.inputs_at(0.0);
        self.adaa.u_prev.copy_from_slice(&self.u);
        self.adaa.kernel.reset(&self.work.v, &self.work.q, &self.k);
    }

    /// One sample with ADAA: `self.u` holds this sample's inputs and
    /// `self.p` its prediction; returns the probe.
    pub(super) fn step_adaa(&mut self, time: f64) -> f64 {
        let (nx, nu, nq) = (self.x.len(), self.u.len(), self.nq);
        let interior = self.adaa.kernel.begin(&self.p);
        for j in 0..interior {
            let p = self.adaa.kernel.node(j, &self.p);
            self.work.solve(
                Prediction { p, k: &self.k },
                &mut self.devices,
                time,
                &self.settings,
                &mut self.health,
            );
            self.adaa.kernel.add(j, &self.work.q);
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
        let adaa = &mut self.adaa;
        let q_mean = adaa
            .kernel
            .finish(&self.p, &self.work.v, &self.work.q, &self.devices, interior);
        for i in 0..nx {
            adaa.x_mean[i] = 0.5 * (self.x[i] + adaa.x_prev[i]);
        }
        for i in 0..nu {
            adaa.u_mean[i] = 0.5 * (self.u[i] + adaa.u_prev[i]);
        }
        let mut y = [0.0];
        accumulate(&self.ex, &adaa.x_mean, &mut y, nx);
        accumulate(&self.eu, &adaa.u_mean, &mut y, nu);
        accumulate(&self.eq, q_mean, &mut y, nq);
        self.x_next.iter_mut().for_each(|x| *x = 0.0);
        accumulate(&self.ax, &adaa.x_mean, &mut self.x_next, nx);
        accumulate(&self.au, &adaa.u_mean, &mut self.x_next, nu);
        accumulate(&self.aq, q_mean, &mut self.x_next, nq);
        for x in &mut self.x_next {
            *x = crate::kernel::denormal::flush64(*x);
        }
        // History moves on: x[n−1] becomes x[n−2].
        adaa.x_prev.copy_from_slice(&self.x);
        std::mem::swap(&mut self.x, &mut self.x_next);
        adaa.u_prev.copy_from_slice(&self.u);
        y[0]
    }
}

#[cfg(test)]
mod tests;
