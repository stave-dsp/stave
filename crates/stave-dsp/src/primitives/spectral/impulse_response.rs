//! **ImpulseResponse** — what a [`Convolver`](super::Convolver)
//! convolves with: one path (mono), two (stereo, each lane its own
//! response) or four (true stereo, every input to every output).
//!
//! Built from samples ([`mono`](ImpulseResponse::mono) …) taken at
//! whatever rate the convolver is prepared at, or from a capture's
//! channels and the rate it was recorded at
//! ([`from_channels`](ImpulseResponse::from_channels)), in which case
//! the convolver resamples it to the prepared rate. A WAV file arrives
//! through `stave_host::wav::read_impulse_response`, since file input
//! lives above this crate.

use super::cepstrum;
use super::window::{bessel_i0, Window};
use crate::units::SampleRate;
use core::f64::consts::PI;
use core::fmt;
use crate::units::Db;

/// How an [`ImpulseResponse`]'s paths connect inputs to outputs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IrLayout {
    /// One input, one output, one path.
    Mono,
    /// Two lanes, each convolved with its own response: left → left,
    /// right → right.
    Stereo,
    /// Two inputs, two outputs, four paths: each output is the sum of
    /// both inputs, each through its own response (a stereo room
    /// captured from two source positions).
    TrueStereo,
}

impl IrLayout {
    /// The input (and output) lanes: 1 for mono, 2 otherwise.
    pub const fn lanes(self) -> usize {
        match self {
            IrLayout::Mono => 1,
            IrLayout::Stereo | IrLayout::TrueStereo => 2,
        }
    }
}

/// What [`ImpulseResponse::normalized`] scales to unity. One gain
/// scales every path, so the balance between them is kept.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Normalization {
    /// The largest sample of any path is ±1.
    Peak,
    /// The loudest path's energy `Σh²` is 1: unity gain for white noise
    /// (a reverb or cabinet sits at the level of its dry signal).
    Energy,
}

/// Why channels could not become an [`ImpulseResponse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrError {
    /// Only 1 (mono), 2 (stereo) or 4 (true stereo) channels map onto a
    /// layout.
    Channels(usize),
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrError::Channels(n) => write!(
                f,
                "an impulse response has 1, 2 or 4 channels (mono, stereo, true stereo), not {n}"
            ),
        }
    }
}

impl std::error::Error for IrError {}

/// An impulse response: one, two or four paths of samples, laid out by
/// an [`IrLayout`], and the rate they were captured at when known.
/// Input data for a [`Convolver`](super::Convolver), copied in at
/// construction (which allocates).
///
/// # Models
///
/// A linear time-invariant system by its sampled response: per path,
/// `h[n]` is the output `n` samples after a unit impulse.
///
/// # Derivation
///
/// [`resampled`](Self::resampled): band-limited interpolation through a
/// Kaiser-windowed sinc (100 dB, flat to 0.45 of the lower rate, stopped
/// from 0.5), scaled by `from/to` so the response's gain is kept (a
/// tap is the response integrated over one sample period).
/// [`minimum_phase`](Self::minimum_phase): the real cepstrum, per path
/// (Oppenheim & Schafer, §13.8.1).
///
/// # Verified
///
/// `crates/stave-dsp/tests/impulse_response.rs`: a band-limited
/// analytic response resampled between 44.1, 48, 96 and 192 kHz lands
/// on the analytic response at the new rate within 9e-7 of its peak
/// (budget 1e-4), and its gain at DC and at each tone is kept within
/// 1e-4; minimum phase keeps each path's magnitude within 8e-7 dB
/// (budget 0.01) wherever it is within 60 dB of its peak and never
/// trails the original's energy; channels map onto layouts; the
/// convolver re-plans at `prepare` and returns the resampled response.
///
/// # Cost
///
/// Off the audio thread (loading, resampling and minimum phase
/// allocate): `tests/datasheet.rs`, "ImpulseResponse", on an Apple
/// M-series core: resampling a two-second response from 44.1 to 48 kHz
/// about 250 ns per output sample
/// (26 ms); the minimum phase of 4096 taps about 10 ms.
///
/// # Aliasing
///
/// Resampling down rejects what the new rate cannot carry by 100 dB.
///
/// # Valid ranges
///
/// Any length; sample rates finite and positive.
///
/// ```
/// use stave_dsp::ImpulseResponse;
/// use stave_dsp::primitives::spectral::IrLayout;
///
/// let room = ImpulseResponse::stereo(&[1.0, 0.5, 0.25], &[1.0, -0.5]);
/// assert_eq!(room.layout(), IrLayout::Stereo);
/// assert_eq!(room.len(), 3); // the longest path
/// assert_eq!(room.path(1, 1), Some(&[1.0, -0.5][..]));
/// assert_eq!(room.path(0, 1), None); // stereo has no cross paths
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ImpulseResponse {
    layout: IrLayout,
    /// `paths[input * lanes + output]`, `None` where the layout has no
    /// path.
    paths: Vec<Option<Vec<f32>>>,
    /// The rate the samples were captured at; `None` for samples taken
    /// at whatever rate the convolver is prepared at.
    sample_rate: Option<SampleRate>,
}

impl ImpulseResponse {
    /// One path: the response of a mono system.
    pub fn mono(samples: &[f32]) -> Self {
        Self {
            layout: IrLayout::Mono,
            paths: vec![Some(samples.to_vec())],
            sample_rate: None,
        }
    }

    /// Two independent lanes: `left` convolves the left input into the
    /// left output, `right` the right into the right.
    pub fn stereo(left: &[f32], right: &[f32]) -> Self {
        Self {
            layout: IrLayout::Stereo,
            paths: vec![Some(left.to_vec()), None, None, Some(right.to_vec())],
            sample_rate: None,
        }
    }

    /// Four paths, named source → destination: the left output is
    /// `left_to_left ∗ L + right_to_left ∗ R`, the right output
    /// `left_to_right ∗ L + right_to_right ∗ R`.
    pub fn true_stereo(
        left_to_left: &[f32],
        left_to_right: &[f32],
        right_to_left: &[f32],
        right_to_right: &[f32],
    ) -> Self {
        Self {
            layout: IrLayout::TrueStereo,
            paths: vec![
                Some(left_to_left.to_vec()),
                Some(left_to_right.to_vec()),
                Some(right_to_left.to_vec()),
                Some(right_to_right.to_vec()),
            ],
            sample_rate: None,
        }
    }

    /// A capture's channels, recorded at `sample_rate`: one channel is
    /// mono, two stereo (left, right), four true stereo in the order
    /// left→left, left→right, right→left, right→right (the left source
    /// captured to both microphones, then the right). A
    /// [`Convolver`](super::Convolver) resamples it to the rate it is
    /// prepared at.
    ///
    /// ```
    /// use stave_dsp::ImpulseResponse;
    /// use stave_dsp::primitives::spectral::IrLayout;
    /// use stave_dsp::units::SampleRate;
    ///
    /// let wav_channels: [&[f32]; 2] = [&[1.0, 0.3], &[0.9, 0.4]];
    /// let ir = ImpulseResponse::from_channels(&wav_channels, SampleRate::new(44_100.0))?;
    /// assert_eq!(ir.layout(), IrLayout::Stereo);
    /// assert_eq!(ir.sample_rate(), Some(SampleRate::new(44_100.0)));
    /// let three: [&[f32]; 3] = [&[0.0]; 3];
/// assert!(ImpulseResponse::from_channels(&three, SampleRate::new(48_000.0)).is_err());
    /// # Ok::<(), stave_dsp::primitives::spectral::IrError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`IrError::Channels`] for any count but 1, 2 or 4.
    pub fn from_channels(channels: &[&[f32]], sample_rate: SampleRate) -> Result<Self, IrError> {
        let ir = match *channels {
            [mono] => Self::mono(mono),
            [left, right] => Self::stereo(left, right),
            [ll, lr, rl, rr] => Self::true_stereo(ll, lr, rl, rr),
            _ => return Err(IrError::Channels(channels.len())),
        };
        Ok(ir.recorded_at(sample_rate))
    }

    /// The same samples, marked as captured at `sample_rate` (a
    /// [`Convolver`](super::Convolver) then resamples them to the rate it
    /// is prepared at).
    pub fn recorded_at(mut self, sample_rate: SampleRate) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    /// How the paths connect inputs to outputs.
    pub const fn layout(&self) -> IrLayout {
        self.layout
    }

    /// The rate the samples were captured at, or `None` for samples
    /// taken at the prepared rate as given.
    pub const fn sample_rate(&self) -> Option<SampleRate> {
        self.sample_rate
    }

    /// The longest path, in samples.
    pub fn len(&self) -> usize {
        self.paths.iter().flatten().map(Vec::len).max().unwrap_or(0)
    }

    /// `true` when every path is empty (the response of silence).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The samples from input lane `input` to output lane `output`, or
    /// `None` when the layout has no such path.
    pub fn path(&self, input: usize, output: usize) -> Option<&[f32]> {
        let lanes = self.layout.lanes();
        if input >= lanes || output >= lanes {
            return None;
        }
        self.paths[input * lanes + output].as_deref()
    }

    /// Every path through `f`, keeping the layout and rate.
    fn map(&self, mut f: impl FnMut(&[f32]) -> Vec<f32>) -> Self {
        Self {
            layout: self.layout,
            paths: self
                .paths
                .iter()
                .map(|p| p.as_deref().map(&mut f))
                .collect(),
            sample_rate: self.sample_rate,
        }
    }

    /// The response at `sample_rate`: every path band-limited
    /// interpolated to the new rate and scaled by `from/to`, so the
    /// response's gain (its frequency response, where both rates carry
    /// it) is kept. Samples with no recorded rate are returned as they
    /// are, marked with `sample_rate`.
    pub fn resampled(&self, sample_rate: SampleRate) -> Self {
        let Some(from) = self.sample_rate else {
            return self.clone().recorded_at(sample_rate);
        };
        if from == sample_rate {
            return self.clone();
        }
        let kernel = Kernel::new(f64::from(from.get()), f64::from(sample_rate.get()));
        let mut out = self.map(|h| kernel.apply(h));
        out.sample_rate = Some(sample_rate);
        out
    }

    /// Every path scaled by one gain so that `by` is unity. Silence is
    /// returned as it is.
    pub fn normalized(&self, by: Normalization) -> Self {
        let paths = self.paths.iter().flatten();
        let level = match by {
            Normalization::Peak => paths
                .flat_map(|p| p.iter())
                .map(|&v| f64::from(v).abs())
                .fold(0.0, f64::max),
            Normalization::Energy => paths
                .map(|p| p.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt())
                .fold(0.0, f64::max),
        };
        if level == 0.0 {
            return self.clone();
        }
        let gain = 1.0 / level;
        self.map(|h| h.iter().map(|&v| (f64::from(v) * gain) as f32).collect())
    }

    /// Every path converted to its minimum-phase version (the real
    /// cepstrum): the same magnitude, the least delay, the low-frequency
    /// polarity kept. Each path converts on its own, so the time
    /// differences between the paths of a stereo capture (its image) are
    /// lost: use it for cabinets and filters, not rooms.
    pub fn minimum_phase(&self) -> Self {
        self.map(|h| {
            let h: Vec<f64> = h.iter().map(|&v| f64::from(v)).collect();
            cepstrum::minimum_phase(&h)
                .into_iter()
                .map(|v| v as f32)
                .collect()
        })
    }
}

/// The band-limited interpolation kernel from one rate to another: a
/// Kaiser-windowed sinc (100 dB stopband) with its edge at 0.475 of the
/// lower rate (flat to 0.45, stopped from 0.5), tabulated finely and
/// read with linear interpolation.
struct Kernel {
    /// Source samples per output sample.
    step: f64,
    /// The kernel's half-width, in source samples.
    half: f64,
    /// The kernel at `|t| = i / TABLE` source samples.
    table: Vec<f64>,
    /// `from / to`: the gain that keeps the response's level.
    gain: f64,
}

impl Kernel {
    /// Table points per source sample: linear interpolation then errs
    /// below 1e-7 of the kernel's peak.
    const TABLE: usize = 4096;
    /// The stopband, in dB.
    const ATTENUATION: f64 = 100.0;

    fn new(from: f64, to: f64) -> Self {
        // In source samples: the lower rate as a fraction of the source.
        let scale = (to / from).min(1.0);
        let cutoff = 0.475 * scale;
        let transition = 0.05 * scale;
        let half = ((Self::ATTENUATION - 7.95) / (14.36 * transition) / 2.0).ceil();
        let Window::Kaiser { beta } = Window::kaiser_for(Db::new(Self::ATTENUATION as f32)) else {
            unreachable!("kaiser_for returns a Kaiser window")
        };
        let inv_i0 = 1.0 / bessel_i0(beta);
        let points = (half * Self::TABLE as f64) as usize + 2;
        let table = (0..points)
            .map(|i| {
                let t = i as f64 / Self::TABLE as f64;
                let ideal = if t == 0.0 {
                    2.0 * cutoff
                } else {
                    (2.0 * PI * cutoff * t).sin() / (PI * t)
                };
                let r = (t / half).min(1.0);
                ideal * bessel_i0(beta * (1.0 - r * r).sqrt()) * inv_i0
            })
            .collect();
        Self {
            step: from / to,
            half,
            table,
            gain: from / to,
        }
    }

    #[inline]
    fn at(&self, t: f64) -> f64 {
        let x = t.abs() * Self::TABLE as f64;
        let i = x as usize;
        if i + 1 >= self.table.len() {
            return 0.0;
        }
        let frac = x - i as f64;
        self.table[i] + (self.table[i + 1] - self.table[i]) * frac
    }

    /// `h` at the new rate: `ceil(len · to/from)` samples.
    fn apply(&self, h: &[f32]) -> Vec<f32> {
        let len = (h.len() as f64 / self.step).ceil() as usize;
        (0..len)
            .map(|m| {
                let t = m as f64 * self.step;
                let first = (t - self.half).ceil().max(0.0) as usize;
                let last = ((t + self.half).floor() as usize).min(h.len().saturating_sub(1));
                let sum: f64 = (first..=last.max(first))
                    .take_while(|&n| n < h.len())
                    .map(|n| f64::from(h[n]) * self.at(t - n as f64))
                    .sum();
                (sum * self.gain) as f32
            })
            .collect()
    }
}
