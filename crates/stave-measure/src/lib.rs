//! Measurement: pure functions over buffers. Nothing here knows about
//! plugins, hosts or threads — feed it samples (or a sample-by-sample
//! system), get numbers back. The host's bench and validation suite, the
//! plugin test suites, and the DSP conformance suites all measure through here.
//!
//! ```text
//! signal      stimuli as pure functions of (channel, frame)
//! level       dB, peak, RMS, DC, correlation, a streaming meter
//! loudness    level measures as strategies: peak, true peak, RMS, LUFS
//! stats       order statistics (timings)
//! system      what measurements drive: closures, plugins, DSP modules
//! spectrum    FFT and Goertzel
//! tone        gain, THD, THD+N of a coherent tone
//! latency     onset + peak of an impulse response
//! transfer    H(f) from input/output (H1, Welch-averaged)
//! compare     bitwise divergence; null-test residuals
//! tone_window   a bin-centred tone (or two) through a system: alias, gain, IMD
//! nonlinear   alias floor, gain surfaces, hysteresis
//! dynamics    gain trajectories, time constants, residuals, decay
//! modulation  sidebands, subharmonics, period multiplication
//! zipper      parameter-ramp spray
//! tail        reverb-tail metrics
//! window      analysis windows as data (Hann, Blackman-Harris, flat-top…)
//! ```
//!
//! Every metric here states its assumptions, because a wrong assumption
//! produces a confident wrong number:
//!
//! - **Bin-centred tones.** Test tones are generated at exactly `m` cycles
//!   per `n`-sample window, so a steady-state (or periodically-excited
//!   LTI/nonlinear-memoryless) system produces output that is *exactly
//!   periodic in n*: harmonics land exactly on bins `k·m`, alias products
//!   land exactly on other integer bins, and no window is needed for
//!   bin classification. Measuring at arbitrary frequencies without a
//!   window leaks the (strong) harmonics into every bin and swamps a
//!   −80 dB floor with −40 dB of leakage.
//! - **Warm-up.** The capture starts only after `warm` samples so filter
//!   group delay, DC blockers (a 5 Hz blocker settles in ~100 ms!),
//!   envelope followers, and memory states reach steady state. Pass a
//!   warm-up of at least one window *and* longer than the slowest time
//!   constant in the path.
//! - **Latency-immune.** All spectral metrics are magnitude-based on
//!   steady-state periodic signals, so fixed group delay does not bias
//!   them (a delayed periodic signal has the same magnitude spectrum).
//! - **Audible-band classification.** Half-band decimators intentionally
//!   leave a transition shoulder that folds only onto the inaudible top
//!   (20 kHz..Nyquist). A full-band alias number therefore *over-counts
//!   by design*; the enforceable claim is the alias floor **below
//!   20 kHz**. [`AliasReport`](nonlinear::AliasReport) carries both so the trade is visible, and
//!   assertions bind the audible figure.

#![forbid(unsafe_code)]

/// The audible band's conventional edges.
pub const AUDIBLE_MIN_HZ: f64 = 20.0;
pub const AUDIBLE_MAX_HZ: f64 = 20_000.0;

pub mod compare;
pub mod dynamics;
pub mod latency;
pub mod level;
pub mod loudness;
pub mod modulation;
pub mod nonlinear;
pub mod signal;
pub mod spectrum;
pub mod stats;
pub mod step;
pub mod stereo;
pub mod system;
pub mod tail;
pub mod tone;
pub mod tone_window;
pub mod transfer;
pub mod waveform;
pub mod window;
pub mod zipper;
