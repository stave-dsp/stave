//! Factory presets — beautiful to experimental, melodic to percussive.
//! Each states the FULL face (the node drawing stays as the user left
//! it: presets shape the memory and the room, not your wiring).

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::EngramParams;

struct Face {
    size: f64,
    decay: f64,
    tone: f64,
    recall: f64,
    focus: f64,
    deja: f64,
    matching: f64,
    texture: f64,
    plasticity: f64,
    mix: f64,
}

fn face(name: &'static str, p: &EngramParams, f: Face) -> Preset {
    preset(
        name,
        &[
            (&p.size, f.size), (&p.decay, f.decay), (&p.tone, f.tone), (&p.recall, f.recall),
            (&p.focus, f.focus), (&p.deja, f.deja), (&p.matching, f.matching), (&p.texture, f.texture),
            (&p.plasticity, f.plasticity), (&p.mix, f.mix), (&p.hold, 0.0), (&p.output, 0.0),
        ],
    )
}

pub fn presets(p: &EngramParams) -> Vec<Preset> {
    alloc::vec![
        face("INIT", p, Face { size: 0.5, decay: 3.0, tone: 0.6, recall: 0.5, focus: 0.6, deja: 4.0, matching: 0.3, texture: 0.15, plasticity: 0.3, mix: 0.35 }),
        // — beautiful —
        // Return to a chord and the hall answers with the first time you
        // played it, seamlessly, a halo behind you.
        face("HALO OF CHORDS", p, Face { size: 0.75, decay: 6.0, tone: 0.55, recall: 0.8, focus: 0.8, deja: 8.0, matching: 0.0, texture: 0.05, plasticity: 0.3, mix: 0.45 }),
        // Every time you were near this harmony, all at once.
        face("COLLECTIVE", p, Face { size: 0.85, decay: 9.0, tone: 0.45, recall: 0.9, focus: 0.1, deja: 2.0, matching: 0.15, texture: 0.25, plasticity: 0.5, mix: 0.5 }),
        // Only moments over half a minute old may answer.
        face("LONG AGO", p, Face { size: 0.75, decay: 6.0, tone: 0.4, recall: 1.0, focus: 0.7, deja: 30.0, matching: 0.2, texture: 0.1, plasticity: 0.4, mix: 0.5 }),
        // — percussive —
        // A tight room that answers each of your hits with a remembered
        // one: call and response with your own past.
        face("CALL & RESPONSE", p, Face { size: 0.2, decay: 0.8, tone: 0.7, recall: 0.9, focus: 0.85, deja: 2.0, matching: 0.9, texture: 0.1, plasticity: 0.2, mix: 0.45 }),
        // Kits remembered by sound: the snare answers snares, the hats
        // hats — in a big, dark space.
        face("DRUM GHOSTS", p, Face { size: 0.6, decay: 2.5, tone: 0.45, recall: 0.8, focus: 0.7, deja: 4.0, matching: 0.8, texture: 0.3, plasticity: 0.3, mix: 0.4 }),
        // — experimental —
        // Scattered: remembered moments as a granular cloud.
        face("CLOUD MEMORY", p, Face { size: 0.7, decay: 5.0, tone: 0.6, recall: 0.9, focus: 0.4, deja: 3.0, matching: 0.5, texture: 0.6, plasticity: 0.5, mix: 0.5 }),
        // Shattered: short grains, reversed, leaping octaves — every
        // memory at once, by sound and groove.
        face("SHATTERED", p, Face { size: 0.55, decay: 3.5, tone: 0.7, recall: 1.0, focus: 0.2, deja: 1.0, matching: 0.7, texture: 1.0, plasticity: 0.8, mix: 0.55 }),
        // Echo of now: no déjà gap, rhythm-matched, mid texture — it
        // re-plays your groove back at you, slightly wrong.
        face("ECHO OF NOW", p, Face { size: 0.4, decay: 2.0, tone: 0.65, recall: 0.8, focus: 0.75, deja: 0.0, matching: 0.6, texture: 0.4, plasticity: 0.2, mix: 0.45 }),
    ]
}
