extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::HaloParameters;

pub fn build_presets(parameters: &HaloParameters) -> Vec<Preset> {
    alloc::vec![
        preset(
            "OPEN",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 6.0),
                (&parameters.bass_tone, 5.5),
                (&parameters.midrange_tone, 6.0),
                (&parameters.treble_tone, 5.5),
                (&parameters.headroom, 0.7),
                (&parameters.bloom, 0.45),
                (&parameters.reserve, 0.7),
                (&parameters.response, 0.6),
            ],
        ),
        preset(
            "GLASS",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 3.5),
                (&parameters.bass_tone, 4.5),
                (&parameters.midrange_tone, 4.0),
                (&parameters.treble_tone, 7.5),
                (&parameters.headroom, 0.95),
                (&parameters.bloom, 0.2),
                (&parameters.reserve, 0.9),
                (&parameters.response, 0.9),
            ],
        ),
        preset(
            "BLOOM",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 9.0),
                (&parameters.bass_tone, 6.0),
                (&parameters.midrange_tone, 7.0),
                (&parameters.treble_tone, 5.0),
                (&parameters.headroom, 0.55),
                (&parameters.bloom, 0.9),
                (&parameters.reserve, 0.65),
                (&parameters.response, 0.25),
            ],
        ),
        preset(
            "PIANO",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 5.0),
                (&parameters.bass_tone, 6.5),
                (&parameters.midrange_tone, 6.5),
                (&parameters.treble_tone, 5.0),
                (&parameters.headroom, 0.85),
                (&parameters.bloom, 0.35),
                (&parameters.reserve, 1.0),
                (&parameters.response, 0.75),
            ],
        ),
    ]
}
