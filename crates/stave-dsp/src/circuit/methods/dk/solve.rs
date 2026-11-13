//! The nonlinear subsystem's solve: find the controlling voltages `v`
//! with `v = p + K·q(v)`, where `q` are the devices' outputs, by damped
//! Newton from the previous sample's solution — a step that does not
//! reduce the residual is halved (down to `min_damping`), a step is
//! bounded by `max_step`, and each junction bounds its own voltage's
//! step (SPICE's `pnjlim`). A solve that ends unconverged is counted in
//! [`NewtonStats::capped`], never hidden.

use super::devices::Nonlinear;
use super::matrix::{lu, lu_solve};
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};

/// How hard the DK solve tries.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Settings {
    pub newton: NewtonSettings,
}

impl Settings {
    pub const DEFAULT: Self = Self {
        newton: NewtonSettings::DEFAULT,
    };
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The equations a sample's solve faces: `v = p + K·q(v)`.
#[derive(Copy, Clone)]
pub(crate) struct Prediction<'a> {
    pub p: &'a [f64],
    pub k: &'a [f64],
}

/// What the solves did, accumulated: Newton work and cap hits.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Health {
    pub newton: NewtonStats,
}

/// One solve's outcome.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Outcome {
    pub iterations: u32,
    pub converged: bool,
    pub final_step: f64,
}

/// The solve's buffers, sized once so a sample never allocates.
#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    nv: usize,
    nq: usize,
    pub v: Vec<f64>,
    pub q: Vec<f64>,
    jq: Vec<f64>,
    v_try: Vec<f64>,
    q_try: Vec<f64>,
    jq_try: Vec<f64>,
    r: Vec<f64>,
    r_try: Vec<f64>,
    jacobian: Vec<f64>,
    pivots: Vec<usize>,
    delta: Vec<f64>,
}

impl Workspace {
    pub fn new(nv: usize, nq: usize) -> Self {
        Self {
            nv,
            nq,
            v: vec![0.0; nv],
            q: vec![0.0; nq],
            jq: vec![0.0; nq * nv],
            v_try: vec![0.0; nv],
            q_try: vec![0.0; nq],
            jq_try: vec![0.0; nq * nv],
            r: vec![0.0; nv],
            r_try: vec![0.0; nv],
            jacobian: vec![0.0; nv * nv],
            pivots: vec![0; nv],
            delta: vec![0.0; nv],
        }
    }

    /// The device outputs' Jacobian at the current `v` (`nq`×`nv`).
    pub fn jacobian_of_outputs(&self) -> &[f64] {
        &self.jq
    }

    /// Evaluate the devices at the current `v` into `q` and its Jacobian.
    pub fn evaluate(&mut self, devices: &mut [Nonlinear], time: f64) {
        evaluate(devices, &self.v, &mut self.q, &mut self.jq, self.nv, time);
    }

    /// Solve `v = p + K·q(v)` from the current `v`; `v` and `q` hold the
    /// result, `health` the account.
    pub fn solve(
        &mut self,
        prediction: Prediction<'_>,
        devices: &mut [Nonlinear],
        time: f64,
        settings: &Settings,
        health: &mut Health,
    ) {
        let Prediction { p, k } = prediction;
        if self.nv == 0 {
            evaluate(devices, &self.v, &mut self.q, &mut self.jq, 0, time);
            return;
        }
        let outcome = self.newton(p, k, devices, time, &settings.newton);
        health
            .newton
            .record(outcome.iterations, outcome.converged, outcome.final_step);
    }

    /// Damped Newton from the current `v`.
    pub fn newton(
        &mut self,
        p: &[f64],
        k: &[f64],
        devices: &mut [Nonlinear],
        time: f64,
        settings: &NewtonSettings,
    ) -> Outcome {
        let (nv, nq) = (self.nv, self.nq);
        evaluate(devices, &self.v, &mut self.q, &mut self.jq, nv, time);
        let mut norm = residual(p, k, &self.q, &self.v, &mut self.r, nq);
        let (mut iterations, mut step) = (0, f64::INFINITY);
        while iterations < settings.max_iterations && (step.is_nan() || step >= settings.tolerance) {
            // J = K·Jq − I; solve J·δ = −r.
            for i in 0..nv {
                for c in 0..nv {
                    let mut s = if i == c { -1.0 } else { 0.0 };
                    for j in 0..nq {
                        s += k[i * nq + j] * self.jq[j * nv + c];
                    }
                    self.jacobian[i * nv + c] = s;
                }
                self.delta[i] = -self.r[i];
            }
            if !lu(&mut self.jacobian, nv, &mut self.pivots) {
                break;
            }
            lu_solve(&self.jacobian, nv, &self.pivots, &mut self.delta);
            let largest = self.delta.iter().fold(0.0f64, |m, d| m.max(d.abs()));
            if largest > settings.max_step {
                let scale = settings.max_step / largest;
                self.delta.iter_mut().for_each(|d| *d *= scale);
            }
            let mut damping = 1.0;
            let accepted = loop {
                for i in 0..nv {
                    self.v_try[i] = self.v[i] + damping * self.delta[i];
                }
                for d in devices.iter() {
                    d.limit(&mut self.v_try, &self.v);
                }
                evaluate(devices, &self.v_try, &mut self.q_try, &mut self.jq_try, nv, time);
                let trial = residual(p, k, &self.q_try, &self.v_try, &mut self.r_try, nq);
                if trial < norm {
                    break Some(trial);
                }
                if damping <= settings.min_damping {
                    break trial.is_finite().then_some(trial);
                }
                damping *= 0.5;
            };
            let Some(trial) = accepted else {
                break;
            };
            step = 0.0;
            for i in 0..nv {
                step = f64::max(step, (self.v_try[i] - self.v[i]).abs());
            }
            std::mem::swap(&mut self.v, &mut self.v_try);
            std::mem::swap(&mut self.q, &mut self.q_try);
            std::mem::swap(&mut self.jq, &mut self.jq_try);
            std::mem::swap(&mut self.r, &mut self.r_try);
            norm = trial;
            iterations += 1;
        }
        Outcome {
            iterations,
            converged: step < settings.tolerance,
            final_step: step,
        }
    }
}

/// Every device's outputs and their Jacobian at `v`.
fn evaluate(devices: &mut [Nonlinear], v: &[f64], q: &mut [f64], jq: &mut [f64], nv: usize, time: f64) {
    jq.iter_mut().for_each(|x| *x = 0.0);
    for d in devices.iter_mut() {
        d.evaluate(v, q, jq, nv, time);
    }
}

/// `r = p + K·q − v`; returns |r|².
fn residual(p: &[f64], k: &[f64], q: &[f64], v: &[f64], r: &mut [f64], nq: usize) -> f64 {
    let mut norm = 0.0;
    for i in 0..r.len() {
        let mut s = p[i] - v[i];
        for j in 0..nq {
            s += k[i * nq + j] * q[j];
        }
        r[i] = s;
        norm += s * s;
    }
    norm
}
