//! The frequency domain: one [`Fft`] for the workspace, the
//! partitioned [`Convolver`] built on it, the [`ImpulseResponse`] it
//! convolves with, [`Fir`] design and the [`Window`]s it shares. The
//! STFT lands here next (DSP.md F5).
//!
//! Everything plans once, where allocation is allowed (a constructor or
//! `prepare`), and runs in place afterwards: no transform or convolution
//! call allocates.

mod cepstrum;
pub mod convolve;
pub mod fft;
pub mod fir;
pub mod impulse_response;
pub mod window;

pub use convolve::{Convolver, Head};
pub use fft::{Complex, Fft, FftFloat};
pub use fir::{Band, Fir, Passband};
pub use impulse_response::{ImpulseResponse, IrLayout};
pub use window::Window;
