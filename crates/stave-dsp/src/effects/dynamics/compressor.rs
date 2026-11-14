//! The static gain computer and the compressor built around it, in
//! feed-forward and feedback topologies.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::tau_coeff;
use crate::primitives::gain::{amp_to_db, db_to_amp};
use crate::units::Seconds;
use crate::units::Db;

/// The static curve of a compressor: the gain, in dB, for a level.
///
/// # Models
/// Giannoulis, Massberg & Reiss 2012, eq. (4): threshold `T`, ratio `R`,
/// knee width `W` (0 = hard knee), all in dB. With `x` the level:
/// below the knee `y = x`; inside (`2|x − T| ≤ W`)
/// `y = x + (1/R − 1)(x − T + W/2)²/(2W)`; above `y = T + (x − T)/R`.
/// The gain is `y − x`.
///
/// # Derivation
/// The published formula, plus its feedback form: detecting the output
/// level `y`, the gain `c = (1 − R)(y − T)` above the knee (and
/// `(1 − R)(y − T + W/2)²/(2W)` inside it) reaches the same static
/// curve, since `y = x + c` solves to `y = T + (x − T)/R`.
///
/// # Verified
/// `crates/stave-dsp/src/effects/dynamics/tests.rs::static_curves_match_the_formulas`: the
/// curve against eq. (4) at every 0.25 dB, and a compressor's settled
/// output level on a held input against it, both topologies, within
/// 0.01 dB (hard knee) and 0.05 dB (feedback soft knee, against the
/// implicit solution).
///
/// # Cost
/// A few adds and one multiply per call.
///
/// # Aliasing
/// None by itself (a static map of a level, not of the audio).
///
/// # Valid ranges
/// `R ≥ 1` (`f32::INFINITY` limits; the feedback form needs it finite),
/// `W ≥ 0`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GainComputer {
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
}

impl GainComputer {
    /// The gain (≤ 0 dB) for an input level, feed-forward.
    #[inline]
    pub fn gain(&self, level: Db) -> Db {
        Db::new(self.gain_at(level.get()))
    }

    /// [`gain`](Self::gain) on the bare dB scalar: the detector loop's form.
    #[inline]
    fn gain_at(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 / self.ratio - 1.0;
        let w = self.knee_db;
        if 2.0 * over <= -w {
            0.0
        } else if 2.0 * over.abs() <= w && w > 0.0 {
            let k = over + 0.5 * w;
            slope * k * k / (2.0 * w)
        } else {
            slope * over
        }
    }

    /// The output level for an input level: `level + gain(level)`.
    #[inline]
    pub fn output(&self, level: Db) -> Db {
        level + self.gain(level)
    }

    /// The slope `d(feedback_gain)/d(output)` at an output level, dB per
    /// dB: 0 below the knee, `1 − R` above it.
    #[inline]
    pub fn feedback_slope(&self, output: Db) -> f32 {
        let output_db = output.get();
        let over = output_db - self.threshold_db;
        let w = self.knee_db;
        if 2.0 * over <= -w {
            0.0
        } else if 2.0 * over.abs() <= w && w > 0.0 {
            (1.0 - self.ratio) * (over + 0.5 * w) / w
        } else {
            1.0 - self.ratio
        }
    }

    /// The gain (≤ 0 dB) for the *output* level, for a feedback
    /// compressor that reaches the same static curve.
    #[inline]
    pub fn feedback_gain(&self, output: Db) -> Db {
        Db::new(self.feedback_gain_at(output.get()))
    }

    /// [`feedback_gain`](Self::feedback_gain) on the bare dB scalar.
    #[inline]
    fn feedback_gain_at(&self, output_db: f32) -> f32 {
        let over = output_db - self.threshold_db;
        let slope = 1.0 - self.ratio;
        let w = self.knee_db;
        if 2.0 * over <= -w {
            0.0
        } else if 2.0 * over.abs() <= w && w > 0.0 {
            let k = over + 0.5 * w;
            slope * k * k / (2.0 * w)
        } else {
            slope * over
        }
    }
}

/// Where a compressor takes its level from.
///
/// # Models
/// The two topologies of Giannoulis et al. 2012, §2: feed-forward
/// (the input drives the gain) and feedback (the output does, as in
/// the 1176 and LA-2A).
///
/// # Derivation
/// See [`Compressor`].
///
/// # Verified
/// `crates/stave-dsp/src/effects/dynamics/tests.rs::static_curves_match_the_formulas` runs both.
///
/// # Cost
/// None (data).
///
/// # Aliasing
/// None.
///
/// # Valid ranges
/// Any.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Topology {
    /// Detect the input.
    #[default]
    FeedForward,
    /// Detect the output (one sample late).
    Feedback,
}

/// A compressor: level detection, the [`GainComputer`], and smoothing
/// of the gain in dB, in either [`Topology`].
///
/// # Models
/// The log-domain compressor Giannoulis et al. 2012 recommend (§3.4,
/// Fig. 7): instantaneous level in dB → gain computer → smooth
/// branching peak detector on the gain reduction (eq. 17) → linear gain
/// and make-up. Algorithmic; no hardware claim.
///
/// # Derivation
/// The gain reduction `r ≥ 0` follows its target with
/// `r += k·(target − r)`, `k` from [`tau_coeff`] of the attack while the
/// reduction grows and of the release while it shrinks, so the times
/// are time constants of the gain in dB. Feedback detects the previous
/// output (before make-up); linearised, its loop settles by
/// `1 − k·(1 − s)` per sample, `s` the [`GainComputer::feedback_slope`]
/// at the output level (`1 − R` above the knee), so the step is divided
/// by `1 − s`: the closed loop then moves with the set time constant,
/// and is stable for any ratio.
///
/// # Verified
/// `crates/stave-dsp/src/effects/dynamics/tests.rs::static_curves_match_the_formulas`: settled levels match
/// [`GainComputer::output_db`] (feed-forward and feedback); the gain's
/// measured 63 % attack and release times (from the audio, on a DC
/// step) match the settings within 1 % feed-forward and 5 % feedback
/// (`crates/stave-dsp/src/effects/dynamics/tests.rs::compressor_times_match_the_settings`); no allocation after
/// `prepare` (`crates/stave-dsp/tests/realtime_effects.rs::effects_never_allocate_after_prepare`).
///
/// # Cost
/// `"Compressor (feed-forward)"` and `"Compressor (feedback)"` on the
/// datasheet.
///
/// # Aliasing
/// The gain is a smoothed control signal; fast attacks modulate the
/// audio and make sidebands, as any compressor does (no oversampling).
///
/// # Valid ranges
/// Times `≥ 0` ms; the [`GainComputer`]'s ranges; feedback needs a
/// finite ratio.
#[derive(Copy, Clone, Debug)]
pub struct Compressor {
    computer: GainComputer,
    topology: Topology,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    sample_rate: f32,
    attack: f32,
    release: f32,
    reduction_db: f32,
    last_output: f32,
}

impl Compressor {
    /// A compressor with `computer`'s curve in `topology`, attack 10 ms,
    /// release 100 ms, no make-up.
    pub fn new(computer: GainComputer, topology: Topology) -> Self {
        let mut c = Self {
            computer,
            topology,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_db: 0.0,
            sample_rate: 48_000.0,
            attack: 1.0,
            release: 1.0,
            reduction_db: 0.0,
            last_output: 0.0,
        };
        c.update();
        c
    }

    pub fn set_computer(&mut self, computer: GainComputer) {
        self.computer = computer;
        self.update();
    }

    /// The gain reduction's time constant while it grows.
    pub fn set_attack(&mut self, attack: Seconds) {
        let ms = attack.to_ms() as f32;
        self.attack_ms = ms;
        self.update();
    }

    /// The gain reduction's time constant while it shrinks.
    pub fn set_release(&mut self, release: Seconds) {
        let ms = release.to_ms() as f32;
        self.release_ms = ms;
        self.update();
    }

    pub fn set_makeup(&mut self, makeup: Db) {
        let db = makeup.get();
        self.makeup_db = db;
    }

    /// The current gain reduction (≥ 0 dB), for a meter.
    pub fn reduction(&self) -> Db {
        Db::new(self.reduction_db)
    }

    /// Compress `x` keyed by `key` (an external or filtered sidechain);
    /// [`tick`](Stage::tick) is `tick_keyed(x, x)`. Feedback ignores the
    /// key and detects its own output.
    #[inline]
    pub fn tick_keyed(&mut self, x: f32, key: f32) -> f32 {
        let (target, loop_gain) = match self.topology {
            Topology::FeedForward => (-self.computer.gain_at(amp_to_db(key.abs())), 1.0),
            Topology::Feedback => {
                let level = amp_to_db(self.last_output.abs());
                // The loop moves the target by `slope` per dB of
                // reduction; dividing the step by `1 − slope` makes the
                // closed loop settle with the set time constant.
                (
                    -self.computer.feedback_gain_at(level),
                    1.0 - self.computer.feedback_slope(Db::new(level)),
                )
            }
        };
        let k = if target > self.reduction_db {
            self.attack
        } else {
            self.release
        } / loop_gain;
        self.reduction_db = denormal::flush(self.reduction_db + (target - self.reduction_db) * k);
        let y = x * db_to_amp(-self.reduction_db);
        self.last_output = denormal::finite_or_zero(y);
        y * db_to_amp(self.makeup_db)
    }

    fn update(&mut self) {
        self.attack = tau_coeff(Seconds::new(f64::from(self.attack_ms / 1000.0)), self.sample_rate);
        self.release = tau_coeff(Seconds::new(f64::from(self.release_ms / 1000.0)), self.sample_rate);
    }
}

impl Stage for Compressor {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update();
        self.reset();
    }
    fn reset(&mut self) {
        self.reduction_db = 0.0;
        self.last_output = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_keyed(x, x)
    }
    /// None: the gain scales the input, so silence in is silence out.
    fn tail(&self) -> Tail {
        Tail::None
    }
}
