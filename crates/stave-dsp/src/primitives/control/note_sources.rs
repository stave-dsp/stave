//! The sources an instrument's modulation matrix reads. Per note: its
//! velocity, its key position, a stable per-note random (SPREAD — unison
//! detune generalized to any target), its age (the rise clock) and its
//! own LFO phases. Per ensemble: the last note played, HEAT (a fatigue
//! integrator of playing intensity — components warming up) and how much
//! the other voices ring (SYMP, sympathetic strings).

use crate::primitives::random::Xorshift32;

/// Where `hz` sits, ±1 across ±2.5 octaves around middle C.
pub fn key_position(hz: f32) -> f32 {
    ((hz / 261.6256).log2() / 2.5).clamp(-1.0, 1.0)
}

/// The other voices' level as this voice hears it, 0..1.
pub fn sympathy(total_level: f32, own_level: f32) -> f32 {
    ((total_level - own_level) / 3.0).clamp(0.0, 1.0)
}

pub struct NoteSources<const LFOS: usize> {
    pub velocity: f32,
    pub key: f32,
    /// ±1, drawn at note-on.
    pub spread: f32,
    /// Samples since note-on.
    pub age: u32,
    /// Each note-triggered LFO's phase in cycles, restarted at note-on.
    pub lfo_phases: [f32; LFOS],
    rng: Xorshift32,
}

impl<const LFOS: usize> NoteSources<LFOS> {
    /// A voice's own deterministic stream: notes are reproducible.
    pub fn new(seed: u32) -> Self {
        Self {
            velocity: 0.0,
            key: 0.0,
            spread: 0.0,
            age: 0,
            lfo_phases: [0.0; LFOS],
            // Xorshift absorbs at zero.
            rng: Xorshift32::new(seed | 1),
        }
    }

    pub fn note_on(&mut self, hz: f32, velocity: f32) {
        self.velocity = velocity;
        self.key = key_position(hz);
        self.age = 0;
        self.lfo_phases = [0.0; LFOS];
        self.spread = self.rng.next_bipolar();
    }

    pub fn tick(&mut self) {
        self.age = self.age.saturating_add(1);
    }
}

/// The matrix's standard sources for one voice, bipolar ±1, in source
/// order (OFF omitted): LFO, VEL, KEY, ENV, SPREAD, HEAT, SYMP. `lfo` is
/// the voice's LFO value with its rise applied.
pub fn standard_sources<const LFOS: usize>(
    lfo: f32,
    note: &NoteSources<LFOS>,
    envelope_level: f32,
    ensemble: &Ensemble,
    sympathy: f32,
) -> [f32; 7] {
    [
        lfo,
        note.velocity * 2.0 - 1.0,
        note.key,
        envelope_level * 2.0 - 1.0,
        note.spread,
        ensemble.heat * 2.0 - 1.0,
        sympathy * 2.0 - 1.0,
    ]
}

#[derive(Default)]
pub struct Ensemble {
    heat: f32,
    last_velocity: f32,
    last_key: f32,
}

impl Ensemble {
    /// Heat lost per second of play is e^(−1/τ) with τ = 8 s.
    const HEAT_SECONDS: f32 = 8.0;
    /// Heat a full-velocity note adds.
    const HEAT_PER_NOTE: f32 = 0.12;

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn note_on(&mut self, hz: f32, velocity: f32) {
        self.last_velocity = velocity;
        self.last_key = key_position(hz);
        self.heat = (self.heat + Self::HEAT_PER_NOTE * velocity).min(1.0);
    }

    /// Heat leaks over `seconds`.
    pub fn leak(&mut self, seconds: f32) {
        self.heat *= (-seconds / Self::HEAT_SECONDS).exp();
    }

    pub fn heat(&self) -> f32 {
        self.heat
    }

    /// The standard sources seen by the whole ensemble rather than one
    /// voice, in [`standard_sources`] order: per-note sources read the
    /// last note played, ENV and SYMP the ensemble's level, SPREAD is 0.
    /// For global-scope routes and for displays.
    pub fn sources(&self, lfo: f32, total_level: f32) -> [f32; 7] {
        let level = (total_level * 0.5).clamp(0.0, 1.0) * 2.0 - 1.0;
        [
            lfo,
            self.last_velocity * 2.0 - 1.0,
            self.last_key,
            level,
            0.0,
            self.heat * 2.0 - 1.0,
            level,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_c_is_the_center_and_two_and_a_half_octaves_the_edge() {
        assert!(key_position(261.6256).abs() < 1e-6);
        assert!((key_position(261.6256 * 2f32.powf(2.5)) - 1.0).abs() < 1e-5);
        assert_eq!(key_position(20_000.0), 1.0);
    }

    #[test]
    fn heat_rises_with_playing_and_leaks_away() {
        let mut e = Ensemble::default();
        for _ in 0..20 {
            e.note_on(440.0, 1.0);
        }
        assert_eq!(e.heat(), 1.0, "bounded");
        e.leak(8.0);
        assert!((e.heat() - (-1.0f32).exp()).abs() < 1e-6, "τ = 8 s");
    }

    #[test]
    fn a_note_restarts_its_clocks_and_draws_its_spread() {
        let mut n = NoteSources::<2>::new(7);
        n.lfo_phases = [1.0, 2.0];
        n.tick();
        n.note_on(440.0, 0.5);
        assert_eq!((n.age, n.lfo_phases, n.velocity), (0, [0.0, 0.0], 0.5));
        assert!((-1.0..=1.0).contains(&n.spread));
    }
}
