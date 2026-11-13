//! **Layer 6 — learned.** Models trained from captures: a network that
//! learned a device from recordings of its input and output.
//!
//! *Claim:* inference reproduces the reference implementation on the
//! same weights and input, and the trained model reproduces its capture
//! within the error it was trained to.
//!
//! *Verified by:* parity against the reference implementation's output
//! (bit-close, with a budget: `tests/nam_parity.rs`) and the capture
//! error on held-out audio.
//!
//! * [`WaveNet`] — dilated causal convolutions with gated activations
//!   (Neural Amp Modeler's default architecture).
//! * [`Lstm`] — a stacked LSTM with a linear head.
//! * [`Gru`] — a stacked GRU with a linear head.
//! * [`nam`] — Neural Amp Modeler's `.nam` file → a [`nam::Network`].
//! * [`block`] — Wiener, Hammerstein and Wiener–Hammerstein models and
//!   the parallel (polynomial) Hammerstein model, and their file.
//!
//! Every network is a [`Stage`](crate::Stage): f32, one sample per
//! tick, no allocation, locks or unbounded loops after it is built (the
//! loader allocates; load off the audio thread and hand the network
//! over). Each knows the rate it was trained at and refuses another
//! unless told otherwise ([`RatePolicy`]): resample to the network,
//! never the reverse. Training stays in Python, out of tree; inference
//! is here. Block-oriented models ([`block`]: Wiener, Hammerstein,
//! Wiener–Hammerstein, parallel Hammerstein) are fitted in Rust
//! (`stave_measure::identify`) and keep their filters' analog
//! prototypes at any rate. It may use every layer above it.
//!
//! # A recurrent network at the host's rate
//!
//! A WaveNet stays at its trained rate (oversample to it). An LSTM or
//! GRU can follow the host instead: [`RatePolicy::Interpolated`] keeps
//! its learned time constants by reading its state one trained period
//! back (Carson et al. 2024).
//!
//! ```
//! use stave_dsp::learned::{nam, RatePolicy};
//! use stave_dsp::Stage;
//!
//! // One unit, trained at 48 kHz: c' = c + 0.5·tanh(x), y = h.
//! let file = r#"{"version": "0.7.0", "architecture": "LSTM", "sample_rate": 48000,
//!   "config": {"input_size": 1, "hidden_size": 1, "num_layers": 1},
//!   "weights": [0, 0,  0, 0,  1, 0,  0, 0,   0, 9, 0, 9,  0, 0,  1, 0]}"#;
//! let mut amp = nam::parse(file)?.network;
//! amp.prepare(96_000.0);
//! assert_eq!(amp.tick(0.5), 0.0); // strict: silent at a foreign rate
//!
//! amp.set_rate_policy(RatePolicy::Interpolated); // allocates: off the audio thread
//! amp.prepare(96_000.0);
//! assert!(amp.tick(0.5) > 0.0);
//! assert!(amp.rate_mismatch().is_some()); // the rates still differ, knowingly
//! # Ok::<(), stave_dsp::learned::LoadError>(())
//! ```

mod activation;
pub mod block;
mod conv;
mod error;
mod gru;
mod json;
mod lstm;
pub mod nam;
mod rate;
mod wavenet;

pub use error::LoadError;
pub use gru::Gru;
pub use lstm::Lstm;
pub use rate::{RateMismatch, RatePolicy};
pub use wavenet::WaveNet;
