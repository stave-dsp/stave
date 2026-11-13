//! Level detectors: what a dynamics processor measures before it
//! decides a gain. Each is a [`Stage`] from signal to a linear level.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::control::envelope::{EnvelopeFollower, EnvelopeTimes};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::primitives::filters::one_pole::tau_coeff;

/// A peak detector: the rectified signal through a one-pole that rises
/// with the attack time constant and falls with the release.
///
/// # Models
/// The smooth branching peak detector of Giannoulis, Massberg & Reiss,
/// "Digital Dynamic Range Compressor Design — A Tutorial and Analysis",
/// JAES 60(6), 2012, eq. (17), on `|x|`.
///
/// # Derivation
/// [`EnvelopeFollower`] over `|x|`: `y += k·(|x| − y)`, `k` from
/// [`tau_coeff`] of the attack while `|x| > y`, of the release otherwise.
/// Times are time constants (a step covers 63 % in one).
///
/// # Verified
/// `effects::dynamics::tests::detector_times_match_the_settings`: a
/// step's measured 63 % rise and fall land within one sample of the
/// attack and release.
///
/// # Cost
/// `Peak detector` on the datasheet.
///
/// # Aliasing
/// The rectifier makes harmonics of the input; they are the detector's
/// ripple, not audio (the output drives a gain).
///
/// # Valid ranges
/// Times `≥ 0` ms (0 follows instantly).
#[derive(Copy, Clone, Debug)]
pub struct Peak {
    follower: EnvelopeFollower,
}

impl Peak {
    /// A detector with time constants `attack_ms` and `release_ms`.
    pub fn new(attack_ms: f32, release_ms: f32) -> Self {
        Self {
            follower: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms,
                release_ms,
                slow_release_ms: None,
            }),
        }
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.follower.set_attack_ms(ms);
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.follower.set_release_ms(ms);
    }
}

impl Stage for Peak {
    fn prepare(&mut self, sample_rate: f32) {
        self.follower.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.follower.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.follower.tick(denormal::finite_or_zero(x).abs())
    }
    fn tail(&self) -> Tail {
        self.follower.tail()
    }
}

/// A windowed RMS detector: the root of the mean square over the last
/// `window_ms`.
///
/// # Models
/// The true moving RMS, `√(Σ x²[n−k] / N)` over `N` samples — a
/// rectangular window, as a sound-level meter's "fast"/"slow" are
/// exponential ones.
///
/// # Derivation
/// A ring of squares (allocated in `prepare`) and a running `f64` sum:
/// add the newest square, subtract the one leaving the window. `f64`
/// keeps the sum's drift below −150 dB of full scale over hours.
///
/// # Verified
/// `effects::dynamics::tests::rms_reads_a_sine_at_its_rms`: a sine
/// reads `A/√2` within 1e-4 once the window is full, and a burst leaves
/// the window exactly `N` samples after it stops.
///
/// # Cost
/// `Rms detector (10 ms)` on the datasheet.
///
/// # Aliasing
/// None that reaches audio (the output drives a gain).
///
/// # Valid ranges
/// `window_ms > 0` (at least one sample).
#[derive(Clone, Debug)]
pub struct Rms {
    window_ms: f32,
    squares: Vec<f32>,
    position: usize,
    sum: f64,
}

impl Rms {
    /// A detector over the last `window_ms`. The window is allocated in
    /// `prepare`.
    pub fn new(window_ms: f32) -> Self {
        debug_assert!(window_ms > 0.0, "window {window_ms} ms");
        Self {
            window_ms,
            squares: Vec::new(),
            position: 0,
            sum: 0.0,
        }
    }

    /// Samples in the window at the prepared rate.
    pub fn window_samples(&self) -> usize {
        self.squares.len()
    }
}

impl Stage for Rms {
    fn prepare(&mut self, sample_rate: f32) {
        let n = ((self.window_ms * sample_rate / 1000.0).round() as usize).max(1);
        self.squares = vec![0.0; n];
        self.reset();
    }
    fn reset(&mut self) {
        self.squares.fill(0.0);
        self.position = 0;
        self.sum = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let Some(slot) = self.squares.get_mut(self.position) else {
            return 0.0;
        };
        let x = denormal::finite_or_zero(x);
        let square = x * x;
        self.sum += f64::from(square) - f64::from(*slot);
        *slot = square;
        self.position += 1;
        if self.position == self.squares.len() {
            self.position = 0;
        }
        (self.sum.max(0.0) / self.squares.len() as f64).sqrt() as f32
    }
    fn tail(&self) -> Tail {
        Tail::samples(self.squares.len() as u32)
    }
}

/// A program-dependent peak detector: the release slows the longer and
/// harder the signal has held the detector up — the two-stage release
/// of an optical cell.
///
/// # Models
/// The behaviour of an opto compressor's release (a fast initial drop
/// after a transient, a long one after sustained level; LA-2A T4), as a
/// behavioural detector — no photocell model (that is `circuit`'s E7).
///
/// # Derivation
/// A peak follower whose release time constant moves between `fast` and
/// `slow` with a memory `m`: a symmetric one-pole of the envelope with
/// time constant `memory`. `τ_release = fast + (slow − fast)·min(m/y, 1)`:
/// a brief transient leaves `m ≪ y` (fast release); a held level brings
/// `m` up to `y` (slow release). Giannoulis et al. 2012 §4.4 discuss
/// such "auto release" smoothing.
///
/// # Verified
/// `effects::dynamics::tests::program_dependent_release_follows_history`:
/// after a 2 s hold the release time constant measures `slow` within
/// 5 %; after a 5 ms burst the 63 % fall is under a quarter of `slow`.
///
/// # Cost
/// `ProgramDependent detector` on the datasheet.
///
/// # Aliasing
/// As [`Peak`].
///
/// # Valid ranges
/// Times `≥ 0` ms, `fast ≤ slow`.
#[derive(Copy, Clone, Debug)]
pub struct ProgramDependent {
    attack_ms: f32,
    fast_ms: f32,
    slow_ms: f32,
    memory_ms: f32,
    sample_rate: f32,
    attack: f32,
    memory: f32,
    envelope: f32,
    held: f32,
}

impl ProgramDependent {
    /// A detector attacking with `attack_ms`, releasing between
    /// `fast_ms` and `slow_ms` as its `memory_ms`-long history fills.
    pub fn new(attack_ms: f32, fast_ms: f32, slow_ms: f32, memory_ms: f32) -> Self {
        debug_assert!(fast_ms <= slow_ms, "fast release {fast_ms} > slow {slow_ms}");
        let mut d = Self {
            attack_ms,
            fast_ms,
            slow_ms,
            memory_ms,
            sample_rate: 48_000.0,
            attack: 1.0,
            memory: 1.0,
            envelope: 0.0,
            held: 0.0,
        };
        d.update();
        d
    }

    fn update(&mut self) {
        self.attack = tau_coeff(self.attack_ms / 1000.0, self.sample_rate);
        self.memory = tau_coeff(self.memory_ms / 1000.0, self.sample_rate);
    }
}

impl Stage for ProgramDependent {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update();
        self.reset();
    }
    fn reset(&mut self) {
        self.envelope = 0.0;
        self.held = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = denormal::finite_or_zero(x).abs();
        let k = if x > self.envelope {
            self.attack
        } else {
            let history = if self.envelope > 0.0 {
                (self.held / self.envelope).min(1.0)
            } else {
                1.0
            };
            let ms = self.fast_ms + (self.slow_ms - self.fast_ms) * history;
            tau_coeff(ms / 1000.0, self.sample_rate)
        };
        self.envelope = denormal::flush(self.envelope + (x - self.envelope) * k);
        self.held = denormal::flush(self.held + (self.envelope - self.held) * self.memory);
        self.envelope
    }
    fn tail(&self) -> Tail {
        Tail::from_time_constant(f64::from(self.slow_ms.max(self.memory_ms)) / 1000.0, self.sample_rate)
    }
}

/// How a stereo detector's two levels become the two gains' keys.
///
/// # Models
/// The usual stereo link modes of a compressor or limiter.
///
/// # Derivation
/// `Independent` keeps each channel's level; `Max` keys both from the
/// louder (the image never shifts); `Mean` from their average.
///
/// # Verified
/// `effects::dynamics::tests::stereo_link_modes`.
///
/// # Cost
/// A compare or an add.
///
/// # Aliasing
/// None.
///
/// # Valid ranges
/// Any levels.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// Each channel keyed by its own level.
    Independent,
    /// Both keyed by the louder channel.
    #[default]
    Max,
    /// Both keyed by the mean of the two levels.
    Mean,
}

impl Link {
    /// The keys for left and right levels `l` and `r`.
    #[inline]
    pub fn apply(self, l: f32, r: f32) -> (f32, f32) {
        match self {
            Link::Independent => (l, r),
            Link::Max => {
                let m = l.max(r);
                (m, m)
            }
            Link::Mean => {
                let m = 0.5 * (l + r);
                (m, m)
            }
        }
    }
}

/// The sidechain filter: a band-limit on what the detector hears, so
/// low end doesn't pump the gain and sibilance can be keyed on.
///
/// # Models
/// A Butterworth highpass and lowpass in series (either optional) —
/// a compressor's sidechain EQ.
///
/// # Derivation
/// Two [`Biquad`]s (`q = 1/√2`, f64 coefficients).
///
/// # Verified
/// `effects::dynamics::tests::sidechain_band_limits_the_key`: −3 dB at
/// each corner, passband unity.
///
/// # Cost
/// `Sidechain (HP + LP)` on the datasheet.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// Corners in `(0, 0.5·sample_rate)` Hz; `None` bypasses that side.
#[derive(Clone, Debug)]
pub struct Sidechain {
    highpass: Option<Biquad>,
    lowpass: Option<Biquad>,
}

impl Sidechain {
    /// A key filter with an optional highpass and lowpass corner, in Hz.
    pub fn new(highpass_hz: Option<f32>, lowpass_hz: Option<f32>) -> Self {
        let q = core::f32::consts::FRAC_1_SQRT_2;
        Self {
            highpass: highpass_hz.map(|hz| Biquad::new(FilterSpec::Highpass { hz, q })),
            lowpass: lowpass_hz.map(|hz| Biquad::new(FilterSpec::Lowpass { hz, q })),
        }
    }
}

impl Stage for Sidechain {
    fn prepare(&mut self, sample_rate: f32) {
        for f in self.highpass.iter_mut().chain(self.lowpass.iter_mut()) {
            f.prepare(sample_rate);
        }
    }
    fn reset(&mut self) {
        for f in self.highpass.iter_mut().chain(self.lowpass.iter_mut()) {
            f.reset();
        }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = x;
        if let Some(f) = &mut self.highpass {
            y = f.tick(y);
        }
        if let Some(f) = &mut self.lowpass {
            y = f.tick(y);
        }
        y
    }
    fn tail(&self) -> Tail {
        let tail = |f: &Option<Biquad>| f.as_ref().map_or(Tail::None, Stage::tail);
        tail(&self.highpass).then(tail(&self.lowpass))
    }
}
