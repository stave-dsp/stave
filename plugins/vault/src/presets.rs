//! Factory presets — the character space's corners, each a PLACE
//! with a reason. Every preset states the FULL musical surface
//! (applying one is a known place, not a diff). The characters
//! lean on the axes the field asked for: per-band decay (bass),
//! static color (tone), and scattering density (diffuse — low is
//! discrete vintage echoes AS a character, not a defect).

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::VaultParams;

#[allow(clippy::too_many_arguments)]
pub fn presets(p: &VaultParams) -> Vec<Preset> {
    // (mix, size, decay_ms, predelay, early, damp, bass, tone,
    //  diffuse, motion, wow, drive, width)
    #[allow(clippy::too_many_arguments)]
    let full = |mix: f64,
                size: f64,
                decay: f64,
                predelay: f64,
                early: f64,
                damp: f64,
                bass: f64,
                tone: f64,
                diffuse: f64,

                width: f64,
                stiff: f64,
                duck: f64,
                stand: f64,
                material: f64,
                topology: f64,
                ascend: f64| {
        alloc::vec![
            (&p.mix as &dyn stave_core::params::AnyParam, mix),
            (&p.size, size),
            (&p.decay, decay),
            (&p.predelay, predelay),
            (&p.early, early),
            (&p.damp, damp),
            (&p.bass, bass),
            (&p.tone, tone),
            (&p.diffuse, diffuse),
            (&p.width, width),
            (&p.stiff, stiff),
            (&p.duck, duck),
            (&p.stand, stand),
            (&p.material, material),
            (&p.topology, topology),
            (&p.ascend, ascend),
            (&p.output, 0.0),
        ]
    };
    let cathedral = full(
        0.38, 2.7, 7_000.0, 45.0, 0.35, 0.42, 1.9, -0.15, 0.85, 1.15, 0.0, 0.15, 0.3, 2.0, 0.0, 0.0,
    );
    let wood_room = full(
        0.30, 0.5, 500.0, 4.0, 0.85, 0.55, 0.75, -0.2, 0.6, 0.8, 0.0, 0.2, -0.35, 0.0, 0.0, 0.0,
    );
    let bright_plate = full(
        0.32, 0.85, 2_800.0, 10.0, 0.0, 0.08, 0.9, 0.55, 0.95, 1.3, 0.65, 0.3, 0.0, 5.0, 1.0, 0.0,
    );
    let dark_plate = full(
        0.32, 0.9, 3_600.0, 20.0, 0.0, 0.7, 1.5, -0.6, 0.9, 1.0, 0.55, 0.3, 0.0, 5.0, 1.0, 0.0,
    );
    let frozen_choir = full(
        0.5, 1.6, 20_000.0, 0.0, 0.0, 0.15, 1.2, 0.2, 0.8, 1.3, 0.0, 0.2, 0.0, 1.0, 2.0, 0.75,
    );
    let the_memory = full(
        0.4, 1.0, 8_000.0, 0.0, 0.0, 0.25, 1.0, 0.0, 0.7, 1.0, 0.0, 0.15, 0.0, 1.0, 2.0, 0.0,
    );
    let gated_snare = full(
        0.45, 1.0, 350.0, 0.0, 0.0, 0.2, 0.9, 0.3, 0.85, 1.2, 0.0, 0.0, 0.0, 1.0, 3.0, 0.0,
    );
    let open_void = full(
        0.5, 2.0, 14_000.0, 0.0, 0.0, 0.2, 1.1, 0.1, 0.9, 1.4, 0.0, 0.15, 0.0, 1.0, 4.0, 0.0,
    );
    let chamber = full(
        0.28, 1.0, 1_400.0, 16.0, 0.5, 0.35, 1.1, 0.1, 0.75, 0.9, 0.0, 0.45, 0.25, 1.0, 0.0, 0.0,
    );
    let warehouse = full(
        0.35, 1.6, 2_200.0, 30.0, 0.4, 0.6, 1.2, -0.35, 0.65, 1.1, 0.3, 0.4, -0.5, 2.0, 0.0, 0.0,
    );
    let canyon = full(
        0.40, 2.6, 3_200.0, 60.0, 0.8, 0.3, 1.0, 0.0, 0.08, 1.5, 0.0, 0.5, 0.6, 2.0, 0.0, 0.0,
    );
    let airlock = full(
        0.42, 3.0, 14_000.0, 70.0, 0.15, 0.25, 1.6, 0.2, 0.9, 1.35, 0.0, 0.25, 0.15, 3.0, 0.0, 0.0,
    );
    alloc::vec![
        preset("STONE CATHEDRAL", &cathedral),
        preset("WOOD ROOM", &wood_room),
        preset("BRIGHT PLATE", &bright_plate),
        preset("DARK PLATE", &dark_plate),
        preset("FROZEN CHOIR", &frozen_choir),
        preset("THE MEMORY", &the_memory),
        preset("GATED SNARE", &gated_snare),
        preset("OPEN VOID", &open_void),
        preset("CHAMBER", &chamber),
        preset("WAREHOUSE", &warehouse),
        preset("ECHO CANYON", &canyon),
        preset("AIRLOCK", &airlock),
    ]
}
