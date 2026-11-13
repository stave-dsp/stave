//! The frequency domain: one [`Fft`] for the workspace. Convolution,
//! STFT and FIR design land here next (DSP.md F2, F5, F6).
//!
//! Everything plans once, where allocation is allowed (a constructor or
//! `prepare`), and runs in place afterwards: no transform allocates.

pub mod fft;

pub use fft::{Complex, Fft, FftFloat};
