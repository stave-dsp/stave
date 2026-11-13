//! **LSTM** — a stacked long short-term memory network with a linear
//! head, the recurrent architecture Neural Amp Modeler trains.

use super::activation::{sigmoid_in_place, tanh_in_place, tanh_into};
use super::error::{LoadError, Weights};
use super::rate::{RateGuard, RateMismatch, RatePolicy};
use crate::kernel::node::{Stage, Tail};

/// A stacked LSTM (Hochreiter & Schmidhuber 1997; for amplifier capture
/// Wright, Damskägg & Välimäki 2019) with a linear head, one sample per
/// tick.
///
/// # Models
///
/// Whatever it was trained on (see [`WaveNet`](super::WaveNet)): a
/// device captured by input/output recordings.
///
/// Each layer, PyTorch's `nn.LSTM` cell with gates in PyTorch's order
/// (input, forget, cell, output):
///
/// ```text
/// [i f g o] = W · [x, h] + b
/// c' = σ(f)·c + σ(i)·tanh(g)
/// h' = σ(o)·tanh(c')
/// ```
///
/// then `y = w · h_last + b`.
///
/// # Derivation
///
/// Trained out of tree (Neural Amp Modeler) and loaded from its `.nam`
/// file by [`nam::parse`](super::nam::parse). The file stores each
/// layer's starting state (the trainer's state after a second of
/// silence); [`reset`](Stage::reset) returns to it.
///
/// # Verified
///
/// A two-layer, five-unit `.nam` exported by Neural Amp Modeler 0.13,
/// run against the trainer's forward pass from the same file
/// (`crates/stave-dsp/tests/nam_parity.rs::lstm_matches_the_trainer`),
/// and random shapes against an independent f64 forward pass
/// (`crates/stave-dsp/tests/nam_parity.rs::random_recurrent_networks_match_the_f64_reference`).
/// Budget: max abs error ≤ 1e-5 (measured 1.2e-7 against the trainer).
///
/// # Cost
///
/// `4H(I + H)` multiply-adds and `5H` transcendental functions per
/// layer per sample; one 16-unit layer is datasheet `"LSTM (1 x 16)"`.
///
/// # Aliasing
///
/// None added beyond what the network learned at its trained rate.
///
/// # Valid ranges
///
/// Input in the range it was trained on. Sample rate: the trained rate,
/// [`sample_rate`](Self::sample_rate); at any other the stage outputs
/// silence unless [`RatePolicy::Any`] is chosen.
#[derive(Clone, Debug)]
pub struct Lstm {
    cells: Vec<Cell>,
    head: Vec<f32>,
    head_bias: f32,
    rate: RateGuard,
}

#[derive(Clone, Debug)]
struct Cell {
    inputs: usize,
    hidden: usize,
    /// `[input + hidden][4·hidden]`: row `j` is what `[x, h][j]` adds
    /// to each gate.
    weights: Vec<f32>,
    bias: Vec<f32>,
    h0: Vec<f32>,
    c0: Vec<f32>,
    h: Vec<f32>,
    c: Vec<f32>,
    gates: Vec<f32>,
}

impl Lstm {
    /// Build from the `.nam` weight list: per layer the gate matrix
    /// `(4H, I + H)` row-major, the summed bias `(4H)`, the starting
    /// hidden and cell states `(H)` each; then the head weight `(H)` and
    /// bias.
    pub(crate) fn build(
        input_size: usize,
        hidden_size: usize,
        layers: usize,
        weights: &[f32],
        sample_rate: f32,
    ) -> Result<Self, LoadError> {
        if input_size != 1 {
            return Err(LoadError::Unsupported(format!(
                "LSTM input_size {input_size} (mono networks read 1)"
            )));
        }
        if hidden_size == 0 || layers == 0 {
            return Err(LoadError::Config(
                "an LSTM needs at least one layer of at least one unit".into(),
            ));
        }
        let mut w = Weights::new(weights);
        let cells = (0..layers)
            .map(|l| {
                let inputs = if l == 0 { input_size } else { hidden_size };
                Cell::read(&mut w, inputs, hidden_size)
            })
            .collect();
        let head = w.take(hidden_size).collect();
        let head_bias = w.one();
        w.finish()?;
        Ok(Self {
            cells,
            head,
            head_bias,
            rate: RateGuard::new(sample_rate),
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
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        self.rate.set_policy(policy);
    }
}

impl Cell {
    fn read(w: &mut Weights<'_>, inputs: usize, hidden: usize) -> Self {
        let cols = inputs + hidden;
        let torch: Vec<f32> = w.take(4 * hidden * cols).collect();
        let mut weights = vec![0.0; torch.len()];
        for r in 0..4 * hidden {
            for j in 0..cols {
                weights[j * 4 * hidden + r] = torch[r * cols + j];
            }
        }
        let bias = w.take(4 * hidden).collect();
        let h0: Vec<f32> = w.take(hidden).collect();
        let c0: Vec<f32> = w.take(hidden).collect();
        Self {
            inputs,
            hidden,
            weights,
            bias,
            h: h0.clone(),
            c: c0.clone(),
            h0,
            c0,
            gates: vec![0.0; 4 * hidden],
        }
    }

    #[inline]
    fn step(&mut self, x: &[f32]) {
        let n = 4 * self.hidden;
        self.gates.copy_from_slice(&self.bias);
        let inputs = x[..self.inputs].iter().chain(&self.h);
        for (row, &v) in self.weights.chunks_exact(n).zip(inputs) {
            for (g, &w) in self.gates.iter_mut().zip(row) {
                *g += w * v;
            }
        }
        let h = self.hidden;
        let (ifg, o) = self.gates.split_at_mut(3 * h);
        let (i_f, g) = ifg.split_at_mut(2 * h);
        sigmoid_in_place(i_f);
        sigmoid_in_place(o);
        tanh_in_place(g);
        let (i, f) = i_f.split_at(h);
        for k in 0..h {
            self.c[k] = f[k] * self.c[k] + i[k] * g[k];
        }
        tanh_into(&self.c, &mut self.h);
        for (hk, &ok) in self.h.iter_mut().zip(&*o) {
            *hk *= ok;
        }
    }
}

impl Stage for Lstm {
    /// Record the rate (see [`RatePolicy`]) and return to the starting
    /// state.
    fn prepare(&mut self, sample_rate: f32) {
        self.rate.prepare(sample_rate);
        self.reset();
    }

    /// Return every layer to the file's starting state.
    fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.h.copy_from_slice(&cell.h0);
            cell.c.copy_from_slice(&cell.c0);
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.rate.muted() {
            return 0.0;
        }
        self.cells[0].step(&[x]);
        for l in 1..self.cells.len() {
            let (done, rest) = self.cells.split_at_mut(l);
            rest[0].step(&done[l - 1].h);
        }
        let h = &self.cells[self.cells.len() - 1].h;
        let mut y = self.head_bias;
        for (w, v) in self.head.iter().zip(h) {
            y += w * v;
        }
        y
    }

    /// Recurrent: the state need not ever settle, so the honest bound
    /// is none.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

#[cfg(test)]
mod tests;
