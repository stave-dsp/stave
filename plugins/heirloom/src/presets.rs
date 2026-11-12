//! Factory presets — corners of the character space, each stating
//! the FULL face (knobs, toggles, footswitch) so loading one never
//! inherits a stray setting.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::HeirloomParams;

/// One full face. `machine` / `warp` / `media` are step indices.
struct Face {
    time: f64,
    repeats: f64,
    mix: f64,
    wobble: f64,
    rpm: f64,
    age: f64,
    machine: f64,
    warp: f64,
    media: f64,
}

fn face(name: &'static str, p: &HeirloomParams, f: Face) -> Preset {
    preset(
        name,
        &[
            (&p.time, f.time), (&p.repeats, f.repeats), (&p.mix, f.mix),
            (&p.wobble, f.wobble), (&p.rpm, f.rpm), (&p.age, f.age),
            (&p.machine, f.machine), (&p.warp, f.warp), (&p.media, f.media),
            (&p.hold, 0.0), (&p.sync, 0.0), (&p.output, 0.0),
        ],
    )
}

pub fn presets(p: &HeirloomParams) -> Vec<Preset> {
    alloc::vec![
        // The defaults: a clean record (AGE is the wear you add).
        face("INIT", p, Face {
            time: 450.0, repeats: 0.5, mix: 0.45, wobble: 0.3, rpm: 0.8, age: 0.0,
            machine: 0.0, warp: 0.0, media: 0.0,
        }),
        // A reverse swell off a dusty LP: slow warp, crackle under it.
        face("ATTIC LP", p, Face {
            time: 640.0, repeats: 0.6, mix: 0.55, wobble: 0.45, rpm: 0.55, age: 0.45,
            machine: 0.0, warp: 1.0, media: 0.0,
        }),
        // Octave shimmer dubbed to a cassette that's been in a hot car.
        face("HOT CAR", p, Face {
            time: 300.0, repeats: 0.7, mix: 0.5, wobble: 0.35, rpm: 1.6, age: 0.6,
            machine: 1.0, warp: 2.0, media: 1.0,
        }),
        // BLOOM on a third-generation VHS dub: wide, narrow, wandering.
        face("HOME MOVIE", p, Face {
            time: 800.0, repeats: 0.55, mix: 0.7, wobble: 0.25, rpm: 0.35, age: 0.7,
            machine: 2.0, warp: 2.0, media: 2.0,
        }),
        // Mostly Warped Vinyl: half wet, so the platter beats against
        // the dry; one far-off reverse repeat underneath.
        face("SEASICK", p, Face {
            time: 1200.0, repeats: 0.0, mix: 0.5, wobble: 0.7, rpm: 2.4, age: 0.1,
            machine: 0.0, warp: 0.0, media: 0.0,
        }),
        // Mostly Generation Loss: all wet, through a dying deck; one
        // far-off reverse repeat.
        face("DYING DECK", p, Face {
            time: 1200.0, repeats: 0.0, mix: 1.0, wobble: 0.1, rpm: 0.5, age: 0.85,
            machine: 0.0, warp: 2.0, media: 1.0,
        }),
        // Reverse loops held near forever, barely worn — a drone bed.
        face("ETERNAL", p, Face {
            time: 1000.0, repeats: 0.93, mix: 0.75, wobble: 0.2, rpm: 0.2, age: 0.2,
            machine: 2.0, warp: 0.0, media: 0.0,
        }),
    ]
}
