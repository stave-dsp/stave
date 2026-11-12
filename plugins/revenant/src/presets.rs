//! Factory presets — each states the FULL face.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::RevenantParams;

struct Face {
    learn: f64,
    memory: f64,
    chaos: f64,
    horizon: f64,
    dream: f64,
    mix: f64,
}

fn face(name: &'static str, p: &RevenantParams, f: Face) -> Preset {
    preset(
        name,
        &[
            (&p.learn, f.learn), (&p.memory, f.memory), (&p.chaos, f.chaos), (&p.horizon, f.horizon),
            (&p.dream, f.dream), (&p.mix, f.mix), (&p.sleep, 0.0), (&p.output, 0.0),
        ],
    )
}

pub fn presets(p: &RevenantParams) -> Vec<Preset> {
    alloc::vec![
        face("INIT", p, Face { learn: 0.6, memory: 0.6, chaos: 0.45, horizon: 3.0, dream: 0.0, mix: 0.5 }),
        // A close shadow: it learns fast and guesses only a hair ahead.
        face("SHADOW", p, Face { learn: 0.85, memory: 0.3, chaos: 0.4, horizon: 0.6, dream: 0.0, mix: 0.45 }),
        // It must imagine 20 ms ahead: smeared, half-right futures.
        face("PROPHET", p, Face { learn: 0.7, memory: 0.6, chaos: 0.55, horizon: 20.0, dream: 0.0, mix: 0.6 }),
        // Half listening, half hearing itself: it wanders off your line.
        face("DAYDREAM", p, Face { learn: 0.6, memory: 0.5, chaos: 0.55, horizon: 4.0, dream: 0.5, mix: 0.55 }),
        // At the edge of chaos: it rings with its own life between notes.
        face("EDGE", p, Face { learn: 0.5, memory: 0.35, chaos: 0.62, horizon: 3.0, dream: 0.15, mix: 0.5 }),
        // Past the edge, slow memory, mostly dreaming: a haunting.
        face("HAUNTING", p, Face { learn: 0.4, memory: 0.75, chaos: 0.8, horizon: 8.0, dream: 0.7, mix: 0.6 }),
    ]
}
