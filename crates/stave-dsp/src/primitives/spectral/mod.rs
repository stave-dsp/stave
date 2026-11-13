//! The frequency domain: one [`Fft`] for the workspace, the
//! partitioned [`Convolver`] built on it, the [`ImpulseResponse`] it
//! convolves with, [`Fir`] design, the [`Stft`] stage, and the
//! [`Window`]s they share.
//!
//! Everything plans once, where allocation is allowed (a constructor or
//! `prepare`), and runs in place afterwards: no transform or convolution
//! call allocates.

mod cepstrum;
pub mod convolve;
pub mod fft;
pub mod fir;
pub mod impulse_response;
pub mod stft;
pub mod window;

pub use convolve::{Convolver, Head};
pub use fft::{Complex, Fft, FftFloat};
pub use fir::{Band, Fir, FrequencySampling, Passband};
pub use impulse_response::{ImpulseResponse, IrError, IrLayout, Normalization};
pub use stft::{Stft, StftError};
pub use window::Window;
