//! An instrument's modulation LFO: a shaped oscillator (sine, triangle,
//! saw, square) or a random walk at the same rate (S&H: stepped renewal;
//! drift: eased toward each renewal), free-running or locked to tempo.
//! Voices that restart its phase at note-on read it at their own phase,
//! faded in by an onset ramp.

use core::f32::consts::TAU;

use super::lfo::{Lfo, LfoShape};
use super::random_walk::RandomWalk;

/// Beats per cycle of each tempo division: 1/1, 1/2, 1/4, 1/8, 1/16,
/// 1/4T, 1/8T.
pub const SYNC_BEATS: [f32; 7] = [4.0, 2.0, 1.0, 0.5, 0.25, 2.0 / 3.0, 1.0 / 3.0];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ModLfoShape {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleHold,
    Drift,
}

impl ModLfoShape {
    /// In the order instruments list them (SINE, TRI, SAW, SQR, S&H, DRIFT).
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Triangle,
            2 => Self::Saw,
            3 => Self::Square,
            4 => Self::SampleHold,
            5 => Self::Drift,
            _ => Self::Sine,
        }
    }

    /// The oscillator shape; the random shapes have none.
    pub fn basis(self) -> Option<LfoShape> {
        match self {
            Self::Sine => Some(LfoShape::Sine),
            Self::Triangle => Some(LfoShape::Triangle),
            Self::Saw => Some(LfoShape::Saw),
            Self::Square => Some(LfoShape::Square),
            Self::SampleHold | Self::Drift => None,
        }
    }
}

/// The rate: `free_hz`, or division `sync` (1-based into [`SYNC_BEATS`];
/// 0 = free) at `tempo_bpm`.
pub fn mod_lfo_hz(free_hz: f32, sync: usize, tempo_bpm: f32) -> f32 {
    match sync.checked_sub(1).and_then(|i| SYNC_BEATS.get(i)) {
        Some(beats) => (tempo_bpm / 60.0) / beats,
        None => free_hz,
    }
}

/// A note's onset ramp, 0 → 1 over `rise_ms` (a player's vibrato fades in).
pub fn rise(age_samples: u32, rise_ms: f32, sample_rate: f32) -> f32 {
    if rise_ms <= 0.0 {
        1.0
    } else {
        (age_samples as f32 / (rise_ms * 0.001 * sample_rate)).min(1.0)
    }
}

pub struct ModLfo {
    lfo: Lfo,
    walk: RandomWalk,
    shape: ModLfoShape,
    hz: f32,
    value: f32,
}

impl ModLfo {
    /// `walk_interval_ms` and `seed` start the random walk (its interval
    /// follows the rate from the first `set`).
    pub fn new(walk_interval_ms: f32, seed: u32) -> Self {
        Self {
            lfo: Lfo::default(),
            walk: RandomWalk::new(walk_interval_ms, 0.0, seed),
            shape: ModLfoShape::Sine,
            hz: 1.0,
            value: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32, hz: f32) {
        self.lfo.prepare(sample_rate, hz, 0.0);
        self.walk.prepare(sample_rate);
        self.hz = hz;
    }

    pub fn set(&mut self, hz: f32, shape: ModLfoShape, sample_rate: f32) {
        self.hz = hz;
        self.shape = shape;
        self.lfo.set_rate(hz);
        self.lfo.set_shape(shape.basis().unwrap_or(LfoShape::Sine));
        let interval_ms = 1_000.0 / hz.max(0.01);
        let ease_ms = if shape == ModLfoShape::Drift {
            interval_ms * 0.6
        } else {
            0.0
        };
        self.walk.set_times(interval_ms, ease_ms, sample_rate);
    }

    /// Run `frames` samples; the value at the last.
    pub fn advance(&mut self, frames: usize) -> f32 {
        let (mut oscillator, mut walk) = (self.value, self.value);
        for _ in 0..frames {
            oscillator = self.lfo.tick();
            walk = self.walk.tick();
        }
        self.value = match self.shape.basis() {
            Some(_) => oscillator,
            None => walk,
        };
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// The value at a voice's own phase (radians). The random shapes have
    /// no phase: every voice reads the shared walk.
    pub fn at_phase(&self, phase: f32) -> f32 {
        match self.shape.basis() {
            Some(basis) => Lfo::eval(basis, phase),
            None => self.value,
        }
    }

    /// A voice phase advanced by `seconds` at this rate, wrapped.
    pub fn advance_phase(&self, phase: f32, seconds: f32) -> f32 {
        (phase + TAU * self.hz * seconds) % TAU
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_divides_the_beat() {
        assert_eq!(mod_lfo_hz(3.0, 0, 120.0), 3.0, "free");
        assert_eq!(
            mod_lfo_hz(3.0, 3, 120.0),
            2.0,
            "1/4 at 120 bpm: two per second"
        );
        assert_eq!(mod_lfo_hz(3.0, 1, 120.0), 0.5, "1/1: one per bar");
    }

    #[test]
    fn random_shapes_read_the_shared_walk_at_any_phase() {
        let mut lfo = ModLfo::new(833.0, 7);
        lfo.prepare(48_000.0, 4.0);
        lfo.set(4.0, ModLfoShape::SampleHold, 48_000.0);
        let v = lfo.advance(4_800);
        assert_eq!(lfo.at_phase(1.0), v);
        lfo.set(4.0, ModLfoShape::Sine, 48_000.0);
        assert_eq!(
            lfo.at_phase(core::f32::consts::FRAC_PI_2),
            Lfo::eval(LfoShape::Sine, core::f32::consts::FRAC_PI_2)
        );
    }

    #[test]
    fn the_rise_ramps_from_note_on() {
        assert_eq!(rise(0, 0.0, 48_000.0), 1.0, "no rise");
        assert!((rise(240, 10.0, 48_000.0) - 0.5).abs() < 1e-6);
        assert_eq!(rise(4_800, 10.0, 48_000.0), 1.0);
    }
}
