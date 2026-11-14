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
//! - **Honest convergence**: a solve has converged when its error is
//!   below `tolerance` *and* the equations hold to a microvolt per volt —
//!   a stalled step in an ill-conditioned system is not convergence. The
//!   error is the size of the last step — or, per sample, the next Newton
//!   correction, which the monotonicity test has already computed at the
//!   accepted point (Deuflhard's simplified correction `J⁻¹·r(v + δ)`):
//!   where a full step contracts it at least twofold (the quadratic
//!   regime, where it is the true next step to first order) and it is
//!   below tolerance, it is applied as a chord step, the device outputs
//!   moved along their Jacobian with it, and the solve stops — without
//!   the iteration that would only confirm convergence. The operating
//!   point is solved to a step below tolerance.
//! - **Sparse device Jacobian**: each device's outputs depend only on its
//!   own controls, so `K·Jq` is assembled from the structural nonzeros of
//!   `Jq` (found once from the devices), not as a dense product.
//! - **Block-triangular decomposition** (the strongly connected components
//!   of the subsystem's dependency graph, found at prepare): where
//!   a stage drives the next without being loaded by it — an op-amp's
//!   output, an ideal source — `K` has no coupling back, and the subsystem
//!   is solved block by block in dependency order, each block's Newton on
//!   its own controls with the blocks before it held. The same solution
//!   (the couplings left out are `K`'s rounding, below 1e-13 of their
//!   row), from small factorisations, each block iterating only as much as
//!   it needs. A block that fails sends the sample to the whole system's
//!   continuation; a part's value that couples blocks after all ends the
//!   split until the next prepare.
//! - **Reported, bounded**: at most
//!   `max_iterations · (1 + continuation_steps)` iterations a sample (per
//!   block: a split solve reports the most any block took), each
//!   at most `1 + log2(1/min_damping)` device evaluations; a solve that
//!   ends unconverged is counted in [`NewtonStats::capped`], never
//!   hidden.

use super::devices::Nonlinear;
use super::matrix::Lu;
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

/// Below this fraction of its row's largest entry, a `K` entry is taken as
/// no coupling when the subsystem is split into blocks: the rounding an
/// ideal source's decoupling leaves in `K` sits near 1e-16 of the row.
const DECOUPLED: f64 = 1e-13;

/// Controls solved together and the devices they read: one strongly
/// connected component of the subsystem's dependency graph.
#[derive(Clone, Debug, Default)]
struct Block {
    /// Its unknowns, global control indices, ascending.
    controls: Vec<usize>,
    /// The first of them when they are a contiguous run.
    first: Option<usize>,
    /// The devices it evaluates, ascending.
    devices: Vec<usize>,
    /// Their outputs, indices into `q`.
    outputs: Vec<usize>,
    /// `Jq`'s structural nonzeros on the block's own controls, `(output,
    /// control, column in the block)`, outputs ascending.
    columns: Vec<(usize, usize, usize)>,
    /// The same, `(output, control)` by column then output, and each
    /// column's range in it: the Jacobian's assembly a column at a time.
    assembly: Vec<(usize, usize)>,
    by_column: Vec<(usize, usize)>,
    /// Every `Jq` entry its devices write, `(output, control)`.
    pattern: Vec<(usize, usize)>,
    /// Its Jacobian, assembled and factored in place each iteration.
    lu: Lu,
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
    delta: Vec<f64>,
    correction: Vec<f64>,
    p_step: Vec<f64>,
    p_start: Vec<f64>,
    v_start: Vec<f64>,
    /// Every control and device as one block (the operating point's
    /// solve, continuation, and any subsystem that does not split).
    whole: Block,
    /// The blocks in dependency order, when the subsystem splits into
    /// more than one ([`decompose`](Self::decompose)).
    blocks: Vec<Block>,
    /// The `K` entries `(control, output)` the order takes as no coupling:
    /// an output of a later block on an earlier block's control.
    decoupled: Vec<(usize, usize)>,
    /// Whether the per-sample solve runs block by block.
    split: bool,
    /// `Kᵀ` (`nq`×`nv`), for assembling the Jacobian a column at a time:
    /// kept by [`decompose`](Self::decompose) and [`recheck`](Self::recheck),
    /// which every change to `K` goes through.
    kt: Vec<f64>,
    /// One column of `K` gathered, for a block whose controls are not one
    /// run, or a `K` whose transpose is not kept.
    gathered: Vec<f64>,
    /// Which `Jq` entries a device may write (the whole block's pattern
    /// as a mask: the debug check that no device writes outside it).
    #[cfg(debug_assertions)]
    written: Vec<bool>,
}

impl Workspace {
    /// Buffers for `devices`, whose outputs are `nq` and controls `nv`.
    pub fn new(nv: usize, nq: usize, devices: &[Nonlinear]) -> Self {
        let all_devices: Vec<usize> = (0..devices.len()).collect();
        let whole = block(&(0..nv).collect::<Vec<_>>(), &all_devices, devices, nq);
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
            delta: vec![0.0; nv],
            correction: vec![0.0; nv],
            p_step: vec![0.0; nv],
            p_start: vec![0.0; nv],
            v_start: vec![0.0; nv],
            blocks: Vec::new(),
            decoupled: Vec::new(),
            split: false,
            kt: vec![0.0; nv * nq],
            gathered: vec![0.0; nv],
            #[cfg(debug_assertions)]
            written: {
                let mut mask = vec![false; nq * nv];
                whole.pattern.iter().for_each(|&(j, c)| mask[j * nv + c] = true);
                mask
            },
            whole,
        }
    }

    /// The device outputs' Jacobian at the current `v` (`nq`×`nv`).
    pub fn jacobian_of_outputs(&self) -> &[f64] {
        &self.jq
    }

    /// Evaluate the devices at the current `v` into `q` and its Jacobian.
    pub fn evaluate(&mut self, devices: &mut [Nonlinear], time: f64) {
        let whole = std::mem::take(&mut self.whole);
        self.evaluate_block(&whole, devices, time, false);
        self.whole = whole;
    }

    /// Split the subsystem `v = p + K·q(v)` into blocks solved in turn:
    /// the strongly connected components of its dependency graph (a
    /// device reads its controls; a control reads the outputs its row of
    /// `K` weighs above [`DECOUPLED`] of its largest), in dependency
    /// order. An op-amp's output stage drives what follows it without
    /// being loaded by it, so a pedal's stages decouple: the DS-1's 13
    /// controls are four blocks, the Phase 90's 20 are ten. At prepare
    /// (it allocates); a subsystem that does not split is solved whole.
    pub fn decompose(&mut self, k: &[f64], devices: &[Nonlinear]) {
        self.transpose(k);
        let (nv, nq, nd) = (self.nv, self.nq, devices.len());
        let outputs = |d: usize| devices[d].output..devices.get(d + 1).map_or(nq, |n| n.output);
        let scale: Vec<f64> = (0..nv).map(|c| row_scale(k, c, nq)).collect();
        // Nodes: controls 0..nv, then devices; an edge runs from what is
        // read to what reads it.
        let mut reads: Vec<Vec<usize>> = vec![Vec::new(); nv + nd];
        for (d, device) in devices.iter().enumerate() {
            for &(c, _) in &device.controls {
                if !reads[c].contains(&(nv + d)) {
                    reads[c].push(nv + d);
                }
            }
            for c in 0..nv {
                if outputs(d).any(|j| k[c * nq + j].abs() > DECOUPLED * scale[c]) {
                    reads[nv + d].push(c);
                }
            }
        }
        let components = strongly_connected(&reads);
        let mut order = vec![0; nv + nd];
        self.blocks = components
            .iter()
            .enumerate()
            .map(|(b, nodes)| {
                nodes.iter().for_each(|&n| order[n] = b);
                let mut controls: Vec<usize> = nodes.iter().copied().filter(|&n| n < nv).collect();
                let mut members: Vec<usize> = nodes.iter().filter(|&&n| n >= nv).map(|&n| n - nv).collect();
                controls.sort_unstable();
                members.sort_unstable();
                block(&controls, &members, devices, nq)
            })
            .collect();
        self.decoupled.clear();
        for c in 0..nv {
            for d in 0..nd {
                if order[nv + d] > order[c] {
                    self.decoupled.extend(outputs(d).filter(|&j| k[c * nq + j] != 0.0).map(|j| (c, j)));
                }
            }
        }
        self.split = self.blocks.len() > 1;
    }

    /// After `K` moved (a part's value, N6): keep the blocks only while
    /// every coupling their order takes as none still is. Realtime.
    pub fn recheck(&mut self, k: &[f64]) {
        self.transpose(k);
        let nq = self.nq;
        if self.split
            && self.decoupled.iter().any(|&(c, j)| k[c * nq + j].abs() > DECOUPLED * row_scale(k, c, nq))
        {
            self.split = false;
        }
    }

    /// Keep `Kᵀ` for `k`.
    fn transpose(&mut self, k: &[f64]) {
        let (nv, nq) = (self.nv, self.nq);
        for i in 0..nv {
            for j in 0..nq {
                self.kt[j * nv + i] = k[i * nq + j];
            }
        }
    }

    /// Solve whole from now on (a test's reference for the split solve).
    #[cfg(test)]
    pub fn unsplit(&mut self) {
        self.split = false;
    }

    /// The couplings the block order takes as none, `(control, output)`.
    #[cfg(test)]
    pub fn decoupled(&self) -> &[(usize, usize)] {
        &self.decoupled
    }

    /// The blocks the per-sample solve runs, in order: their sizes in
    /// controls (one block, the whole subsystem, when it does not split).
    pub fn block_sizes(&self) -> impl Iterator<Item = usize> + '_ {
        let blocks = if self.split { &self.blocks[..] } else { std::slice::from_ref(&self.whole) };
        blocks.iter().map(|b| b.controls.len())
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
        #[cfg(debug_assertions)]
        {
            let (nv, nq) = (self.nv, self.nq);
            let kept = (0..nv).all(|i| (0..nq).all(|j| self.kt[j * nv + i].to_bits() == k[i * nq + j].to_bits()));
            debug_assert!(kept, "K changed without Workspace::recheck");
        }
        if self.nv == 0 {
            self.evaluate(devices, time);
            return;
        }
        self.v_start.copy_from_slice(&self.v);
        let mut outcome = if self.split {
            self.solve_blocks(p, k, devices, time, &settings.newton)
        } else {
            let mut whole = std::mem::take(&mut self.whole);
            let outcome = self.iterate(&mut whole, p, k, devices, time, &settings.newton, true);
            self.whole = whole;
            outcome
        };
        if !outcome.converged && settings.continuation_steps > 0 {
            health.continuations += 1;
            self.v.copy_from_slice(&self.v_start);
            self.evaluate(devices, time);
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
            let mut whole = std::mem::take(&mut self.whole);
            for s in 1..=steps {
                let f = f64::from(s) / f64::from(steps);
                for i in 0..self.nv {
                    self.p_step[i] = self.p_start[i] + f * (p[i] - self.p_start[i]);
                }
                let p_step = std::mem::take(&mut self.p_step);
                outcome = self.iterate(&mut whole, &p_step, k, devices, time, &settings.newton, true);
                self.p_step = p_step;
                total += outcome.iterations;
                if !outcome.converged {
                    break;
                }
            }
            self.whole = whole;
            outcome.iterations = total;
        }
        health.record(outcome.iterations, outcome.converged, outcome.final_step, outcome.residual);
    }

    /// Each block in order, the ones before it already solved: the
    /// iterations the longest took, converged when every block did.
    fn solve_blocks(
        &mut self,
        p: &[f64],
        k: &[f64],
        devices: &mut [Nonlinear],
        time: f64,
        settings: &NewtonSettings,
    ) -> Outcome {
        // A block's trials read the solved blocks before it through the
        // trial buffers: they agree with the solution outside the block.
        self.v_try.copy_from_slice(&self.v);
        self.q_try.copy_from_slice(&self.q);
        self.jq_try.copy_from_slice(&self.jq);
        let mut blocks = std::mem::take(&mut self.blocks);
        let mut total = Outcome {
            iterations: 0,
            converged: true,
            final_step: 0.0,
            residual: 0.0,
        };
        for b in &mut blocks {
            let o = self.iterate(b, p, k, devices, time, settings, true);
            total.iterations = total.iterations.max(o.iterations);
            total.final_step = total.final_step.max(o.final_step);
            total.residual = if total.residual.is_nan() || o.residual.is_nan() {
                f64::NAN
            } else {
                total.residual.max(o.residual)
            };
            if !o.converged {
                total.converged = false;
                break;
            }
            for &c in &b.controls {
                self.v_try[c] = self.v[c];
            }
            for &j in &b.outputs {
                self.q_try[j] = self.q[j];
            }
            for &(j, c) in &b.pattern {
                self.jq_try[j * self.nv + c] = self.jq[j * self.nv + c];
            }
        }
        self.blocks = blocks;
        total
    }

    /// Damped Newton from the current `v`, to a step below tolerance
    /// (the operating point's solve, run once: no early stop).
    pub fn newton(
        &mut self,
        p: &[f64],
        k: &[f64],
        devices: &mut [Nonlinear],
        time: f64,
        settings: &NewtonSettings,
    ) -> Outcome {
        let mut whole = std::mem::take(&mut self.whole);
        let outcome = self.iterate(&mut whole, p, k, devices, time, settings, false);
        self.whole = whole;
        outcome
    }

    /// The block's devices at `v` (or the trial `v`) into `q` and `Jq`
    /// (or their trial buffers); `Jq` is zero outside the devices'
    /// pattern, and only the block's entries are cleared.
    fn evaluate_block(&mut self, b: &Block, devices: &mut [Nonlinear], time: f64, trial: bool) {
        let nv = self.nv;
        let (v, q, jq) = if trial {
            (&self.v_try, &mut self.q_try, &mut self.jq_try)
        } else {
            (&self.v, &mut self.q, &mut self.jq)
        };
        for &(j, c) in &b.pattern {
            jq[j * nv + c] = 0.0;
        }
        for &d in &b.devices {
            devices[d].evaluate(v, q, jq, nv, time);
        }
        #[cfg(debug_assertions)]
        debug_assert!(
            jq.iter().zip(&self.written).all(|(&x, &written)| written || x == 0.0),
            "a device wrote outside its Jacobian pattern"
        );
    }

    /// The block's rows of `r = p + K·q − v` (or of the trial's).
    fn residual_block(&mut self, b: &Block, p: &[f64], k: &[f64], trial: bool) {
        let nq = self.nq;
        let (v, q, r) = if trial {
            (&self.v_try, &self.q_try[..nq], &mut self.r_try)
        } else {
            (&self.v, &self.q[..nq], &mut self.r)
        };
        for &i in &b.controls {
            let mut s = p[i] - v[i];
            for (a, b) in k[i * nq..(i + 1) * nq].iter().zip(q) {
                s += a * b;
            }
            r[i] = s;
        }
    }

    /// Damped Newton on block `b`'s controls from the current `v`, the
    /// blocks before it held; with `early`, a solve in the quadratic
    /// regime stops on the correction it has already computed (module
    /// docs) and takes it as a chord step.
    #[allow(clippy::too_many_arguments)]
    fn iterate(
        &mut self,
        b: &mut Block,
        p: &[f64],
        k: &[f64],
        devices: &mut [Nonlinear],
        time: f64,
        settings: &NewtonSettings,
        early: bool,
    ) -> Outcome {
        // The per-sample solves (`early`) face the circuit's own `K`, whose
        // transpose is kept; the operating point's faces the DC one, read
        // as given.
        let transposed = early;
        let (nv, nq, m) = (self.nv, self.nq, b.controls.len());
        self.evaluate_block(b, devices, time, false);
        self.residual_block(b, p, k, false);
        let (mut iterations, mut step) = (0, f64::INFINITY);
        if m == 0 {
            step = 0.0;
        }
        while iterations < settings.max_iterations && (step.is_nan() || step >= settings.tolerance) {
            // J = K·Jq − I on the block's controls, over Jq's structural
            // nonzeros (each device's outputs read its own controls only);
            // solve J·δ = −r. A column at a time, each written whole (no
            // clearing pass): `K`'s column for each output that reads the
            // control (a row of `Kᵀ`, contiguous) times its slope, summed
            // in ascending output order onto the identity's −1 — the sums
            // a row at a time would form, in the same order.
            let stride = b.lu.stride();
            let jacobian = b.lu.columns_mut();
            for (a, &c) in b.controls.iter().enumerate() {
                self.delta[a] = -self.r[c];
            }
            for (column, &(from, to)) in b.by_column.iter().enumerate() {
                let x = &mut jacobian[column * stride..column * stride + m];
                let mut written = false;
                for &(j, c) in &b.assembly[from..to] {
                    let slope = self.jq[j * nv + c];
                    if slope == 0.0 {
                        continue;
                    }
                    let k_column: &[f64] = match (b.first, transposed) {
                        (Some(first), true) => &self.kt[j * nv + first..j * nv + first + m],
                        (_, true) => {
                            for (y, &row) in self.gathered.iter_mut().zip(&b.controls) {
                                *y = self.kt[j * nv + row];
                            }
                            &self.gathered[..m]
                        }
                        (_, false) => {
                            for (y, &row) in self.gathered.iter_mut().zip(&b.controls) {
                                *y = k[row * nq + j];
                            }
                            &self.gathered[..m]
                        }
                    };
                    if written {
                        for (x, &kj) in x.iter_mut().zip(k_column) {
                            *x += kj * slope;
                        }
                    } else {
                        for (x, &kj) in x.iter_mut().zip(k_column) {
                            *x = kj * slope;
                        }
                        x[column] = -1.0 + k_column[column] * slope;
                        written = true;
                    }
                }
                if !written {
                    x.fill(0.0);
                    x[column] = -1.0;
                }
            }
            if !b.lu.factor() {
                break;
            }
            b.lu.solve(&mut self.delta);
            let largest = self.delta[..m].iter().fold(0.0f64, |s, d| s.max(d.abs()));
            if largest > settings.max_step {
                let scale = settings.max_step / largest;
                self.delta[..m].iter_mut().for_each(|d| *d *= scale);
            }
            // Natural monotonicity (Deuflhard): a step is accepted when the
            // Newton correction at the trial point, J⁻¹·r(v + λδ) with this
            // iteration's J, is smaller than δ. Unlike the residual's norm it
            // is blind to how the equations are scaled — a plate's volts-per-
            // amp row would otherwise veto good full steps.
            let size = self.delta[..m].iter().fold(0.0f64, |s, d| s.max(d.abs()));
            let mut damping = 1.0;
            let (accepted, next) = loop {
                for (a, &c) in b.controls.iter().enumerate() {
                    self.v_try[c] = self.v[c] + damping * self.delta[a];
                }
                for &d in &b.devices {
                    devices[d].limit(&mut self.v_try, &self.v);
                }
                self.evaluate_block(b, devices, time, true);
                self.residual_block(b, p, k, true);
                for (a, &c) in b.controls.iter().enumerate() {
                    self.correction[a] = -self.r_try[c];
                }
                b.lu.solve(&mut self.correction);
                let next = self.correction[..m].iter().fold(0.0f64, |s, d| s.max(d.abs()));
                if next < size || size == 0.0 || (next.is_finite() && damping <= settings.min_damping) {
                    break (true, next);
                }
                if damping <= settings.min_damping {
                    break (false, next);
                }
                damping *= 0.5;
            };
            if !accepted {
                break;
            }
            step = b.controls.iter().fold(0.0f64, |s, &c| s.max((self.v_try[c] - self.v[c]).abs()));
            // A full step that contracted the correction at least twofold
            // is in Newton's quadratic regime: the correction just computed
            // is the next step to first order. Below tolerance — and where
            // the equations hold — it is taken as a chord step (this
            // iteration's J), the outputs moved along their Jacobian to
            // match, its own error second order in it: the iteration that
            // would only confirm convergence is not run.
            if early
                && damping == 1.0
                && next <= 0.5 * step
                && next < settings.tolerance
                && holds(&b.controls, &self.r_try, &self.v_try)
            {
                for (a, &c) in b.controls.iter().enumerate() {
                    self.v_try[c] += self.correction[a];
                }
                for &(j, c, column) in &b.columns {
                    self.q_try[j] += self.jq_try[j * nv + c] * self.correction[column];
                }
                step = next;
            }
            for &c in &b.controls {
                self.v[c] = self.v_try[c];
                self.r[c] = self.r_try[c];
            }
            for &j in &b.outputs {
                self.q[j] = self.q_try[j];
            }
            for &(j, c) in &b.pattern {
                self.jq[j * nv + c] = self.jq_try[j * nv + c];
            }
            iterations += 1;
        }
        // A step below tolerance is convergence only where the equations
        // hold too: an ill-conditioned system can stall with a tiny step
        // and a residual of volts.
        Outcome {
            iterations,
            converged: step < settings.tolerance && holds(&b.controls, &self.r, &self.v),
            final_step: step,
            residual: largest_residual(&b.controls, &self.r),
        }
    }
}

/// A block of `controls` and `devices`: its outputs and `Jq` entries.
fn block(controls: &[usize], devices: &[usize], all: &[Nonlinear], nq: usize) -> Block {
    let contiguous = controls.windows(2).all(|w| w[1] == w[0] + 1);
    let m = controls.len();
    let mut b = Block {
        lu: Lu::new(m),
        controls: controls.to_vec(),
        first: controls.first().copied().filter(|_| contiguous),
        devices: devices.to_vec(),
        ..Block::default()
    };
    for &d in devices {
        // A device owns the outputs from its first to the next device's
        // (its currents, then its charges), each read from its controls.
        let end = all.get(d + 1).map_or(nq, |next| next.output);
        let mut columns: Vec<usize> = all[d].controls.iter().map(|&(g, _)| g).collect();
        columns.sort_unstable();
        columns.dedup();
        for j in all[d].output..end {
            b.outputs.push(j);
            for &c in &columns {
                b.pattern.push((j, c));
                if let Some(column) = controls.iter().position(|&x| x == c) {
                    b.columns.push((j, c, column));
                }
            }
        }
    }
    b.pattern.sort_unstable();
    b.columns.sort_unstable();
    let mut by_column: Vec<(usize, usize, usize)> =
        b.columns.iter().map(|&(j, c, column)| (column, j, c)).collect();
    by_column.sort_unstable();
    b.assembly = by_column.iter().map(|&(_, j, c)| (j, c)).collect();
    b.by_column = (0..m)
        .map(|column| {
            let from = by_column.partition_point(|e| e.0 < column);
            (from, by_column.partition_point(|e| e.0 <= column))
        })
        .collect();
    b
}

/// The largest `|K|` in row `c`.
fn row_scale(k: &[f64], c: usize, nq: usize) -> f64 {
    k[c * nq..(c + 1) * nq].iter().fold(0.0f64, |m, x| m.max(x.abs()))
}

/// The strongly connected components of the graph `edges` (node → the
/// nodes reading it), sources first (Tarjan 1972, whose components come
/// out sinks first).
fn strongly_connected(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct Search<'a> {
        edges: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        components: Vec<Vec<usize>>,
    }
    fn visit(s: &mut Search<'_>, v: usize) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &s.edges[v] {
            match s.index[w] {
                None => {
                    visit(s, w);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(i) if s.on_stack[w] => s.low[v] = s.low[v].min(i),
                Some(_) => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            let mut component = Vec::new();
            while let Some(w) = s.stack.pop() {
                s.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            s.components.push(component);
        }
    }
    let n = edges.len();
    let mut s = Search {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for v in 0..n {
        if s.index[v].is_none() {
            visit(&mut s, v);
        }
    }
    s.components.reverse();
    s.components
}

/// The largest `|r|` over `rows`, NaN if any is.
fn largest_residual(rows: &[usize], r: &[f64]) -> f64 {
    rows.iter()
        .fold(0.0f64, |m, &i| if m.is_nan() || r[i].is_nan() { f64::NAN } else { m.max(r[i].abs()) })
}

/// Whether the equations of `rows` hold at `v` to [`RESIDUAL`] of its
/// scale.
fn holds(rows: &[usize], r: &[f64], v: &[f64]) -> bool {
    let scale = 1.0 + rows.iter().fold(0.0f64, |m, &i| m.max(v[i].abs()));
    largest_residual(rows, r) <= RESIDUAL * scale
}
