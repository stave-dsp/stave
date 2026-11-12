//! `Strings` — the harp: twelve voices, one per pitch class, each a
//! pair of plucked-string loops (Karplus–Strong: a tuned delay, a
//! damping low-pass, a loop gain) — the voice's string in the OCTAVE
//! register and a quieter one an octave above it, the way a doubled
//! course rings.
//!
//! * Each voice is BOWED: its drive level (detect.rs) shapes a noise
//!   bow into both loops, so the string sings at its own pitch for as
//!   long as its note is played, then rings out on SUSTAIN. The bow
//!   is low-passed at the voice's fundamental — a bowed string's
//!   spectrum falls like 1/n (the sawtooth of stick-slip), so the
//!   fundamental leads, as it does on a real string.
//! * SUSTAIN sets each loop's gain from a T60 (0.12 s … 36 s); a soft
//!   saturator in every loop bounds it, so maxed strings sing but
//!   never run away.
//! * TONE sets the damping as a corner RELATIVE to each string's
//!   fundamental (dark keeps ~3 harmonics, bright ~40), and the loop
//!   gain is compensated at the fundamental — so every register rings
//!   for the same T60 and only the overtones die faster, like gut.
//! * VIBRATO drifts each voice's tuning on its own slow random walk.
//! * Self-oscillation: with WIND and SUSTAIN both near the top, a
//!   whisper of noise enters every loop — the "noise floor" the
//!   strings bloom from with nothing played.

use stave_dsp::{DelayLine, Xorshift32};

use super::detect::{class_hz, CLASSES};

const T60_MIN_S: f32 = 0.12;
const T60_MAX_S: f32 = 36.0;
/// The octave-up string's level against the voice's own.
const UPPER: f32 = 0.2;
/// The longest string's frequency (C2 at LOW), with margin.
const LOWEST_HZ: f32 = 30.0;
const NOISE_FLOOR: f32 = 0.01;
/// The bow's strength per unit of drive level.
const BOW: f32 = 9.0;
/// Vibrato depth at full DRIFT (fraction of the period: ±0.6%
/// ≈ ±10 cents), and how often each voice picks a new drift target.
const VIBRATO: f32 = 0.006;
const DRIFT_HZ: f32 = 1.3;

#[inline]
fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// One plucked-string loop.
struct Loop {
    line: DelayLine,
    lp: f32,
    /// The period in samples at nominal tuning.
    period: f32,
    /// This loop's damping coefficient and gain (per block).
    damp: f32,
    gain: f32,
}

impl Loop {
    fn new() -> Self {
        Self { line: DelayLine::new(), lp: 0.0, period: 100.0, damp: 0.0, gain: 0.0 }
    }

    /// Set damping (corner `harmonics` × the fundamental) and the loop
    /// gain for `t60` seconds, compensated for the damping's loss at
    /// the fundamental.
    fn tune(&mut self, sr: f32, t60: f32, harmonics: f32) {
        let f0 = sr / self.period;
        let fc = (f0 * harmonics).min(0.45 * sr);
        let a = (-core::f32::consts::TAU * fc / sr).exp();
        let w = core::f32::consts::TAU * f0 / sr;
        let loss = (1.0 - a) / (1.0 - 2.0 * a * w.cos() + a * a).sqrt();
        self.damp = a;
        self.gain = (10f32.powf(-3.0 * self.period / (t60 * sr)) / loss).min(0.9999);
    }

    #[inline]
    fn tick(&mut self, x: f32, tune: f32) -> f32 {
        let (gain, damp) = (self.gain, self.damp);
        // The damping low-pass delays the loop by damp/(1−damp)
        // samples at low frequencies; the line makes up the rest.
        let d = (self.period * tune - damp / (1.0 - damp)).max(2.0);
        let y = self.line.read_cubic(d);
        self.lp = y + damp * (self.lp - y);
        self.line.write(soft(x + gain * self.lp));
        self.line.advance();
        self.lp
    }
}

/// Per-block settings.
#[derive(Copy, Clone)]
pub(super) struct Voicing {
    pub(super) octave: i32,
    pub(super) sustain: f32,
    pub(super) tone: f32,
    pub(super) wind: f32,
    pub(super) vibrato: f32,
}

pub(super) struct Strings {
    sr: f32,
    main: [Loop; CLASSES],
    upper: [Loop; CLASSES],
    input_gain: [[f32; 2]; CLASSES],
    /// Each voice's bow filter: coefficient, its level make-up, state.
    bow_k: [f32; CLASSES],
    bow_gain: [f32; CLASSES],
    bow_lp: [f32; CLASSES],
    noise: f32,
    vibrato: f32,
    octave: i32,
    drift: [f32; CLASSES],
    drift_to: [f32; CLASSES],
    drift_phase: [f32; CLASSES],
    rng: Xorshift32,
    /// Each voice's ring level (for the LEDs).
    level: [f32; CLASSES],
    level_att: f32,
    level_rel: f32,
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Strings {
    pub(super) fn new() -> Self {
        Self {
            sr: 48_000.0,
            main: core::array::from_fn(|_| Loop::new()),
            upper: core::array::from_fn(|_| Loop::new()),
            input_gain: [[0.0; 2]; CLASSES],
            bow_k: [1.0; CLASSES],
            bow_gain: [1.0; CLASSES],
            bow_lp: [0.0; CLASSES],
            noise: 0.0,
            vibrato: 0.0,
            octave: -1,
            drift: [0.0; CLASSES],
            drift_to: [0.0; CLASSES],
            drift_phase: core::array::from_fn(|c| c as f32 / CLASSES as f32),
            rng: Xorshift32::new(0xAE01_1A11),
            level: [0.0; CLASSES],
            level_att: 0.0,
            level_rel: 0.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let cap = (sr / LOWEST_HZ) as usize + 8;
        for l in self.main.iter_mut().chain(self.upper.iter_mut()) {
            l.line.prepare(cap);
            l.lp = 0.0;
        }
        self.octave = -1;
        self.drift = [0.0; CLASSES];
        self.drift_to = [0.0; CLASSES];
        self.level = [0.0; CLASSES];
        self.bow_lp = [0.0; CLASSES];
        self.level_att = 1.0 - (-1000.0 / (8.0 * sr)).exp();
        self.level_rel = 1.0 - (-1000.0 / (220.0 * sr)).exp();
        self.rng = Xorshift32::new(0xAE01_1A11);
    }

    /// Re-voice for a block: register, loop gains, damping, noise.
    pub(super) fn voice(&mut self, v: Voicing) {
        if v.octave != self.octave {
            self.octave = v.octave;
            for c in 0..CLASSES {
                self.main[c].period = self.sr / class_hz(c, v.octave);
                self.upper[c].period = self.sr / class_hz(c, v.octave + 1);
                let k = (core::f32::consts::TAU * class_hz(c, v.octave) / self.sr).min(1.0);
                self.bow_k[c] = k;
                // A one-pole keeps ~k/2 of white noise's power.
                self.bow_gain[c] = (2.0 / k).sqrt().min(30.0) * 0.5;
            }
        }
        let t60 = if v.sustain >= 1.0 { T60_MAX_S } else { T60_MIN_S * (T60_MAX_S / T60_MIN_S).powf(v.sustain) };
        let drive = 1.5 + 4.0 * v.wind;
        let harmonics = 3.0 * (40.0f32 / 3.0).powf(v.tone);
        for c in 0..CLASSES {
            for (k, l) in [&mut self.main[c], &mut self.upper[c]].into_iter().enumerate() {
                l.tune(self.sr, t60, harmonics);
                // A noise bow settles near 1/√(1−g), but a long-T60
                // string never settles within a played note — (1−g)^0.35
                // keeps short strings tame and lets long ones still
                // swell to a useful level.
                self.input_gain[c][k] = drive * (1.0 - l.gain).powf(0.35);
            }
        }
        self.noise = NOISE_FLOOR * smoothstep(0.8, 1.0, v.wind) * smoothstep(0.8, 1.0, v.sustain);
        self.vibrato = VIBRATO * v.vibrato;
    }

    /// One frame: each voice's drive level in, its ring out.
    #[inline]
    pub(super) fn tick(&mut self, exc: &[f32; CLASSES], out: &mut [f32; CLASSES]) {
        let step = DRIFT_HZ / self.sr;
        for c in 0..CLASSES {
            let mut tune = 1.0;
            if self.vibrato > 0.0 {
                self.drift_phase[c] += step;
                if self.drift_phase[c] >= 1.0 {
                    self.drift_phase[c] -= 1.0;
                    self.drift_to[c] = self.rng.next_bipolar();
                }
                self.drift[c] += (self.drift_to[c] - self.drift[c]) * (4.0 * step);
                tune += self.vibrato * self.drift[c];
            }
            let bow = exc[c] * BOW + self.noise;
            let x = if bow > 0.0 {
                let w = bow * self.bow_gain[c] * self.rng.next_bipolar();
                self.bow_lp[c] += (w - self.bow_lp[c]) * self.bow_k[c];
                self.bow_lp[c]
            } else {
                // No bow: the filter's residue fades with the string.
                self.bow_lp[c] *= 1.0 - self.bow_k[c];
                self.bow_lp[c]
            };
            let a = self.main[c].tick(x * self.input_gain[c][0], tune);
            let b = self.upper[c].tick(x * self.input_gain[c][1], tune);
            let y = a + UPPER * b;
            out[c] = y;
            let m = y.abs();
            let k = if m > self.level[c] { self.level_att } else { self.level_rel };
            self.level[c] += (m - self.level[c]) * k;
        }
    }

    pub(super) fn levels(&self) -> &[f32; CLASSES] {
        &self.level
    }
}
