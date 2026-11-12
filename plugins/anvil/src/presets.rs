//! Factory presets — known places on the head. RHYTHM is the
//! load-time default (the metalcore brief); the others bracket the
//! space: the honest single tube path, and the drop-tuned extreme.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::AnvilParameters;

pub fn build_presets(parameters: &AnvilParameters) -> Vec<Preset> {
    alloc::vec![
        // The load-time brief, as a destination you can come back to.
        preset(
            "RHYTHM",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 18.0),
                (&parameters.input_low_cut, 120.0),
                (&parameters.bass_tone, 6.25),
                (&parameters.midrange_tone, 3.75),
                (&parameters.treble_tone, 6.666_667),
                (&parameters.presence_tone, 2.5),
                (&parameters.depth_tone, 2.5),
                (&parameters.topology_separation_amount, 0.5),
                (&parameters.difference_tone, 0.35),
                (&parameters.high_frequency_emphasis, 0.5),
                (&parameters.power_pair_overdrive, 12.0),
            ],
        ),
        // One honest tube path — the chug pumps the teeth, as in
        // every real amp (topology_separation_amount 0 = the classic rig alone).
        preset(
            "CLASSIC",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 14.0),
                (&parameters.input_low_cut, 90.0),
                (&parameters.bass_tone, 6.666_667),
                (&parameters.midrange_tone, 5.416_667),
                (&parameters.treble_tone, 6.25),
                (&parameters.presence_tone, 1.5),
                (&parameters.depth_tone, 1.5),
                (&parameters.topology_separation_amount, 0.0),
                (&parameters.difference_tone, 0.0),
                (&parameters.high_frequency_emphasis, 0.0),
                (&parameters.power_pair_overdrive, 8.0),
            ],
        ),
        // The drop-tuned extreme: full split, everything forward.
        preset(
            "SLUDGE",
            &[
                (&parameters.output_level, 0.0),
                (&parameters.preamp_gain, 24.0),
                (&parameters.input_low_cut, 65.0),
                (&parameters.bass_tone, 7.5),
                (&parameters.midrange_tone, 5.0),
                (&parameters.treble_tone, 5.833_333),
                (&parameters.presence_tone, 2.0),
                (&parameters.depth_tone, 4.0),
                (&parameters.topology_separation_amount, 1.0),
                (&parameters.difference_tone, 0.6),
                (&parameters.high_frequency_emphasis, 1.0),
                (&parameters.power_pair_overdrive, 16.0),
            ],
        ),
    ]
}
