//! The frequency domain: one [`Fft`] for the workspace, the
//! partitioned [`Convolver`] built on it, and the [`ImpulseResponse`] it
//! convolves with. STFT and FIR design land here next (DSP.md F5, F6).
//!
//! Everything plans once, where allocation is allowed (a constructor or
//! `prepare`), and runs in place afterwards: no transform or convolution
//! call allocates.

pub mod convolve;
pub mod fft;
pub mod impulse_response;

pub use convolve::{Convolver, Head};
pub use fft::{Complex, Fft, FftFloat};
pub use impulse_response::{ImpulseResponse, IrLayout};
