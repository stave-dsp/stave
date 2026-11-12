//! Factory presets — each states the FULL face.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::RibbonParams;

struct Face {
    rate: f64,
    intensity: f64,
    drive: f64,
    heads: f64,
    wear: f64,
    erase: f64,
    spring: f64,
    mix: f64,
}

fn face(name: &'static str, p: &RibbonParams, f: Face) -> Preset {
    preset(
        name,
        &[
            (&p.rate, f.rate), (&p.intensity, f.intensity), (&p.drive, f.drive), (&p.heads, f.heads),
            (&p.wear, f.wear), (&p.erase, f.erase), (&p.spring, f.spring), (&p.mix, f.mix),
            (&p.brake, 0.0), (&p.output, 0.0),
        ],
    )
}

pub fn presets(p: &RibbonParams) -> Vec<Preset> {
    alloc::vec![
        face("INIT", p, Face {
            rate: 1.0, intensity: 0.4, drive: 0.25, heads: 6.0, wear: 0.0, erase: 1.0, spring: 0.3, mix: 0.4,
        }),
        // Classic dub: the long head, deep repeats, a splash of spring.
        face("DUB SIREN", p, Face {
            rate: 0.7, intensity: 0.72, drive: 0.4, heads: 2.0, wear: 0.25, erase: 1.0, spring: 0.45, mix: 0.45,
        }),
        // Slapback off head one, hot and short.
        face("SLAP", p, Face {
            rate: 1.4, intensity: 0.1, drive: 0.55, heads: 0.0, wear: 0.1, erase: 1.0, spring: 0.15, mix: 0.35,
        }),
        // All three heads galloping into the spring.
        face("GALLOP", p, Face {
            rate: 0.9, intensity: 0.55, drive: 0.3, heads: 6.0, wear: 0.15, erase: 1.0, spring: 0.6, mix: 0.45,
        }),
        // The erase head half-off: every pass lands on the last — a
        // slowly decaying archive of what you played.
        face("ARCHIVE", p, Face {
            rate: 0.8, intensity: 0.3, drive: 0.2, heads: 3.0, wear: 0.4, erase: 0.25, spring: 0.35, mix: 0.55,
        }),
        // Erase off, worn tape, slow motor: frippertronics on a dying deck.
        face("LOOP OF YEARS", p, Face {
            rate: 0.5, intensity: 0.15, drive: 0.15, heads: 6.0, wear: 0.65, erase: 0.0, spring: 0.4, mix: 0.6,
        }),
        // Runaway: right at the edge of self-oscillation.
        face("RUNAWAY", p, Face {
            rate: 1.0, intensity: 0.86, drive: 0.5, heads: 4.0, wear: 0.2, erase: 1.0, spring: 0.3, mix: 0.5,
        }),
    ]
}
