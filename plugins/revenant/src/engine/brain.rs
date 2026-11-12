//! `Brain` — an echo-state network (Jaeger's reservoir computer),
//! written out: N leaky tanh neurons, sparsely and randomly wired, a
//! fixed random input projection, and ONE trained thing — a linear
//! readout, learned online by normalised LMS to predict the input
//! HORIZON steps ahead from the network's state.
//!
//! * The wiring is fixed at birth (seeded, so every instance is the
//!   same creature) and normalised to spectral radius 1; CHAOS scales
//!   it. Below 1 the echo of an input dies away (the "echo-state
//!   property"); past 1 the network sustains its own activity.
//! * MEMORY is the neurons' leak — fast leak hears timbre, slow leak
//!   integrates over longer stretches.
//! * Training uses the state from HORIZON steps ago against the input
//!   now, so the readout learns to see the future: its output is a
//!   guess at what comes next.
//! * DREAM closes the loop through a SECOND readout, trained in
//!   parallel to predict just one step ahead — the generator, learned
//!   by recursive least squares (the FORCE recipe, Sussillo & Abbott:
//!   LMS is too imprecise for a closed loop; RLS is near-optimal). A long-
//!   horizon guess chained on itself compounds its errors and slides
//!   into the network's own attractor; a one-step generator stays on
//!   the shape it learned. The network hears the generator's voice
//!   instead of — or mixed with — the input; fully closed, it plays on
//!   by itself, and what you hear crossfades from its guess at your
//!   future to its own continuation.
//!
//! Allocation happens only in `new`/`prepare`; the tick is fixed-size
//! arithmetic over preallocated buffers.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use stave_dsp::Xorshift32;

pub(super) const NEURONS: usize = 96;
/// Incoming connections per neuron.
const FAN_IN: usize = 8;
/// The longest horizon, in brain steps (25 ms at the 12 kHz brain).
pub(super) const MAX_HORIZON: usize = 320;
const INPUT_GAIN: f32 = 1.4;
/// NLMS regulariser and a whisper of weight decay (keeps the readout
/// from drifting on long sessions).
const EPS: f32 = 1e-3;
const DECAY: f32 = 1e-6;
/// The readout's step size at LEARN 1.
const MAX_MU: f32 = 0.6;
/// The dream loop's DC blocker pole (~15 Hz at the 12 kHz brain).
const DC_POLE: f32 = 0.992;
/// Training noise on the network's input while it LEARNS (teacher
/// forcing with jitter, Jaeger's recipe): the generator learns to pull
/// a perturbed state back onto the learned shape, so its closed loop
/// settles there instead of compounding its errors.
const TRAIN_NOISE: f32 = 0.004;
/// The generator's RLS: how many neurons it reads, how often it
/// updates (brain steps), its forgetting factor (a ~4 s window of
/// updates), its initial inverse-correlation scale, and the ceiling on
/// P's trace (a guard: with no excitation RLS's P grows without bound).
const GEN_N: usize = 64;
const GEN_EVERY: usize = 2;
const GEN_LAMBDA: f64 = 0.99995;
const GEN_P0: f64 = 50.0;
const GEN_TRACE_MAX: f64 = GEN_P0 * GEN_N as f64 * 4.0;

/// `scale` · I, as RLS's starting inverse correlation (allocates: call
/// only off the audio path — `new`/`reset` — or into a reused buffer).
fn identity(scale: f64) -> Vec<f64> {
    let mut m = vec![0.0; GEN_N * GEN_N];
    for i in 0..GEN_N {
        m[i * GEN_N + i] = scale;
    }
    m
}

/// P back to `GEN_P0` · I, in place (no allocation — audio-safe).
fn reset_p(p: &mut [f64]) {
    p.fill(0.0);
    for i in 0..GEN_N {
        p[i * GEN_N + i] = GEN_P0;
    }
}

#[inline]
fn tanh(x: f32) -> f32 {
    // A rational tanh: exact sign, saturating, cheap.
    let x = x.clamp(-4.5, 4.5);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

pub(super) struct Brain {
    /// Sparse wiring: for neuron i, `src[i*FAN_IN + k]` feeds it with
    /// `w[i*FAN_IN + k]`.
    src: Vec<u16>,
    w: Vec<f32>,
    w_in: Vec<f32>,
    x: Vec<f32>,
    pre: Vec<f32>,
    /// Ring of past states (MAX_HORIZON + 1 deep) and past outputs.
    past: Vec<f32>,
    past_y: Vec<f32>,
    head: usize,
    readout: Vec<f32>,
    /// The one-step generator readout (over the first GEN_N neurons),
    /// its RLS inverse-correlation matrix, and its last voice.
    gen: Vec<f32>,
    p: Vec<f64>,
    pk: Vec<f64>,
    gen_tick: usize,
    y_gen: f32,
    /// DC blocker on the dream loop (a free-running network creeps).
    dc_x: f32,
    dc_y: f32,
    y: f32,
    rng: Xorshift32,
}

/// One step's settings.
#[derive(Copy, Clone)]
pub(super) struct Mind {
    pub(super) rho: f32,
    pub(super) leak: f32,
    pub(super) mu: f32,
    pub(super) horizon: usize,
    pub(super) dream: f32,
    pub(super) learning: bool,
}

impl Mind {
    /// The face's knobs → the network's numbers.
    pub(super) fn from_knobs(learn: f32, memory: f32, chaos: f32, horizon_steps: usize, dream: f32, asleep: bool) -> Self {
        Self {
            rho: 0.6 + 0.75 * chaos,
            leak: 0.9 * (0.05f32 / 0.9).powf(memory),
            mu: MAX_MU * learn * learn,
            horizon: horizon_steps.clamp(1, MAX_HORIZON),
            dream: if asleep { 1.0 } else { dream },
            learning: !asleep && learn > 0.0,
        }
    }
}

impl Brain {
    pub(super) fn new() -> Self {
        let mut rng = Xorshift32::new(0x0E5C_A1A1);
        let mut src = vec![0u16; NEURONS * FAN_IN];
        let mut w = vec![0.0f32; NEURONS * FAN_IN];
        for i in 0..NEURONS {
            for k in 0..FAN_IN {
                src[i * FAN_IN + k] = (rng.next_u32() % NEURONS as u32) as u16;
                w[i * FAN_IN + k] = rng.next_bipolar();
            }
        }
        let w_in: Vec<f32> = (0..NEURONS).map(|_| {
            let s = if rng.next_unit() < 0.5 { -1.0 } else { 1.0 };
            s * (0.4 + 0.6 * rng.next_unit())
        }).collect();
        let mut b = Self {
            src,
            w,
            w_in,
            x: vec![0.0; NEURONS],
            pre: vec![0.0; NEURONS],
            past: vec![0.0; NEURONS * (MAX_HORIZON + 1)],
            past_y: vec![0.0; MAX_HORIZON + 1],
            head: 0,
            readout: vec![0.0; NEURONS],
            gen: vec![0.0; GEN_N],
            p: identity(GEN_P0),
            pk: vec![0.0; GEN_N],
            gen_tick: 0,
            y_gen: 0.0,
            dc_x: 0.0,
            dc_y: 0.0,
            y: 0.0,
            rng: Xorshift32::new(0xD12E_A3F1),
        };
        b.normalise();
        b
    }

    /// Scale the wiring to spectral radius ≈ 1 (power iteration on the
    /// sparse matrix — the radius of a random matrix is its largest
    /// eigenvalue's magnitude; |λ| is what CHAOS then scales).
    fn normalise(&mut self) {
        let mut v = vec![1.0f32; NEURONS];
        let mut tmp = vec![0.0f32; NEURONS];
        let mut lambda = 1.0f32;
        for _ in 0..200 {
            for i in 0..NEURONS {
                let mut s = 0.0;
                for k in 0..FAN_IN {
                    s += self.w[i * FAN_IN + k] * v[self.src[i * FAN_IN + k] as usize];
                }
                tmp[i] = s;
            }
            // Two steps of the iteration handle a complex-conjugate pair.
            let n2: f32 = tmp.iter().map(|a| a * a).sum::<f32>().sqrt();
            let n1: f32 = v.iter().map(|a| a * a).sum::<f32>().sqrt();
            lambda = n2 / n1.max(1e-12);
            for (a, b) in v.iter_mut().zip(&tmp) {
                *a = b / n2.max(1e-12);
            }
        }
        let scale = 1.0 / lambda.max(1e-6);
        for w in &mut self.w {
            *w *= scale;
        }
    }

    pub(super) fn reset(&mut self) {
        self.x.fill(0.0);
        self.past.fill(0.0);
        self.past_y.fill(0.0);
        self.head = 0;
        self.readout.fill(0.0);
        self.gen.fill(0.0);
        reset_p(&mut self.p);
        self.gen_tick = 0;
        self.y_gen = 0.0;
        self.dc_x = 0.0;
        self.dc_y = 0.0;
        self.y = 0.0;
    }

    /// One brain step: hear `u`, think, learn, guess. Returns (the
    /// guess, the error it just made predicting `u`).
    #[inline]
    pub(super) fn tick(&mut self, u: f32, m: Mind) -> (f32, f32) {
        let ring = MAX_HORIZON + 1;
        // What it hears: the world, its own guess (lined up by the
        // horizon), or a blend.
        // The generator's last voice is its guess at THIS step's input —
        // DC-blocked, so a free-running dream cannot creep off zero.
        let fed = self.y_gen - self.dc_x + DC_POLE * self.dc_y;
        self.dc_x = self.y_gen;
        self.dc_y = fed;
        let jitter = if m.learning && m.mu > 0.0 && u != 0.0 { TRAIN_NOISE * self.rng.next_bipolar() } else { 0.0 };
        let drive = (1.0 - m.dream) * u + m.dream * fed + jitter;

        // Learn first, from the state `horizon` steps back vs now.
        // (The error is measured always — it is the SURPRISE.)
        let base = ((self.head + ring - m.horizon) % ring) * NEURONS;
        let old = &self.past[base..base + NEURONS];
        let guess: f32 = self.readout.iter().zip(old).map(|(w, x)| w * x).sum();
        let err = u - guess;
        if m.learning && m.mu > 0.0 {
            let norm: f32 = EPS + old.iter().map(|x| x * x).sum::<f32>();
            let k = m.mu * err / norm;
            for (w, x) in self.readout.iter_mut().zip(old) {
                *w += k * x - DECAY * *w;
            }
            // The generator learns the one-step future from the
            // current (pre-update) state, by RLS — only when there is
            // something to learn (silence would only inflate P).
            self.gen_tick += 1;
            if self.gen_tick.is_multiple_of(GEN_EVERY) && u.abs() > 1e-4 {
                self.rls(u);
            }
        }

        // Think: one leaky step of the network.
        for i in 0..NEURONS {
            let mut s = 0.0;
            let row = i * FAN_IN;
            for k in 0..FAN_IN {
                s += self.w[row + k] * self.x[self.src[row + k] as usize];
            }
            self.pre[i] = m.rho * s + INPUT_GAIN * self.w_in[i] * drive;
        }
        for i in 0..NEURONS {
            self.x[i] += m.leak * (tanh(self.pre[i]) - self.x[i]);
        }

        // Remember this state; guess the future from it.
        self.head = (self.head + 1) % ring;
        let base = self.head * NEURONS;
        self.past[base..base + NEURONS].copy_from_slice(&self.x);
        self.y = self.readout.iter().zip(&self.x).map(|(w, x)| w * x).sum::<f32>().clamp(-2.0, 2.0);
        self.y_gen = self.gen.iter().zip(&self.x[..GEN_N]).map(|(w, x)| w * x).sum::<f32>().clamp(-2.0, 2.0);
        self.past_y[self.head] = self.y;
        // What it says: its guess at your future while it listens, its
        // own continuation as it dreams.
        (self.y + (self.y_gen - self.y) * m.dream, err)
    }

    /// One RLS step: the generator's weights toward predicting `u` from
    /// the state it was just in. P ← (P − k kᵀ/(λ + rᵀk))/λ.
    fn rls(&mut self, u: f32) {
        let r = &self.x[..GEN_N];
        let n = GEN_N;
        for i in 0..n {
            let row = &self.p[i * n..(i + 1) * n];
            self.pk[i] = row.iter().zip(r).map(|(a, b)| a * f64::from(*b)).sum();
        }
        let rk: f64 = r.iter().zip(&self.pk).map(|(a, b)| f64::from(*a) * b).sum();
        let c = 1.0 / (GEN_LAMBDA + rk);
        let guess: f32 = self.gen.iter().zip(r).map(|(w, x)| w * x).sum();
        let e = f64::from(u - guess);
        for (w, k) in self.gen.iter_mut().zip(&self.pk) {
            *w += (e * k * c) as f32;
        }
        let inv_l = 1.0 / GEN_LAMBDA;
        let mut trace = 0.0;
        for i in 0..n {
            let ki = self.pk[i] * c;
            for j in 0..n {
                let v = &mut self.p[i * n + j];
                *v = (*v - ki * self.pk[j]) * inv_l;
            }
            trace += self.p[i * n + i];
        }
        if !(trace.is_finite() && trace < GEN_TRACE_MAX) {
            reset_p(&mut self.p);
        }
    }

    /// The first `n` neurons' activity, for the face.
    pub(super) fn neuron(&self, i: usize) -> f32 {
        self.x[i]
    }
}

