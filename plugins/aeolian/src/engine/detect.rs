//! `Detector` — which notes are being played, as twelve pitch-CLASS
//! levels. Each class is five narrow band-passes, one per octave
//! (C2 … B6), summed: a played E in any octave lands in the E band.
//! What leaves is a LEVEL per class — how hard that note is being
//! played — never the band signal itself: a string only builds up at
//! its own harmonics, so an E2 cannot ring an E4 string directly. The
//! strings turn the level into a bow (strings.rs).
//!
//! WIND is velocity: it sets the level a band must reach to open its
//! gate (−18 dBFS at 0 … −66 dBFS at 1). A class must also stand out
//! from the loudest one, and each class is inhibited by its louder
//! semitone neighbour — the band-passes leak, and leakage must not
//! strum the whole harp.

use stave_dsp::Svf;

pub(super) const CLASSES: usize = 12;
const OCTAVES: usize = 5;
const LOW_OCTAVE: i32 = 2;
/// Band-pass Q — narrow enough to tell neighbours apart at C2
/// (semitone ≈ 6%), wide enough to catch a vibrato'd note.
const Q: f32 = 28.0;
/// Slow enough that a pick's broadband click does not open every band.
const ATTACK_MS: f32 = 15.0;
const RELEASE_MS: f32 = 140.0;
/// The gate's knee: fully open this many dB above threshold.
const KNEE_DB: f32 = 12.0;
/// How much a semitone neighbour's level is subtracted (lateral
/// inhibition — the leak into the next band is ~−10 dB at Q 28).
const INHIBIT: f32 = 0.8;

/// C0, the root of every pitch the plugin names.
pub(super) const C0_HZ: f32 = 16.351_6;

pub(super) fn class_hz(class: usize, octave: i32) -> f32 {
    C0_HZ * 2f32.powf(octave as f32 + class as f32 / 12.0)
}

pub(super) struct Detector {
    bands: [[Svf; OCTAVES]; CLASSES],
    norm: f32,
    env: [f32; CLASSES],
    att: f32,
    rel: f32,
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Detector {
    pub(super) fn new() -> Self {
        Self {
            bands: core::array::from_fn(|_| core::array::from_fn(|_| Svf::new())),
            norm: 1.0 / Q,
            env: [0.0; CLASSES],
            att: 0.0,
            rel: 0.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        let res = 1.0 - 1.0 / (2.0 * Q);
        for (c, row) in self.bands.iter_mut().enumerate() {
            for (k, f) in row.iter_mut().enumerate() {
                f.prepare(sr);
                f.set(class_hz(c, LOW_OCTAVE + k as i32).min(0.45 * sr), res);
            }
        }
        self.env = [0.0; CLASSES];
        self.att = 1.0 - (-1000.0 / (ATTACK_MS * sr)).exp();
        self.rel = 1.0 - (-1000.0 / (RELEASE_MS * sr)).exp();
    }

    /// One frame of mono input → each class's drive level (its band
    /// envelope, gated by WIND, dominance and its neighbours).
    #[inline]
    pub(super) fn tick(&mut self, x: f32, wind: f32, out: &mut [f32; CLASSES]) {
        let mut loudest = 0.0f32;
        for c in 0..CLASSES {
            let mut s = 0.0;
            for f in self.bands[c].iter_mut() {
                s += f.process(x, 0).bp;
            }
            let a = (s * self.norm).abs();
            let k = if a > self.env[c] { self.att } else { self.rel };
            self.env[c] += (a - self.env[c]) * k;
            loudest = loudest.max(self.env[c]);
        }
        let thr_db = -18.0 - 48.0 * wind;
        for c in 0..CLASSES {
            let near = self.env[(c + 1) % CLASSES].max(self.env[(c + CLASSES - 1) % CLASSES]);
            let e = (self.env[c] - INHIBIT * near).max(0.0);
            if e <= 1e-9 {
                out[c] = 0.0;
                continue;
            }
            let db = 20.0 * e.log10();
            let gate = smoothstep(thr_db, thr_db + KNEE_DB, db) * smoothstep(0.45, 0.8, e / loudest);
            out[c] = e * gate;
        }
    }
}
