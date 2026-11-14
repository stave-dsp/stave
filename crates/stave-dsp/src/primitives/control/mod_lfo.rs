//! An instrument's modulation LFO: a shaped oscillator (sine, triangle,
//! saw, square) or a random walk at the same rate (S&H: stepped renewal;
//! drift: eased toward each renewal), free-running or locked to tempo.
//! Voices that restart its phase at note-on read it at their own phase,
//! faded in by an onset ramp.

use super::lfo::Lfo;
use super::random_walk::RandomWalk;
use crate::kernel::control::Source;
use crate::kernel::node::{Generator, Stage, Tail};
use crate::primitives::osc::Waveform;
use crate::units::Hz;
use crate::units::Seconds;
use crate::units::ms;

/// Beats per cycle of each tempo division: 1/1, 1/2, 1/4, 1/8, 1/16,
/// 1/4T, 1/8T.
pub const SYNC_BEATS: [f32; 7] = [4.0, 2.0, 1.0, 0.5, 0.25, 2.0 / 3.0, 1.0 / 3.0];

/// What a [`ModLfo`] draws: a periodic [`Waveform`], or a random walk at
/// the same rate.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ModLfoShape {
    Wave(Waveform),
    /// Stepped renewal once per cycle.
    SampleHold,
    /// Eased toward each renewal.
    Drift,
}

impl ModLfoShape {
    /// In the order instruments list them (SINE, TRI, SAW, SQR, S&H, DRIFT).
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Wave(Waveform::Triangle),
            2 => Self::Wave(Waveform::Saw),
            3 => Self::Wave(Waveform::Square),
            4 => Self::SampleHold,
            5 => Self::Drift,
            _ => Self::Wave(Waveform::Sine),
        }
    }
}

/// The LFO rate: `free`, or the tempo division at `sync` (1-based
/// into [`SYNC_BEATS`]; 0 = free).
pub fn mod_lfo_rate(free: Hz, sync: usize, tempo_bpm: f32) -> Hz {
    match sync.checked_sub(1).and_then(|i| SYNC_BEATS.get(i)) {
        Some(beats) => Hz::new((tempo_bpm / 60.0) / beats),
        None => free,
    }
}

/// The onset ramp: 0 → 1 over `rise_ms` from note-on.
pub fn rise(age_samples: u32, rise: Seconds, sample_rate: f32) -> f32 {
    let rise_ms = rise.to_ms() as f32;
    if rise_ms <= 0.0 {
        1.0
    } else {
        (age_samples as f32 / (rise_ms * 0.001 * sample_rate)).min(1.0)
    }
}

#[derive(Clone)]
pub struct ModLfo {
    lfo: Lfo,
    walk: RandomWalk,
    shape: ModLfoShape,
    hz: f32,
    value: f32,
}

impl ModLfo {
    pub fn new(walk_interval: Seconds, seed: u32) -> Self {
        Self {
            lfo: Lfo::default(),
            walk: RandomWalk::new(ms(f64::from(walk_interval)), ms(f64::from(Seconds::new(0.0))), seed),
            shape: ModLfoShape::Wave(Waveform::Sine),
            hz: 1.0,
            value: 0.0,
        }
    }

    /// Rate and shape (block rate).
    pub fn set(&mut self, rate: Hz, shape: ModLfoShape) {
        let hz = rate.get();
        self.hz = hz;
        self.shape = shape;
        self.lfo.set_frequency(Hz::new(hz));
        if let ModLfoShape::Wave(w) = shape {
            self.lfo.set_waveform(w);
        }
        let interval_ms = 1_000.0 / hz.max(0.01);
        let ease_ms = if shape == ModLfoShape::Drift {
            interval_ms * 0.6
        } else {
            0.0
        };
        self.walk.set_times(ms(f64::from(interval_ms)), ms(f64::from(ease_ms)));
    }

    /// Run `frames` samples and return the value at the end (block-rate
    /// use: one read per block).
    pub fn tick_frames(&mut self, frames: usize) -> f32 {
        let (mut oscillator, mut walk) = (self.value, self.value);
        for _ in 0..frames {
            oscillator = self.lfo.next();
            walk = self.walk.next();
        }
        self.value = match self.shape {
            ModLfoShape::Wave(_) => oscillator,
            _ => walk,
        };
        self.value
    }

    /// The shape at a voice's own `phase` (cycles); a random shape
    /// reads the shared walk.
    pub fn at_phase(&self, phase: f32) -> f32 {
        match self.shape {
            ModLfoShape::Wave(w) => w.eval(phase),
            _ => self.value,
        }
    }

    /// `phase` (cycles) moved on by `seconds` at this LFO's rate.
    pub fn advance_phase(&self, phase: f32, elapsed: Seconds) -> f32 {
        let seconds = elapsed.get() as f32;
        (phase + self.hz * seconds).rem_euclid(1.0)
    }
}

impl Stage for ModLfo {
    fn prepare(&mut self, sample_rate: f32) {
        self.lfo.prepare(sample_rate);
        self.walk.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.lfo.reset();
        self.walk.reset();
        self.value = 0.0;
    }
    /// One sample (input ignored).
    fn tick(&mut self, _: f32) -> f32 {
        self.tick_frames(1)
    }
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for ModLfo {}

impl Source for ModLfo {
    fn state(&self) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_divides_the_beat() {
        assert_eq!(mod_lfo_rate(Hz::new(3.0), 0, 120.0).get(), 3.0, "free");
        assert_eq!(
            mod_lfo_rate(Hz::new(3.0), 3, 120.0).get(),
            2.0,
            "1/4 at 120 bpm: two per second"
        );
        assert_eq!(mod_lfo_rate(Hz::new(3.0), 1, 120.0).get(), 0.5, "1/1: one per bar");
    }

    #[test]
    fn random_shapes_read_the_shared_walk_at_any_phase() {
        let mut lfo = ModLfo::new(ms(833.0), 7);
        lfo.prepare(48_000.0);
        lfo.set(Hz::new(4.0), ModLfoShape::SampleHold);
        let v = lfo.tick_frames(4_800);
        assert_eq!(lfo.at_phase(0.16), v);
        lfo.set(Hz::new(4.0), ModLfoShape::Wave(Waveform::Sine));
        assert_eq!(lfo.at_phase(0.25), Waveform::Sine.eval(0.25));
    }

    #[test]
    fn the_rise_ramps_from_note_on() {
        assert_eq!(rise(0, ms(0.0), 48_000.0), 1.0, "no rise");
        assert!((rise(240, ms(10.0), 48_000.0) - 0.5).abs() < 1e-6);
        assert_eq!(rise(4_800, ms(10.0), 48_000.0), 1.0);
    }
}
