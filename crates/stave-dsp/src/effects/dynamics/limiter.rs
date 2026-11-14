//! The look-ahead brickwall limiter, on sample peaks or true peaks.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::primitives::gain::db_to_amp;
use crate::primitives::time::delay::DelayLine;
use crate::units::Seconds;
use crate::units::Db;

/// Interpolator taps on each side of a reconstructed point — the
/// BS.1770-4 annex 2 meter's (and `stave_measure::loudness::TruePeak`'s
/// default).
const HALF_TAPS: usize = 16;
/// Points per sample interval the true-peak detector reconstructs.
const OVERSAMPLING: usize = 4;
/// The input history the interpolator reads (a power of two above
/// `2·HALF_TAPS`).
const HISTORY: usize = 64;
/// The shortest ramp (samples) in true-peak mode: twice the
/// interpolator's support. The gain then changes little across any one
/// reconstructed point's 32 taps, which is what keeps the reconstructed
/// output under the ceiling; shorter ramps measured up to +0.016 dB over
/// it on a clipped square (`tests::the_limiter_never_exceeds_its_ceiling`,
/// which first holds at 48 samples).
const MIN_TRUE_PEAK_RAMP: usize = 4 * HALF_TAPS;
/// The gain is scaled by this so the running mean's rounding (a few
/// ulps over the lookahead) can never lift it over the ceiling:
/// −0.00001 dB.
const ROUNDING_GUARD: f32 = 1.0 - 1e-6;

/// What a [`Limiter`] holds under its ceiling.
///
/// # Models
/// Sample peaks (the stored values) or true peaks (the reconstructed
/// waveform between them, ITU-R BS.1770-4 annex 2).
///
/// # Derivation
/// See [`Limiter`].
///
/// # Verified
/// `crates/stave-dsp/src/effects/dynamics/tests.rs::the_limiter_never_exceeds_its_ceiling`.
///
/// # Cost
/// None (data); true peak costs the interpolator (datasheet).
///
/// # Aliasing
/// None.
///
/// # Valid ranges
/// Either.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Peaks {
    /// The samples themselves.
    Sample,
    /// The 4× reconstructed waveform (inter-sample peaks).
    #[default]
    True,
}

/// A look-ahead brickwall limiter: the output never exceeds the
/// ceiling, sample or true peak, and the gain moves smoothly into each
/// peak instead of clipping it.
///
/// # Models
/// The standard look-ahead peak limiter (Zölzer, *DAFX* 2nd ed. §4.2.2;
/// the min-hold / moving-average gain of Giannoulis et al. 2012 §5),
/// detecting true peak with the BS.1770-4 interpolator. Algorithmic; no
/// hardware claim.
///
/// # Derivation
/// For each sample the required gain is `min(1, ceiling/p)`, `p` the
/// peak of the sample and the reconstructed points of the intervals on
/// either side (4×, 32-tap Blackman-windowed sinc, as the meter). A
/// running minimum over `L + 2H` samples (`L` the lookahead, `H` the
/// interpolator's half length, 0 for sample peaks) holds it; a one-pole
/// release lets it recover; a moving average over `L` samples ramps
/// into it. Every sample within `H` of a peak is then scaled by at most
/// that peak's gain, and the audio is delayed by `H + (L − 1) + H` so
/// the ramp finishes as the peak arrives. [`latency`](Stage::latency)
/// reports that delay exactly.
///
/// # Verified
/// `crates/stave-dsp/src/effects/dynamics/tests.rs::the_limiter_never_exceeds_its_ceiling`: on hostile signals (a +12 dB sine at
/// `sample_rate/4` and 45°, all inter-sample overs; +20 dB noise; noise
/// bursts, a near-Nyquist sweep, a clipped square, spikes, a DC step;
/// lookaheads 0.1–5 ms) the output's true peak, measured by
/// `stave_measure::loudness::TruePeak`, never exceeds the ceiling (worst
/// −0.000004 dB); sample-peak mode never exceeds it in samples; an impulse
/// under the ceiling arrives at exactly `latency` samples; the release
/// time constant measures the setting within 2 %.
///
/// # Cost
/// `"Limiter (true peak)"` and `"Limiter (sample peak)"` on the datasheet.
///
/// # Aliasing
/// The gain is band-limited by the ramp (an `L`-sample moving average);
/// the limiter adds no oversampling of its own.
///
/// # Valid ranges
/// Ceiling `≤ 0` dB(TP) in practice (any finite value works); lookahead
/// `≥ 0` ms, at least one sample, and in true-peak mode at least 64
/// samples (1.33 ms at 48 kHz; see `MIN_TRUE_PEAK_RAMP`); release
/// `≥ 0` ms.
#[derive(Clone, Debug)]
pub struct Limiter {
    peaks: Peaks,
    ceiling_db: f32,
    lookahead_ms: f32,
    release_ms: f32,
    sample_rate: f32,
    ceiling: f32,
    release: f32,
    lookahead: usize,
    hold: usize,
    kernels: [[f32; 2 * HALF_TAPS]; OVERSAMPLING - 1],
    history: [f32; HISTORY],
    frame: usize,
    previous_interval: f32,
    minimum: MinWindow,
    recovered: f32,
    ramp: Vec<f32>,
    ramp_position: usize,
    ramp_sum: f64,
    audio: DelayLine,
}

impl Limiter {
    /// A limiter on `peaks` at `ceiling_db`, with 1.5 ms lookahead and
    /// 50 ms release. Buffers are allocated in `prepare`.
    pub fn new(peaks: Peaks, ceiling: Db) -> Self {
        let ceiling_db = ceiling.get();
        let mut l = Self {
            peaks,
            ceiling_db,
            lookahead_ms: 1.5,
            release_ms: 50.0,
            sample_rate: 48_000.0,
            ceiling: 1.0,
            release: 1.0,
            lookahead: 1,
            hold: 0,
            kernels: true_peak_kernels(),
            history: [0.0; HISTORY],
            frame: 0,
            previous_interval: 0.0,
            minimum: MinWindow::default(),
            recovered: 1.0,
            ramp: Vec::new(),
            ramp_position: 0,
            ramp_sum: 0.0,
            audio: DelayLine::new(),
        };
        l.update();
        l
    }

    /// The ceiling in dB (true peak or sample peak, per [`Peaks`]).
    pub fn set_ceiling(&mut self, ceiling: Db) {
        let db = ceiling.get();
        self.ceiling_db = db;
        self.update();
    }

    /// The release time constant (gain recovery, 63 %).
    pub fn set_release(&mut self, release: Seconds) {
        let ms = release.to_ms() as f32;
        self.release_ms = ms;
        self.update();
    }

    /// The lookahead: how long the gain ramps into a peak. Changes the
    /// latency, so it takes effect at the next `prepare`.
    pub fn set_lookahead(&mut self, lookahead: Seconds) {
        let ms = lookahead.to_ms() as f32;
        self.lookahead_ms = ms.max(0.0);
    }

    /// The gain applied to the sample now leaving (linear, ≤ 1), for a
    /// meter.
    pub fn gain(&self) -> f32 {
        (self.ramp_sum / self.lookahead as f64) as f32
    }

    fn update(&mut self) {
        self.ceiling = db_to_amp(self.ceiling_db);
        self.release = tau_coeff(Seconds::new(f64::from(self.release_ms / 1000.0)), self.sample_rate);
    }

    /// Samples between the detector's newest input and the sample it
    /// judges (the interpolator's look forward).
    fn detector_delay(&self) -> usize {
        match self.peaks {
            Peaks::Sample => 0,
            Peaks::True => HALF_TAPS,
        }
    }

    /// The largest magnitude of the reconstructed points strictly
    /// between the sample `HALF_TAPS` back and the one after it.
    fn interval_peak(&self) -> f32 {
        // taps cover history[frame − 2·HALF + 1 ..= frame].
        let start = self.frame + HISTORY - (2 * HALF_TAPS - 1);
        let mut peak = 0.0f32;
        for kernel in &self.kernels {
            let mut y = 0.0;
            for (k, h) in kernel.iter().enumerate() {
                y += h * self.history[(start + k) % HISTORY];
            }
            peak = peak.max(y.abs());
        }
        peak
    }
}

/// The BS.1770-4 interpolator: for each of the `OVERSAMPLING − 1`
/// fractions, `2·HALF_TAPS` Blackman-windowed sinc taps normalised to
/// unity DC gain — the same kernel as `stave_measure`'s true-peak meter.
fn true_peak_kernels() -> [[f32; 2 * HALF_TAPS]; OVERSAMPLING - 1] {
    let mut kernels = [[0.0f32; 2 * HALF_TAPS]; OVERSAMPLING - 1];
    let half = HALF_TAPS as f64;
    for (p, kernel) in kernels.iter_mut().enumerate() {
        let fraction = (p + 1) as f64 / OVERSAMPLING as f64;
        let mut taps = [0.0f64; 2 * HALF_TAPS];
        for (k, tap) in taps.iter_mut().enumerate() {
            let t = k as f64 - half + 1.0 - fraction;
            let sinc = if t == 0.0 {
                1.0
            } else {
                (core::f64::consts::PI * t).sin() / (core::f64::consts::PI * t)
            };
            let u = (t + half) / (2.0 * half);
            let tau = core::f64::consts::TAU;
            let blackman = 0.42 - 0.5 * (tau * u).cos() + 0.08 * (2.0 * tau * u).cos();
            *tap = sinc * blackman;
        }
        let gain: f64 = taps.iter().sum();
        for (out, tap) in kernel.iter_mut().zip(taps) {
            *out = (tap / gain) as f32;
        }
    }
    kernels
}

impl Stage for Limiter {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.update();
        self.lookahead = ((self.lookahead_ms * sample_rate / 1000.0).round() as usize).max(1);
        if self.peaks == Peaks::True {
            self.lookahead = self.lookahead.max(MIN_TRUE_PEAK_RAMP);
        }
        self.hold = self.detector_delay();
        self.minimum.allocate(self.lookahead + 2 * self.hold);
        self.ramp = vec![1.0; self.lookahead];
        self.audio.allocate(self.latency() as usize + 1);
        self.reset();
    }

    fn reset(&mut self) {
        self.history = [0.0; HISTORY];
        self.frame = 0;
        self.previous_interval = 0.0;
        self.minimum.reset();
        self.recovered = 1.0;
        self.ramp.fill(1.0);
        self.ramp_position = 0;
        self.ramp_sum = self.ramp.len() as f64;
        self.audio.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = denormal::finite_or_zero(x);
        // Detect: the peak around the sample `detector_delay` back.
        let peak = match self.peaks {
            Peaks::Sample => x.abs(),
            Peaks::True => {
                self.frame = (self.frame + 1) % HISTORY;
                self.history[self.frame] = x;
                let judged = self.history[(self.frame + HISTORY - HALF_TAPS) % HISTORY];
                let next = self.interval_peak();
                let peak = judged.abs().max(next).max(self.previous_interval);
                self.previous_interval = next;
                peak
            }
        };
        let required = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        // Hold, recover, ramp.
        let held = self.minimum.push(required);
        self.recovered = if held < self.recovered {
            held
        } else {
            self.recovered + (held - self.recovered) * self.release
        };
        if let Some(slot) = self.ramp.get_mut(self.ramp_position) {
            self.ramp_sum += f64::from(self.recovered) - f64::from(*slot);
            *slot = self.recovered;
            self.ramp_position += 1;
            if self.ramp_position == self.ramp.len() {
                self.ramp_position = 0;
            }
        }
        let gain = (self.ramp_sum / self.lookahead as f64) as f32 * ROUNDING_GUARD;
        // The audio, delayed so the ramp completes as its peak arrives.
        self.audio.write(x);
        let delayed = self.audio.read_int(self.latency() as usize);
        self.audio.advance();
        delayed * gain.min(1.0)
    }

    /// Detector delay + ramp + hold: `H + (L − 1) + H` samples.
    fn latency(&self) -> u32 {
        (self.detector_delay() + self.lookahead - 1 + self.hold) as u32
    }

    /// The delayed audio drains after the input stops.
    fn tail(&self) -> Tail {
        Tail::samples(self.latency())
    }
}

/// A running minimum over the last `window` values: a monotonic deque
/// in a fixed ring (allocated once), amortised O(1) per push.
#[derive(Clone, Debug, Default)]
struct MinWindow {
    values: Vec<(u64, f32)>,
    head: usize,
    len: usize,
    window: u64,
    count: u64,
}

impl MinWindow {
    fn allocate(&mut self, window: usize) {
        self.values = vec![(0, 1.0); window.max(1) + 1];
        self.window = window.max(1) as u64;
        self.reset();
    }

    fn reset(&mut self) {
        self.head = 0;
        self.len = 0;
        self.count = 0;
    }

    /// Push `value`; the minimum of the last `window` values.
    #[inline]
    fn push(&mut self, value: f32) -> f32 {
        let cap = self.values.len();
        if cap == 0 {
            return value;
        }
        // Drop larger-or-equal values from the back: they can never be
        // the minimum again.
        while self.len > 0 {
            let back = (self.head + self.len - 1) % cap;
            if self.values[back].1 >= value {
                self.len -= 1;
            } else {
                break;
            }
        }
        // Expire the front.
        while self.len > 0 && self.values[self.head].0 + self.window <= self.count {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        let slot = (self.head + self.len) % cap;
        self.values[slot] = (self.count, value);
        self.len += 1;
        self.count += 1;
        self.values[self.head].1
    }
}
