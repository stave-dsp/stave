//! **Layer 6 — learned.** Models trained from captures: WaveNet, LSTM
//! and GRU networks, Wiener–Hammerstein blocks.
//!
//! *Claim:* inference reproduces the reference implementation on the
//! same weights and input, and the trained model reproduces its capture
//! within the error it was trained to.
//!
//! *Verified by:* parity against the reference implementation's output
//! (bit-close, with a budget) and the capture error on held-out audio.
//!
//! Empty until its first model lands (DSP.md M1–M3). It may use every
//! layer above it.
