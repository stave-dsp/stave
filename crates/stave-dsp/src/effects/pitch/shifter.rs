//! The phase-vocoder pitch shifter: a real-time time stretch by the
//! pitch ratio, read back at that ratio.

use super::vocoder::{Reconstruction, Vocoder};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::interpolators::{Interpolator, Sinc};
use crate::units::{ms, Seconds, Semitones};

/// The window length a vocoder uses at a rate: the power of two nearest
/// `window` in samples (2048 at 44.1 and 48 kHz for the default 40 ms).
pub(super) fn frame_size(window: Seconds, sample_rate: f32) -> usize {
    let n = (window.get() * f64::from(sample_rate)).max(16.0);
    1usize << n.log2().round() as u32
}

/// One analysed frame's place on both time axes.
#[derive(Copy, Clone, Debug, Default)]
struct Anchor {
    /// The frame's centre in input samples.
    centre: f64,
    /// Its start on the stretched axis, before rounding.
    start: f64,
    /// The stretch it was analysed at.
    ratio: f64,
}

/// A phase-vocoder pitch shifter: every partial of a polyphonic signal
/// moved by the same interval, the duration kept.
///
/// # Models
///
/// Time-stretch by the pitch ratio `α` with a phase vocoder, then
/// resample by `α` (Laroche & Dolson 1999, §5): the stretched signal's
/// partials keep their frequencies and coherence, and reading it `α`
/// times faster scales every frequency by `α` exactly. The phase is
/// rebuilt per [`Reconstruction`]: Průša's phase-gradient heap
/// integration by default (both phase derivatives integrated from the
/// strongest coefficients, one frame of look-ahead for the centred time
/// derivative, as the real-time PGHI), or Laroche–Dolson identity phase
/// locking. No hardware claim; formants move with the pitch.
///
/// # Derivation
///
/// Hann-windowed frames of about 40 ms (2048 samples at 44.1 and 48 kHz)
/// on the real [`Fft`](crate::Fft), synthesis hop a quarter frame
/// (`α ≥ 1`: the analysis hop shrinks to `N/(4α)`; `α < 1`: the
/// synthesis hop shrinks to `αN/4`); frames placed at rounded positions
/// with the phase integrated over the rounded hop, so the stretched
/// signal stays coherent; weighted overlap-add normalised by the
/// accumulated squared window; for `α > 1` the bins above `0.95/α` of
/// Nyquist are cleared, so the read-back cannot alias. The read position
/// follows the frames' centres piecewise-linearly (drift-free when the
/// shift moves) through a 16-tap windowed-sinc interpolator
/// ([`Sinc<16>`], S2).
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/pitch/tests.rs::vocoder_shifts_pitch_to_the_cent`:
/// sines at 220 Hz and 1.5 kHz shifted ±12 and +7 semitones at 44.1, 48
/// and 96 kHz land within 0.13 cents of the target, both
/// reconstructions (budget 1 cent).
/// `crates/stave-dsp/src/effects/pitch/tests.rs::vocoder_keeps_clicks_sharp`:
/// a click train shifted +12 / −12 semitones keeps each click's energy
/// within an RMS spread of 1.2 / 0.3 ms with Průša's integration,
/// against 5.0 / 8.9 ms for identity locking. Latency: an impulse's
/// energy centroid lands at the reported latency within 0.7 samples
/// (Průša, unshifted and ±12; identity locking unshifted, 2.3)
/// (`crates/stave-dsp/src/effects/pitch/tests.rs::vocoder_latency_is_reported`).
/// Quality of the stretch itself: [`time_stretch`](super::time_stretch).
///
/// # Cost
///
/// Datasheet `"PhaseVocoder +7 st (Prusa)"` and `"PhaseVocoder +7 st
/// (LarocheDolson)"` at 48 kHz: two real transforms of 2048 per
/// analysis hop, the heap integration's `O(B log B)` per frame, and the
/// interpolator per sample.
///
/// # Aliasing
///
/// None for upward shifts (the bins that would fold are cleared before
/// the read-back); downward shifts read slower and image only through
/// the sinc interpolator's stopband (≈ −60 dB).
///
/// # Valid ranges
///
/// Shifts within the `range` given at construction (`±12` semitones by
/// default, up to `±36`); the latency is fixed by it,
/// `N/2 + ⌈(N/2 + 8)/α_min⌉ + N/2 + 2` samples (4 114 at 48 kHz for ±12).
/// Monophonic voice keeps its formants better through [`Psola`](super::Psola).
///
/// ```
/// use stave_dsp::effects::pitch::{PhaseVocoder, Reconstruction};
/// use stave_dsp::units::semitones;
/// use stave_dsp::Stage;
///
/// let mut up = PhaseVocoder::new(Reconstruction::Prusa, semitones(12.0));
/// up.set_shift(semitones(7.0)); // a fifth up
/// up.prepare(48_000.0);
/// let latency = up.latency() as usize;
/// let y: Vec<f32> = (0..latency + 2_048)
///     .map(|n| up.tick((n as f32 * core::f32::consts::TAU * 220.0 / 48_000.0).sin()))
///     .collect();
/// assert!(y[latency + 1_024..].iter().any(|v| v.abs() > 0.5));
/// ```
#[derive(Clone, Debug)]
pub struct PhaseVocoder {
    reconstruction: Reconstruction,
    range: f64,
    window: Seconds,
    ratio: f64,
    sample_rate: f32,
    size: usize,
    vocoder: Vocoder,
    /// The last `size` input samples, circular.
    input: Vec<f32>,
    /// Input samples taken, ever.
    ticks: u64,
    /// Samples since the last analysis.
    since: usize,
    /// The hop to the next analysis.
    hop: usize,
    anchors: Vec<Anchor>,
    /// Anchors made, ever (the newest is at `(made − 1) % len`).
    made: usize,
    /// The stretched signal's overlap-add sum, window-square sum and
    /// finished samples, circular.
    acc: Vec<f32>,
    norm: Vec<f32>,
    out: Vec<f32>,
    /// Stretched samples before this are finished.
    finished: i64,
    /// The rounded start of the last synthesised frame.
    last_start: i64,
    latency: usize,
    interp: Sinc<16>,
}

impl PhaseVocoder {
    /// The default analysis window, 40 ms (2048 samples at 44.1 and
    /// 48 kHz).
    pub const WINDOW: Seconds = ms(40.0);

    /// A shifter over `range` semitones either way (it sets the
    /// latency), unshifted until [`set_shift`](Self::set_shift).
    ///
    /// # Panics
    ///
    /// If `range` is outside `0..=36` semitones.
    pub fn new(reconstruction: Reconstruction, range: Semitones) -> Self {
        Self::with_window(reconstruction, range, Self::WINDOW)
    }

    /// The same with an analysis window of about `window` (rounded to a
    /// power of two of samples): longer resolves low notes better and
    /// smears transients more, for more latency.
    pub fn with_window(reconstruction: Reconstruction, range: Semitones, window: Seconds) -> Self {
        let range = f64::from(range.get());
        assert!((0.0..=36.0).contains(&range), "PhaseVocoder range {range} outside 0..=36 semitones");
        let mut s = Self {
            reconstruction,
            range,
            window,
            ratio: 1.0,
            sample_rate: 0.0,
            size: 0,
            vocoder: Vocoder::new(2, reconstruction),
            input: Vec::new(),
            ticks: 0,
            since: 0,
            hop: 1,
            anchors: Vec::new(),
            made: 0,
            acc: Vec::new(),
            norm: Vec::new(),
            out: Vec::new(),
            finished: 0,
            last_start: 0,
            latency: 0,
            interp: Sinc::default(),
        };
        s.plan(48_000.0);
        s
    }

    /// Moves every partial by `shift` (clamped to the range), from the
    /// next analysis frame on. Realtime: never allocates.
    pub fn set_shift(&mut self, shift: Semitones) {
        let st = f64::from(shift.get()).clamp(-self.range, self.range);
        self.ratio = (st / 12.0).exp2();
    }

    /// The shift's frequency ratio.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// The analysis window, in samples, at the prepared rate.
    pub fn window_samples(&self) -> usize {
        self.size
    }

    fn plan(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let n = frame_size(self.window, sample_rate);
        self.size = n;
        self.vocoder = Vocoder::new(n, self.reconstruction);
        self.input = vec![0.0; n];
        let lowest = (-self.range / 12.0).exp2();
        let highest = (self.range / 12.0).exp2();
        self.latency = n / 2 + ((n / 2 + 8) as f64 / lowest).ceil() as usize + n / 2 + 2;
        let fewest_hop = (n as f64 / (4.0 * highest)).round().max(1.0);
        self.anchors = vec![Anchor::default(); (self.latency as f64 / fewest_hop).ceil() as usize + 8];
        let span = ((self.latency + 3 * n) as f64 * (highest + 1.0)) as usize + 4 * n;
        let ring = span.next_power_of_two();
        self.acc = vec![0.0; ring];
        self.norm = vec![0.0; ring];
        self.out = vec![0.0; ring];
        self.interp.prepare();
        self.clear();
    }

    fn clear(&mut self) {
        self.vocoder.reset();
        self.input.fill(0.0);
        self.ticks = 0;
        self.since = 0;
        self.hop = self.analysis_hop();
        self.made = 0;
        self.acc.fill(0.0);
        self.norm.fill(0.0);
        self.out.fill(0.0);
        self.finished = 0;
        self.last_start = 0;
    }

    /// The hop to the next analysis at the current ratio: a quarter
    /// frame, or less when stretching so the synthesis hop stays a
    /// quarter frame.
    fn analysis_hop(&self) -> usize {
        ((self.size as f64 / (4.0 * self.ratio.max(1.0))).round() as usize).max(1)
    }

    fn anchor(&self, age: usize) -> &Anchor {
        let len = self.anchors.len();
        &self.anchors[(self.made - 1 - age) % len]
    }

    /// Analyses the frame that ends with the newest input, anchors it,
    /// and synthesises the frame before it.
    fn analyse(&mut self) {
        let n = self.size;
        let pos = (self.ticks % n as u64) as usize;
        let (input, hop) = (&self.input, self.hop);
        self.vocoder.analyse(
            |frame| {
                // Oldest first: the ring from the slot after the newest.
                let (newer, older) = input.split_at(pos + 1);
                frame[..older.len()].copy_from_slice(older);
                frame[older.len()..].copy_from_slice(newer);
            },
            hop,
        );
        let centre = (self.ticks + 1) as f64 - n as f64 / 2.0;
        let start = if self.made == 0 {
            0.0
        } else {
            self.anchor(0).start + self.ratio * hop as f64
        };
        let len = self.anchors.len();
        self.anchors[self.made % len] = Anchor {
            centre,
            start,
            ratio: self.ratio,
        };
        self.made += 1;
        if self.vocoder.waiting() >= 2 {
            self.synthesise();
        }
        self.hop = self.analysis_hop();
    }

    /// Synthesises the older waiting frame and overlap-adds it on the
    /// stretched axis.
    fn synthesise(&mut self) {
        let n = self.size;
        let frame = *self.anchor(1);
        let first = self.vocoder.waiting() == self.made;
        let start = frame.start.round() as i64;
        let hop = if first { n / 4 } else { (start - self.last_start).max(1) as usize };
        let cutoff = if frame.ratio > 1.0 {
            ((n / 2) as f64 * 0.95 / frame.ratio) as usize
        } else {
            n / 2 + 1
        };
        self.vocoder.synthesise(hop, cutoff, false);
        let mask = self.acc.len() - 1;
        let y = self.vocoder.output();
        let w = self.vocoder.window();
        for k in 0..n {
            let i = (start + k as i64) as usize & mask;
            self.acc[i] += y[k];
            self.norm[i] += w[k] * w[k];
        }
        // Everything before this frame's start is finished.
        while self.finished < start {
            let i = self.finished as usize & mask;
            self.out[i] = self.acc[i] / self.norm[i].max(0.05);
            self.acc[i] = 0.0;
            self.norm[i] = 0.0;
            self.finished += 1;
        }
        self.last_start = start;
    }

    /// The stretched position the output reads now, if there is one yet.
    fn read_position(&self) -> Option<f64> {
        let u = self.ticks as f64 - self.latency as f64;
        let half = self.size as f64 / 2.0;
        let kept = self.made.min(self.anchors.len());
        for age in 0..kept {
            let a = self.anchor(age);
            if a.centre <= u {
                let slope = if age == 0 {
                    a.ratio
                } else {
                    let b = self.anchor(age - 1);
                    (b.start - a.start) / (b.centre - a.centre)
                };
                return Some(a.start + half + (u - a.centre) * slope);
            }
        }
        None
    }
}

impl Stage for PhaseVocoder {
    /// Re-plans for the rate (this allocates) and clears the state.
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.plan(sample_rate);
        }
        self.clear();
    }

    fn reset(&mut self) {
        self.clear();
    }

    /// Realtime: never allocates; every analysis hop runs two
    /// transforms of the frame and the phase reconstruction.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let n = self.size;
        self.input[(self.ticks % n as u64) as usize] = x;
        self.since += 1;
        if self.since >= self.hop && self.ticks + 1 >= n as u64 {
            self.since = 0;
            self.analyse();
        }
        let y = match self.read_position() {
            Some(r) if r >= 8.0 => {
                let i = r.floor();
                let frac = (r - i) as f32;
                let i = i as i64;
                debug_assert!(
                    i + 9 <= self.finished,
                    "PhaseVocoder read {r} past the finished {}",
                    self.finished
                );
                let (top, frac) = if frac > 0.0 { (i + 8, 1.0 - frac) } else { (i + 7, 0.0) };
                let mask = self.out.len() - 1;
                let mut taps = [0.0f32; 16];
                for (k, t) in taps.iter_mut().enumerate() {
                    *t = self.out[(top - k as i64) as usize & mask];
                }
                self.interp.interpolate(&taps, frac)
            }
            _ => 0.0,
        };
        self.ticks += 1;
        y
    }

    fn latency(&self) -> u32 {
        self.latency as u32
    }

    /// The latency plus a frame.
    fn tail(&self) -> Tail {
        Tail::samples((self.latency + self.size) as u32)
    }
}
