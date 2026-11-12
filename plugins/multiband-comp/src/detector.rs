//! A band's detector: sidechain high-pass, peak/RMS blend, and an
//! envelope follower retimed per sample by the style's program
//! dependence.

extern crate alloc;
use alloc::vec::Vec;

use stave_dsp::{Biquad, EnvelopeFollower, EnvelopeTimes, FilterSpec};

use crate::style::{attack_shrink, Character, FASTEST_ATTACK_MS};

/// The sidechain high-pass is off below this.
const HPF_MIN_HZ: f32 = 10.0;
const RMS_WINDOW_S: f32 = 0.010;

/// The knobs a detector reads, in the params' units.
#[derive(Copy, Clone, Debug)]
pub struct DetectorSettings {
    /// Program dependence measures overshoot against this.
    pub threshold_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Transient catch; 0 leaves attack dependence to the style.
    pub transient: f32,
    /// Peak (0) to RMS (1).
    pub rms: f32,
    pub hpf_hz: f32,
}

/// Running RMS over a fixed window.
struct RmsWindow {
    squares: Vec<f32>,
    sum: f64,
    next: usize,
}

impl RmsWindow {
    fn new() -> Self {
        Self {
            squares: Vec::new(),
            sum: 0.0,
            next: 0,
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        let len = (RMS_WINDOW_S * sample_rate).round().max(1.0) as usize;
        self.squares = alloc::vec![0.0; len];
        self.sum = 0.0;
        self.next = 0;
    }

    fn push(&mut self, x: f32) -> f32 {
        let square = x * x;
        self.sum += f64::from(square) - f64::from(self.squares[self.next]);
        self.squares[self.next] = square;
        self.next = (self.next + 1) % self.squares.len();
        (self.sum.max(0.0) / self.squares.len() as f64).sqrt() as f32
    }
}

/// Mono detector input → envelope, retimed per sample by the style's
/// program dependence.
pub struct Detector {
    hpf: Biquad,
    hpf_hz: f32,
    rms: RmsWindow,
    follower: EnvelopeFollower,
    sample_rate: f32,
}

impl Detector {
    pub fn new() -> Self {
        Self {
            hpf: Biquad::default(),
            hpf_hz: 0.0,
            rms: RmsWindow::new(),
            follower: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 8.0,
                release_ms: 100.0,
                slow_release_ms: None,
            }),
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.hpf.reset();
        self.hpf_hz = 0.0;
        self.rms.prepare(sample_rate);
        self.follower.prepare(sample_rate);
    }

    /// One sample of mono detector input; returns the envelope (linear).
    pub fn tick(&mut self, mono: f32, s: &DetectorSettings, character: &Character) -> f32 {
        if s.hpf_hz != self.hpf_hz {
            self.hpf_hz = s.hpf_hz;
            if self.hpf_hz >= HPF_MIN_HZ {
                let hpf = FilterSpec::Highpass {
                    hz: self.hpf_hz,
                    q: core::f32::consts::FRAC_1_SQRT_2,
                };
                self.hpf.set(hpf, self.sample_rate);
            }
        }
        let x = if self.hpf_hz >= HPF_MIN_HZ {
            self.hpf.process(mono)
        } else {
            mono
        };
        let rms = self.rms.push(x);
        let input = x.abs() + s.rms * (rms - x.abs());

        let envelope = self.follower.state();
        let dependence = if s.transient > 0.0 {
            s.transient
        } else {
            character.attack_dependence
        };
        let mut attack_ms = s.attack_ms * character.attack_scale;
        if dependence > 0.0 {
            attack_ms =
                (attack_ms * attack_shrink(dependence, envelope, input)).max(FASTEST_ATTACK_MS);
        }
        let stretch = character.release_stretch(envelope, s.threshold_db);
        self.retime(EnvelopeTimes {
            attack_ms,
            release_ms: s.release_ms * character.release_scale * stretch,
            slow_release_ms: character.slow_release_ms.map(|ms| ms * stretch),
        });
        self.follower.tick(input)
    }

    fn retime(&mut self, times: EnvelopeTimes) {
        let now = self.follower.times();
        if times.attack_ms != now.attack_ms {
            self.follower.set_attack_ms(times.attack_ms);
        }
        if times.release_ms != now.release_ms {
            self.follower.set_release_ms(times.release_ms);
        }
        if times.slow_release_ms != now.slow_release_ms {
            self.follower.set_slow_release_ms(times.slow_release_ms);
        }
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
