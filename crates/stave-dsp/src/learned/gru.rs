//! **GRU** — a stacked gated recurrent unit network with a linear head.

use super::activation::{sigmoid_in_place, tanh_in_place};
use super::error::{LoadError, Weights};
use super::json::Json;
use super::rate::{RateGuard, RateMismatch, RatePolicy, StateDelay};
use crate::kernel::node::{Stage, Tail};

/// A stacked GRU (Cho et al. 2014) with a linear head, one sample per
/// tick: the lighter recurrent cell several capture tools train in
/// place of an LSTM.
///
/// # Models
///
/// Whatever it was trained on: a device captured by input/output
/// recordings. Each layer is PyTorch's `nn.GRU` cell, gates in its
/// order (reset, update, new):
///
/// ```text
/// r  = σ(W_ir x + b_ir + W_hr h + b_hr)
/// z  = σ(W_iz x + b_iz + W_hz h + b_hz)
/// n  = tanh(W_in x + b_in + r·(W_hn h + b_hn))
/// h' = (1 − z)·n + z·h
/// ```
///
/// then `y = w · h_last + b`. The state starts at zero.
///
/// # Derivation
///
/// Trained out of tree in PyTorch; loaded by [`Gru::parse`] from JSON
/// holding PyTorch's tensors flat (Neural Amp Modeler has no GRU, so
/// there is no `.nam` form).
///
/// # Verified
///
/// A two-layer, four-unit `torch.nn.GRU` with a linear head against
/// PyTorch's forward pass
/// (`crates/stave-dsp/tests/nam_parity.rs::gru_matches_torch`), and
/// random shapes against an independent f64 forward pass
/// (`crates/stave-dsp/tests/nam_parity.rs::random_recurrent_networks_match_the_f64_reference`).
/// Budget: max abs error ≤ 1e-5 (measured 3.6e-7 against PyTorch);
/// `scripts/nam-reference.py` writes the reference.
///
/// # Cost
///
/// `3H(I + H)` multiply-adds and `3H` transcendental functions per
/// layer per sample; one 16-unit layer is datasheet `"GRU (1 x 16)"`.
/// [`RatePolicy::Interpolated`] adds four multiply-adds per state float
/// (the cubic read) and a copy: datasheet
/// `"GRU (1 x 16) Interpolated 48 -> 44.1 kHz"`.
///
/// # Aliasing
///
/// None added beyond what the network learned at its trained rate.
/// Measured (`crates/stave-dsp/tests/aliasing_budgets.rs::learned_stages_hold_their_alias_budgets`,
/// a stepped tone 500 Hz-12 kHz at 0.5, worst inharmonic energy at or
/// below 20 kHz re the fundamental, 44.1/48 kHz): -32.4/-35.3 dB (the 2 x 4 fixture), -46.9/-48.9 (a random 1 x 16).
///
/// # Valid ranges
///
/// Input in the range it was trained on. Sample rate: the trained rate,
/// [`sample_rate`](Self::sample_rate); at any other the stage outputs
/// silence unless [`RatePolicy::Any`] is chosen, or
/// [`RatePolicy::Interpolated`], which keeps the trained time constants
/// from half to sixteen times the trained rate
/// (`crates/stave-dsp/tests/learned_rates.rs::recurrent_networks_run_at_any_rate`:
/// against the network at its trained rate, resampled, the harmonics
/// agree to 47-58 dB at 44.1 kHz, 75-86 dB at 88.2 kHz and 122-139 dB
/// at 96 and 192 kHz for a 48 kHz network; whole ratios interleave the
/// trained recursion bit for bit,
/// `crates/stave-dsp/tests/learned_rates.rs::whole_ratios_interleave_the_trained_recursion`).
#[derive(Clone, Debug)]
pub struct Gru {
    cells: Vec<Cell>,
    head: Vec<f32>,
    head_bias: f32,
    rate: RateGuard,
    /// Past states, for [`RatePolicy::Interpolated`] (empty until chosen).
    delay: StateDelay,
}

#[derive(Clone, Debug)]
struct Cell {
    inputs: usize,
    hidden: usize,
    /// `[input][3·hidden]` and `[hidden][3·hidden]`, axpy rows.
    w_ih: Vec<f32>,
    w_hh: Vec<f32>,
    b_ih: Vec<f32>,
    b_hh: Vec<f32>,
    h: Vec<f32>,
    gi: Vec<f32>,
    gh: Vec<f32>,
}

impl Gru {
    /// Read a network from JSON:
    ///
    /// ```json
    /// {"input_size": 1, "hidden_size": 16, "num_layers": 1,
    ///  "sample_rate": 48000, "weights": [...]}
    /// ```
    ///
    /// `weights` is, per layer, PyTorch's `weight_ih_l{k}` `(3H, I)`,
    /// `weight_hh_l{k}` `(3H, H)`, `bias_ih_l{k}` and `bias_hh_l{k}`
    /// `(3H)` flattened row-major, then the head's weight `(H)` and bias.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        let json = Json::parse(text).map_err(|e| LoadError::Json {
            offset: e.offset,
            reason: e.reason,
        })?;
        let size = |key: &str| {
            json.field(key)
                .and_then(Json::as_f64)
                .filter(|n| n.fract() == 0.0 && *n >= 0.0)
                .map(|n| n as usize)
                .ok_or_else(|| LoadError::Config(format!("\"{key}\" must be a whole number")))
        };
        let (input_size, hidden_size, layers) = (
            size("input_size")?,
            size("hidden_size")?,
            size("num_layers")?,
        );
        let sample_rate = json
            .field("sample_rate")
            .and_then(Json::as_f64)
            .filter(|r| *r > 0.0)
            .ok_or_else(|| LoadError::Config("\"sample_rate\" must be a positive number".into()))?;
        let weights = super::nam::weights(&json)?;
        Self::build(
            input_size,
            hidden_size,
            layers,
            &weights,
            sample_rate as f32,
        )
    }

    fn build(
        input_size: usize,
        hidden_size: usize,
        layers: usize,
        weights: &[f32],
        sample_rate: f32,
    ) -> Result<Self, LoadError> {
        if input_size != 1 {
            return Err(LoadError::Unsupported(format!(
                "GRU input_size {input_size} (mono networks read 1)"
            )));
        }
        if hidden_size == 0 || layers == 0 {
            return Err(LoadError::Config(
                "a GRU needs at least one layer of at least one unit".into(),
            ));
        }
        let mut w = Weights::new(weights);
        let cells = (0..layers)
            .map(|l| {
                Cell::read(
                    &mut w,
                    if l == 0 { input_size } else { hidden_size },
                    hidden_size,
                )
            })
            .collect();
        let head = w.take(hidden_size).collect();
        let head_bias = w.one();
        w.finish()?;
        Ok(Self {
            cells,
            head,
            head_bias,
            rate: RateGuard::recurrent(sample_rate),
            delay: StateDelay::default(),
        })
    }

    /// The sample rate the network was trained at, in Hz.
    pub fn sample_rate(&self) -> f32 {
        self.rate.trained()
    }

    /// The trained and prepared rates, when they differ.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        self.rate.mismatch()
    }

    /// What to do at a rate other than the trained one (default
    /// [`RatePolicy::Strict`]: silence).
    ///
    /// [`RatePolicy::Interpolated`] allocates its delay line of past
    /// states the first time it is chosen: call this off the audio
    /// thread, before or after [`prepare`](Stage::prepare).
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        self.rate.set_policy(policy);
        if policy == RatePolicy::Interpolated && !self.delay.allocated() {
            let width = self.cells.iter().map(|c| c.hidden).sum();
            self.delay = StateDelay::new(width);
        }
        self.configure_delay();
    }

    /// Multiply-adds per sample: the gate matrices and the head (the
    /// transcendentals are extra).
    pub fn multiply_adds(&self) -> usize {
        self.cells
            .iter()
            .map(|c| 3 * c.hidden * (c.inputs + c.hidden))
            .sum::<usize>()
            + self.head.len()
    }

    /// The policy chosen by [`set_rate_policy`](Self::set_rate_policy).
    pub fn rate_policy(&self) -> RatePolicy {
        self.rate.policy()
    }

    /// Point the delay line at the prepared rate and fill it with the
    /// resting state.
    fn configure_delay(&mut self) {
        self.delay
            .configure(self.rate.delay(), |slot| slot.fill(0.0));
    }
}

/// PyTorch's `(rows, cols)` row-major matrix as axpy rows `[col][row]`.
fn transpose(w: &mut Weights<'_>, rows: usize, cols: usize) -> Vec<f32> {
    let torch: Vec<f32> = w.take(rows * cols).collect();
    let mut out = vec![0.0; torch.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = torch[r * cols + c];
        }
    }
    out
}

impl Cell {
    fn read(w: &mut Weights<'_>, inputs: usize, hidden: usize) -> Self {
        let w_ih = transpose(w, 3 * hidden, inputs);
        let w_hh = transpose(w, 3 * hidden, hidden);
        let b_ih = w.take(3 * hidden).collect();
        let b_hh = w.take(3 * hidden).collect();
        Self {
            inputs,
            hidden,
            w_ih,
            w_hh,
            b_ih,
            b_hh,
            h: vec![0.0; hidden],
            gi: vec![0.0; 3 * hidden],
            gh: vec![0.0; 3 * hidden],
        }
    }

    /// Take the delayed state (snapshot floats from `at`) as the last one.
    #[inline]
    fn recall(&mut self, delay: &StateDelay, at: usize) {
        delay.read(at, &mut self.h);
    }

    /// Store the new state (snapshot floats from `at`).
    #[inline]
    fn store(&self, delay: &mut StateDelay, at: usize) {
        delay.write(at, &self.h);
    }

    #[inline]
    fn step(&mut self, x: &[f32]) {
        let n = 3 * self.hidden;
        self.gi.copy_from_slice(&self.b_ih);
        for (row, &v) in self.w_ih.chunks_exact(n).zip(&x[..self.inputs]) {
            for (g, &w) in self.gi.iter_mut().zip(row) {
                *g += w * v;
            }
        }
        self.gh.copy_from_slice(&self.b_hh);
        for (row, &v) in self.w_hh.chunks_exact(n).zip(&self.h) {
            for (g, &w) in self.gh.iter_mut().zip(row) {
                *g += w * v;
            }
        }
        let h = self.hidden;
        // r and z: σ(gi + gh), in gi's first two thirds.
        let (rz, n) = self.gi.split_at_mut(2 * h);
        for (a, &b) in rz.iter_mut().zip(&self.gh) {
            *a += b;
        }
        sigmoid_in_place(rz);
        // n = tanh(gi_n + r·gh_n).
        for k in 0..h {
            n[k] += rz[k] * self.gh[2 * h + k];
        }
        tanh_in_place(n);
        for k in 0..h {
            let z = rz[h + k];
            self.h[k] = (1.0 - z) * n[k] + z * self.h[k];
        }
    }
}

impl Stage for Gru {
    /// Record the rate (see [`RatePolicy`]) and zero the state.
    fn prepare(&mut self, sample_rate: f32) {
        self.rate.prepare(sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.h.fill(0.0);
        }
        self.configure_delay();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.rate.muted() {
            return 0.0;
        }
        let delayed = self.delay.active();
        let mut at = 0;
        for l in 0..self.cells.len() {
            let (done, rest) = self.cells.split_at_mut(l);
            let cell = &mut rest[0];
            if delayed {
                cell.recall(&self.delay, at);
            }
            match done.last() {
                None => cell.step(&[x]),
                Some(prev) => cell.step(&prev.h),
            }
            if delayed {
                cell.store(&mut self.delay, at);
            }
            at += cell.hidden;
        }
        if delayed {
            self.delay.advance();
        }
        let h = &self.cells[self.cells.len() - 1].h;
        let mut y = self.head_bias;
        for (w, v) in self.head.iter().zip(h) {
            y += w * v;
        }
        y
    }

    /// Recurrent: the state need not ever settle.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}
