//! The nonlinear subsystem's solve (N3): find the controlling voltages
//! `v` with `v = p + K·q(v)`, where `q` are the devices' outputs.
//!
//! - **Damped Newton with device limiting**, generalising the scalar
//!   solve every topology uses: a step whose Newton correction does not
//!   shrink is halved (down to `min_damping`; Deuflhard's natural
//!   monotonicity test, which unlike a residual norm ignores how the
//!   equations are scaled), a step is bounded by `max_step`, and each
//!   junction bounds its own voltage's step (SPICE's `pnjlim`), so an
//!   iterate never climbs far up an exponential.
//! - **Warm start**: each sample starts from the previous sample's
//!   solution.
//! - **Continuation** (homotopy in the linear prediction `p`): when
//!   Newton fails from the warm start `v₀`, the sample is re-solved along
//!   `p(s) = p₀ + s·(p − p₀)` in `continuation_steps` steps, each
//!   starting from the last, where `p₀ = v₀ − K·q(v₀)` is the prediction
//!   `v₀` solves exactly; at the operating point the same idea is source
//!   stepping.
//! - **Honest convergence**: a solve has converged when its last step is
//!   below `tolerance` *and* the equations hold to a microvolt per volt —
//!   a stalled step in an ill-conditioned system is not convergence.
//! - **Reported, bounded**: at most
//!   `max_iterations · (1 + continuation_steps)` iterations a sample, each
//!   at most `1 + log2(1/min_damping)` device evaluations; a solve that
//!   ends unconverged is counted in [`NewtonStats::capped`], never
//!   hidden.

use super::devices::Nonlinear;
use super::matrix::{lu, lu_solve};
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats};

/// The largest residual, relative to `1 + max |v|`, a converged solve
/// may leave in `v = p + K·q(v)` (a microvolt on a volt).
const RESIDUAL: f64 = 1e-6;

/// How hard the DK solve tries.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Settings {
    pub newton: NewtonSettings,
    /// Continuation steps tried when Newton fails from the warm start;
    /// 0 disables the fallback, and the operating point's source
    /// stepping with it.
    pub continuation_steps: u32,
}

impl Settings {
    pub const DEFAULT: Self = Self {
        newton: NewtonSettings::DEFAULT,
        continuation_steps: 8,
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

/// What the solves did, accumulated: Newton work and cap hits, how
/// often the continuation fallback ran, and the worst residual a solve
/// ended on — what a plugin publishes as telemetry taps (N8; stave-core's
/// `SolverTaps`), so a circuit that struggles at some setting shows it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Health {
    /// Iterations (sweeps, under the scattering iterative method), cap
    /// hits, the largest final step and the most iterations one sample
    /// took.
    pub newton: NewtonStats,
    /// Samples whose warm-started Newton failed and fell back to
    /// continuation (converged or not; a final failure is also in
    /// `newton.capped`).
    pub continuations: u64,
    /// The largest residual a solve ended on, in volts (the nonlinear
    /// equations' `|p + K·q(v) − v|`; the waves' last change under the
    /// scattering iterative method). NaN for a solver that does not
    /// report one ([`From<NewtonStats>`](#impl-From<NewtonStats>-for-Health)).
    pub max_residual: f64,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            newton: NewtonStats::default(),
            continuations: 0,
            max_residual: 0.0,
        }
    }
}

impl Health {
    /// Account one sample's solve.
    #[inline]
    pub fn record(&mut self, iterations: u32, converged: bool, final_step: f64, residual: f64) {
        self.newton.record(iterations, converged, final_step);
        self.max_residual = self.max_residual.max(residual.abs());
    }

    /// Both accounts as one.
    pub fn merge(&mut self, other: &Self) {
        self.newton.merge(&other.newton);
        self.continuations += other.continuations;
        self.max_residual = self.max_residual.max(other.max_residual);
    }

    /// Mean iterations per solve (per sample, for a circuit).
    pub fn iterations_per_sample(&self) -> f64 {
        self.newton.mean_iterations()
    }
}

/// A hand-derived topology's Newton account as a health report: no
/// continuation, no residual reported (NaN).
impl From<NewtonStats> for Health {
    fn from(newton: NewtonStats) -> Self {
        Self {
            newton,
            continuations: 0,
            max_residual: f64::NAN,
        }
    }
}

/// One solve's outcome.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Outcome {
    pub iterations: u32,
    pub converged: bool,
    pub final_step: f64,
    /// The largest `|r|` the solve ended on.
    pub residual: f64,
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
    correction: Vec<f64>,
    p_step: Vec<f64>,
    p_start: Vec<f64>,
    v_start: Vec<f64>,
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
            correction: vec![0.0; nv],
            p_step: vec![0.0; nv],
            p_start: vec![0.0; nv],
            v_start: vec![0.0; nv],
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

    /// Solve `v = p + K·q(v)` from the current `v`, falling back to
    /// continuation; `v` and `q` hold the result, `health` the account.
    ///
    /// Any start `v₀` solves exactly the prediction `p₀ = v₀ − K·q(v₀)`,
    /// so the path from `p₀` to `p` always begins at a solution, even
    /// after a sample that failed.
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
        self.v_start.copy_from_slice(&self.v);
        let mut outcome = self.newton(p, k, devices, time, &settings.newton);
        if !outcome.converged && settings.continuation_steps > 0 {
            health.continuations += 1;
            self.v.copy_from_slice(&self.v_start);
            evaluate(devices, &self.v, &mut self.q, &mut self.jq, self.nv, time);
            // p₀ = v₀ − K·q(v₀): the prediction the start solves exactly.
            let nq = self.nq;
            for i in 0..self.nv {
                let mut s = self.v[i];
                for j in 0..nq {
                    s -= k[i * nq + j] * self.q[j];
                }
                self.p_start[i] = s;
            }
            let steps = settings.continuation_steps;
            let mut total = outcome.iterations;
            for s in 1..=steps {
                let f = f64::from(s) / f64::from(steps);
                for i in 0..self.nv {
                    self.p_step[i] = self.p_start[i] + f * (p[i] - self.p_start[i]);
                }
                let p_step = std::mem::take(&mut self.p_step);
                outcome = self.newton(&p_step, k, devices, time, &settings.newton);
                self.p_step = p_step;
                total += outcome.iterations;
                if !outcome.converged {
                    break;
                }
            }
            outcome.iterations = total;
        }
        health.record(outcome.iterations, outcome.converged, outcome.final_step, outcome.residual);
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
        residual(p, k, &self.q, &self.v, &mut self.r, nq);
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
            // Natural monotonicity (Deuflhard): a step is accepted when the
            // Newton correction at the trial point, J⁻¹·r(v + λδ) with this
            // iteration's J, is smaller than δ. Unlike the residual's norm it
            // is blind to how the equations are scaled — a plate's volts-per-
            // amp row would otherwise veto good full steps.
            let size = self.delta.iter().fold(0.0f64, |m, d| m.max(d.abs()));
            let mut damping = 1.0;
            let accepted = loop {
                for i in 0..nv {
                    self.v_try[i] = self.v[i] + damping * self.delta[i];
                }
                for d in devices.iter() {
                    d.limit(&mut self.v_try, &self.v);
                }
                evaluate(devices, &self.v_try, &mut self.q_try, &mut self.jq_try, nv, time);
                residual(p, k, &self.q_try, &self.v_try, &mut self.r_try, nq);
                for i in 0..nv {
                    self.correction[i] = -self.r_try[i];
                }
                lu_solve(&self.jacobian, nv, &self.pivots, &mut self.correction);
                let next = self.correction.iter().fold(0.0f64, |m, d| m.max(d.abs()));
                if next < size || size == 0.0 || (next.is_finite() && damping <= settings.min_damping) {
                    break true;
                }
                if damping <= settings.min_damping {
                    break false;
                }
                damping *= 0.5;
            };
            if !accepted {
                break;
            }
            step = 0.0;
            for i in 0..nv {
                step = f64::max(step, (self.v_try[i] - self.v[i]).abs());
            }
            std::mem::swap(&mut self.v, &mut self.v_try);
            std::mem::swap(&mut self.q, &mut self.q_try);
            std::mem::swap(&mut self.jq, &mut self.jq_try);
            std::mem::swap(&mut self.r, &mut self.r_try);
            iterations += 1;
        }
        // A step below tolerance is convergence only where the equations
        // hold too: an ill-conditioned system can stall with a tiny step
        // and a residual of volts.
        let scale = 1.0 + self.v.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let residual = self.r.iter().fold(0.0f64, |m, r| {
            if m.is_nan() || r.is_nan() {
                f64::NAN
            } else {
                m.max(r.abs())
            }
        });
        Outcome {
            iterations,
            converged: step < settings.tolerance && residual <= RESIDUAL * scale,
            final_step: step,
            residual,
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

/// `r = p + K·q − v`.
fn residual(p: &[f64], k: &[f64], q: &[f64], v: &[f64], r: &mut [f64], nq: usize) {
    for i in 0..r.len() {
        let mut s = p[i] - v[i];
        for j in 0..nq {
            s += k[i * nq + j] * q[j];
        }
        r[i] = s;
    }
}
