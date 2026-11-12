//! Factory presets — known places in the cabinet space, each
//! stating the FULL musical surface (the house rule: applying one
//! is a destination, not a diff). BREAKDOWN is the flagship: the
//! ideal modern-metalcore cabinet, built from what the corpus and
//! the living leg taught — a cab that cannot exist, behaving like
//! the best of the ones that do.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::TolexParams;

pub fn presets(p: &TolexParams) -> Vec<Preset> {
    alloc::vec![
        // THE IDEAL MODERN METALCORE CAB. The recipe, justified:
        // a V30-class heavy cone (mass 1.15 — smoother top, breakup
        // seated lower) in a big CLOSED birch 4x12 (170 L: the air
        // spring tightens the chug; birch panels stay out of the
        // low-mids' way); resonance 92 with Q 0.7 — the alignment
        // lands the ~105 Hz palm-mute shelf TIGHT, not boomy;
        // damping 0.3 = a modern high-NFB power amp gripping the
        // cone (the tightness IS the amp-cab coupling); drive +6 —
        // the cone works (compression glues the chug, the even-order
        // voice warms it) without sagging; doping 0.45 tames fizz
        // above 5 kHz while breakup 0.55 keeps the 2-4 kHz teeth
        // that cut through a dense mix; the 421 just off the cap
        // (its corpus-distilled +3.7 dB at 3 kHz IS the metalcore bite)
        // (position 0.15, 2.5 cm) — chunk plus bite, no cap harshness.
        // Better-than-real: this exact combination (heavy-cone
        // smoothness + small-cab tightness + big-cab weight) is a
        // coupling no physical 4x12 offers.
        preset(
            "BREAKDOWN",
            &[
                (&p.output, 0.0),
                (&p.size, 2.0),
                (&p.resonance, 92.0),
                (&p.q, 0.7),
                (&p.mass, 1.05),
                (&p.cone, 21.0),
                (&p.breakup, 0.6),
                (&p.doping, 0.38),
                (&p.drive, 6.0),
                (&p.damping, 0.3),
                (&p.back, 0.0),
                (&p.cab, 170.0),
                (&p.material, 0.0),
                (&p.speakers, 2.0),
                (&p.model, 1.0),
                (&p.mic, 2.5),
                (&p.position, 0.15),
            ],
        ),
        // The house voice, exactly as the knobs default.
        preset(
            "INIT",
            &[
                (&p.output, 0.0),
                (&p.size, 2.0),
                (&p.resonance, 80.0),
                (&p.q, 0.85),
                (&p.mass, 1.0),
                (&p.cone, 0.0),
                (&p.breakup, 0.6),
                (&p.doping, 0.35),
                (&p.drive, 0.0),
                (&p.damping, 0.75),
                (&p.back, 0.0),
                (&p.cab, 60.0),
                (&p.material, 0.0),
                (&p.speakers, 0.0),
                (&p.model, 0.0),
                (&p.mic, 3.0),
                (&p.position, 0.25),
            ],
        ),
        // Vintage aggression: light greenback-class cones, closed
        // birch 4x12, no-NFB tube bloom, 57 walked toward the edge.
        preset(
            "PLEXI SHRED",
            &[
                (&p.output, 0.0),
                (&p.size, 2.0),
                (&p.resonance, 78.0),
                (&p.q, 0.95),
                (&p.mass, 0.9),
                (&p.cone, 4.0),
                (&p.breakup, 0.7),
                (&p.doping, 0.25),
                (&p.drive, 9.0),
                (&p.damping, 0.8),
                (&p.back, 0.0),
                (&p.cab, 120.0),
                (&p.material, 0.0),
                (&p.speakers, 2.0),
                (&p.model, 1.0),
                (&p.mic, 3.0),
                (&p.position, 0.3),
            ],
        ),
        // Open pine 1x12, big condenser pulled back — bloom and air.
        preset(
            "TWEED BLOOM",
            &[
                (&p.output, 0.0),
                (&p.size, 2.0),
                (&p.resonance, 82.0),
                (&p.q, 1.05),
                (&p.mass, 0.95),
                (&p.cone, 12.0),
                (&p.breakup, 0.65),
                (&p.doping, 0.2),
                (&p.drive, 3.0),
                (&p.damping, 0.95),
                (&p.back, 1.0),
                (&p.cab, 80.0),
                (&p.material, 1.0),
                (&p.speakers, 0.0),
                (&p.model, 7.0),
                (&p.mic, 8.0),
                (&p.position, 0.35),
            ],
        ),
    ]
}
