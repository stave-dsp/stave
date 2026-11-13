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
//!
//! Every network is a [`Stage`](crate::Stage): f32, one sample per
//! tick, no allocation, locks or unbounded loops after it is built (the
//! loader allocates; load off the audio thread and hand the network
//! over). Each knows the rate it was trained at and refuses another
//! unless told otherwise ([`RatePolicy`]): resample to the network,
//! never the reverse. Training stays in Python, out of tree; inference
//! is here. Wiener–Hammerstein blocks land here next (DSP.md M3). It
//! may use every layer above it.

mod activation;
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
