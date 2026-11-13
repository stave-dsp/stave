//! **Stft** — the short-time Fourier transform as a [`Stage`]: analysis,
//! a closure over each spectrum, and synthesis by weighted overlap-add.
//!
//! Every `hop` samples the last `size` samples are windowed, transformed
//! with a real [`Fft`], handed to the closure as `size/2 + 1` bins, then
//! transformed back, windowed again and overlap-added into the output.
//! With the window applied twice, reconstruction is perfect exactly when
//! the squared window overlap-adds to a constant at that hop (the COLA
//! condition for weighted overlap-add); the constructor checks it and
//! folds the constant into the synthesis scale, so an identity closure
//! returns the input, delayed by `size` samples.
//!
//! The spectral effects (a phase vocoder, DSP.md S6; denoisers, spectral
//! gates and freezes) are closures over this stage.

use super::fft::{Complex, Fft};
use super::window::Window;
use crate::kernel::node::{Stage, Tail};
use core::fmt;

/// Why an [`Stft`] could not be built.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StftError {
    /// The transform size must be a power of two of at least 2.
    Size(usize),
    /// The hop must be from 1 to the size.
    Hop {
        /// The size asked for.
        size: usize,
        /// The hop asked for.
        hop: usize,
    },
    /// The squared window does not overlap-add to a constant at this
    /// hop, so the output would be modulated by the hop rate.
    NotCola {
        /// The window asked for.
        window: Window,
        /// The size asked for.
        size: usize,
        /// The hop asked for.
        hop: usize,
    },
}

impl fmt::Display for StftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StftError::Size(n) => write!(f, "STFT size {n} is not a power of two of at least 2"),
            StftError::Hop { size, hop } => {
                write!(f, "STFT hop {hop} is not between 1 and the size {size}")
            }
            StftError::NotCola { window, size, hop } => write!(
                f,
                "{window:?} squared does not overlap-add to a constant at size {size}, hop {hop}"
            ),
        }
    }
}

impl std::error::Error for StftError {}

impl Window {
    /// The constant the squared periodic window of `size` overlap-adds
    /// to at `hop` (the window applied at analysis and again at
    /// synthesis), or `None` when the sum varies by more than 1e-9 of
    /// itself: the weighted overlap-add COLA condition. The constant is
    /// `Σ_k w²(n + k·hop)`: 1 for `SqrtHann` at half the size, 1.5 for
    /// `Hann` at a quarter.
    ///
    /// ```
    /// use stave_dsp::primitives::spectral::Window;
    ///
    /// assert_eq!(Window::SqrtHann.overlap_add_gain(512, 256).map(f64::round), Some(1.0));
    /// assert!(Window::Hann.overlap_add_gain(512, 128).is_some()); // Hann² at 75 %
    /// assert!(Window::Hann.overlap_add_gain(512, 256).is_none()); // … not at 50 %
    /// ```
    pub fn overlap_add_gain(self, size: usize, hop: usize) -> Option<f64> {
        if size == 0 || hop == 0 || hop > size {
            return None;
        }
        let w = self.periodic(size);
        let sums: Vec<f64> = (0..hop)
            .map(|n| (n..size).step_by(hop).map(|i| w[i] * w[i]).sum())
            .collect();
        let (lo, hi) = sums
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        (hi > 0.0 && hi - lo <= 1e-9 * hi).then_some(0.5 * (hi + lo))
    }
}

/// The short-time Fourier transform as a [`Stage`]: windowed analysis
/// every `hop` samples, a closure over each spectrum, and weighted
/// overlap-add synthesis. Plans everything at construction; nothing
/// allocates after.
///
/// # Models
///
/// `y = ISTFT(process(STFT(x)))`: with an identity closure, `y[t] =
/// x[t − size]` exactly (to `f32` rounding) for every window and hop
/// that pass the COLA check. The closure sees the `size/2 + 1` bins of
/// the forward transform (DC first, Nyquist last, unnormalised as
/// [`Fft::forward_real`] leaves them); what it changes is circular
/// within a frame, the usual STFT caveat (a filter longer than the
/// window's zero padding wraps).
///
/// # Derivation
///
/// Weighted overlap-add (Crochiere, *A weighted overlap-add method of
/// short-time Fourier analysis/synthesis*, IEEE TASSP 1980; Allen &
/// Rabiner 1977), the analysis window reused for synthesis, the COLA
/// constant of its square folded into the synthesis scale.
///
/// # Verified
///
/// `crates/stave-dsp/tests/stft.rs`: perfect reconstruction for all 132
/// window, size (16 to 1024) and hop combinations that
/// [`Window::overlap_add_gain`] claims COLA, 2.4e-7 of full scale at 75 %
/// overlap and at most 0.35 of the budget `1e-6 + 4e-8·size/hop` (f32
/// rounding grows with the overlapping frames); the claims match the
/// theory (a squared window with cosine terms up to `m` is COLA at
/// `size/R` for `R > m`); a gain or a bin mask in the closure acts as one
/// on the output.
///
/// # Cost
///
/// `tests/datasheet.rs`, "Stft": per sample, one forward and one
/// inverse real transform of `size` every `hop` samples plus the window
/// twice: on an Apple M-series core about 24 ns for 1024/256 and 28
/// for 4096/1024 (Hann, an identity closure).
///
/// # Aliasing
///
/// None with an identity closure. A closure that changes the spectrum
/// is a time-varying filter; its time aliasing is the closure's
/// business (zero-pad, or keep changes smooth across bins).
///
/// # Valid ranges
///
/// Sizes `2..=2^31`, powers of two; hops `1..=size`, COLA for the
/// window. Latency `size` samples, reported.
///
/// ```
/// use stave_dsp::primitives::spectral::{Stft, Window};
/// use stave_dsp::Stage;
///
/// // Halve every bin: the output is the input at half level, `size` late.
/// let mut stft = Stft::new(64, 16, Window::Hann, |bins| {
///     bins.iter_mut().for_each(|z| *z = z.scale(0.5));
/// })?;
/// stft.prepare(48_000.0);
/// assert_eq!(stft.latency(), 64);
/// let x: Vec<f32> = (0..400).map(|n| (n as f32 * 0.05).sin()).collect();
/// let y: Vec<f32> = x.iter().map(|&s| stft.tick(s)).collect();
/// assert!((64..400).all(|t| (y[t] - 0.5 * x[t - 64]).abs() < 1e-5));
/// # Ok::<(), stave_dsp::primitives::spectral::StftError>(())
/// ```
#[derive(Clone)]
pub struct Stft<F> {
    size: usize,
    hop: usize,
    window: Window,
    /// The periodic window, `size` weights.
    weights: Vec<f32>,
    /// `1 / (size · Σw²)`: the inverse transform's `n` and the COLA
    /// constant, folded into the synthesis window.
    scale: f32,
    fft: Fft<f32>,
    /// The last `size` input samples; the newest hop fills its tail.
    input: Vec<f32>,
    /// The overlap-add accumulator; its first `hop` samples are played
    /// out during the current hop.
    output: Vec<f32>,
    /// Scratch: the windowed frame, then the inverse transform.
    frame: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    /// Position in the current hop, `0..hop`.
    pos: usize,
    process: F,
}

impl<F: FnMut(&mut [Complex<f32>])> Stft<F> {
    /// An STFT of `size` samples every `hop`, windowed by `window` at
    /// analysis and synthesis, calling `process` on each spectrum.
    ///
    /// # Errors
    ///
    /// [`StftError::Size`] unless `size` is a power of two of at least 2,
    /// [`StftError::Hop`] unless `1 ≤ hop ≤ size`, and
    /// [`StftError::NotCola`] when the squared window does not
    /// overlap-add to a constant at `hop` ([`Window::overlap_add_gain`]).
    pub fn new(size: usize, hop: usize, window: Window, process: F) -> Result<Self, StftError> {
        if !size.is_power_of_two() || !(2..=1 << 31).contains(&size) {
            return Err(StftError::Size(size));
        }
        if hop == 0 || hop > size {
            return Err(StftError::Hop { size, hop });
        }
        let gain = window
            .overlap_add_gain(size, hop)
            .ok_or(StftError::NotCola { window, size, hop })?;
        Ok(Self {
            size,
            hop,
            window,
            weights: window.periodic(size).iter().map(|&w| w as f32).collect(),
            scale: (1.0 / (size as f64 * gain)) as f32,
            fft: Fft::new(size),
            input: vec![0.0; size],
            output: vec![0.0; size],
            frame: vec![0.0; size],
            spectrum: vec![Complex::default(); size / 2 + 1],
            pos: 0,
            process,
        })
    }

    /// The transform size, in samples.
    pub const fn size(&self) -> usize {
        self.size
    }

    /// The hop between analyses, in samples.
    pub const fn hop(&self) -> usize {
        self.hop
    }

    /// The window, applied at analysis and at synthesis.
    pub const fn window(&self) -> Window {
        self.window
    }

    /// The closure, to change its settings between blocks.
    pub fn process_mut(&mut self) -> &mut F {
        &mut self.process
    }

    /// One analysis–process–synthesis step over the last `size` samples.
    fn step(&mut self) {
        let (size, hop) = (self.size, self.hop);
        for ((f, &x), &w) in self.frame.iter_mut().zip(&self.input).zip(&self.weights) {
            *f = x * w;
        }
        self.fft.forward_real(&self.frame, &mut self.spectrum);
        (self.process)(&mut self.spectrum);
        self.fft.inverse_real(&mut self.spectrum, &mut self.frame);
        self.output.copy_within(hop.., 0);
        self.output[size - hop..].fill(0.0);
        for ((o, &y), &w) in self.output.iter_mut().zip(&self.frame).zip(&self.weights) {
            *o += y * w * self.scale;
        }
        self.input.copy_within(hop.., 0);
    }
}

impl<F: FnMut(&mut [Complex<f32>])> Stage for Stft<F> {
    /// Clears the state; the plan does not depend on the rate.
    fn prepare(&mut self, _sample_rate: f32) {
        self.reset();
    }

    fn reset(&mut self) {
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.pos = 0;
    }

    /// One sample in, one out: the sample `size` ticks ago, as
    /// processed. Realtime: every `hop` ticks runs two transforms of
    /// `size`, never allocates.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.output[self.pos];
        self.input[self.size - self.hop + self.pos] = x;
        self.pos += 1;
        if self.pos == self.hop {
            self.pos = 0;
            self.step();
        }
        y
    }

    fn latency(&self) -> u32 {
        self.size as u32
    }

    /// The latency plus one frame: a closure may spread an input sample
    /// across the whole frame it lands in.
    fn tail(&self) -> Tail {
        Tail::samples(2 * self.size as u32 - 1)
    }
}

impl<F> fmt::Debug for Stft<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stft")
            .field("size", &self.size)
            .field("hop", &self.hop)
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}
