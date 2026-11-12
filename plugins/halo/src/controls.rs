//! The explicit boundary between Halo's panel and its engine.

use crate::params::HaloParameters;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelControlValues {
    pub bass_tone_position: f32,
    pub midrange_tone_position: f32,
    pub treble_tone_position: f32,
    pub headroom_amount: f32,
    pub bloom_amount: f32,
    pub reserve_amount: f32,
    pub response_amount: f32,
}

stave_core::continuous_controls! {
    /// The panel as the processor renders it: each knob glides as its
    /// param declares.
    pub struct Panel<HaloParameters> => PanelRamps, PanelValues {
        bass_tone, midrange_tone, treble_tone, headroom, bloom, reserve, response,
    }
}

impl From<PanelValues> for PanelControlValues {
    fn from(v: PanelValues) -> Self {
        Self {
            bass_tone_position: v.bass_tone,
            midrange_tone_position: v.midrange_tone,
            treble_tone_position: v.treble_tone,
            headroom_amount: v.headroom,
            bloom_amount: v.bloom,
            reserve_amount: v.reserve,
            response_amount: v.response,
        }
    }
}

impl PanelControlValues {
    pub fn read(parameters: &HaloParameters) -> Self {
        Self {
            bass_tone_position: parameters.bass_tone.value(),
            midrange_tone_position: parameters.midrange_tone.value(),
            treble_tone_position: parameters.treble_tone.value(),
            headroom_amount: parameters.headroom.value(),
            bloom_amount: parameters.bloom.value(),
            reserve_amount: parameters.reserve.value(),
            response_amount: parameters.response.value(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HaloDesign {
    pub input_highpass_cutoff_hz: f32,
    pub input_lowpass_cutoff_hz: f32,
    pub reference_input_level_db: f32,
    pub target_output_level_db: f32,
    pub power_pair_cold_bias_volts: f64,
}

impl Default for HaloDesign {
    fn default() -> Self {
        Self {
            input_highpass_cutoff_hz: 24.0,
            input_lowpass_cutoff_hz: 22_000.0,
            reference_input_level_db: -12.0,
            target_output_level_db: -5.0,
            power_pair_cold_bias_volts: 0.65,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputSettings {
    pub highpass_cutoff_hz: f32,
    pub lowpass_cutoff_hz: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreampSettings {
    pub first_stage_drive_db: f32,
    pub recovery_stage_drive_db: f32,
    pub cathode_bloom_depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneSettings {
    pub bass_control_position: f32,
    pub midrange_control_position: f32,
    pub treble_control_position: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicSettings {
    pub headroom_amount: f32,
    pub bloom_amount: f32,
    pub reserve_amount: f32,
    pub power_pair_drive_db: f32,
    pub negative_feedback_db: f32,
    pub supply_droop_amount: f32,
    pub supply_droop_time_ms: f32,
    pub supply_recovery_time_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineSettings {
    pub input: InputSettings,
    pub preamp: PreampSettings,
    pub tone: ToneSettings,
    pub dynamics: DynamicSettings,
}

pub fn resolve_engine_settings(panel: PanelControlValues, design: &HaloDesign) -> EngineSettings {
    let response = panel.response_amount.clamp(0.0, 1.0);
    let bloom = panel.bloom_amount.clamp(0.0, 1.0);
    let headroom = panel.headroom_amount.clamp(0.0, 1.0);
    EngineSettings {
        input: InputSettings {
            highpass_cutoff_hz: design.input_highpass_cutoff_hz,
            lowpass_cutoff_hz: design.input_lowpass_cutoff_hz,
        },
        preamp: PreampSettings {
            first_stage_drive_db: 1.9,
            recovery_stage_drive_db: 0.7,
            cathode_bloom_depth: 0.15 + 0.7 * bloom,
        },
        tone: ToneSettings {
            bass_control_position: panel.bass_tone_position,
            midrange_control_position: panel.midrange_tone_position,
            treble_control_position: panel.treble_tone_position,
        },
        dynamics: DynamicSettings {
            headroom_amount: headroom,
            bloom_amount: bloom,
            reserve_amount: panel.reserve_amount.clamp(0.0, 1.0),
            power_pair_drive_db: 9.0 - 7.5 * headroom,
            negative_feedback_db: 6.0 + 10.0 * response,
            supply_droop_amount: bloom * (0.35 - 0.28 * response),
            supply_droop_time_ms: 8.0 + 24.0 * (1.0 - response),
            supply_recovery_time_ms: 75.0 + 205.0 * (1.0 - response),
        },
    }
}

#[derive(Clone, Debug)]
pub struct LastAppliedSettings<T> {
    value: Option<T>,
}

impl<T> LastAppliedSettings<T> {
    pub const fn new() -> Self {
        Self { value: None }
    }

    pub fn reset(&mut self) {
        self.value = None;
    }
}

impl<T: PartialEq + Copy> LastAppliedSettings<T> {
    pub fn has_changed(&mut self, value: T) -> bool {
        let changed = self.value != Some(value);
        self.value = Some(value);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headroom_and_response_map_to_distinct_engine_quantities() {
        let design = HaloDesign::default();
        let base = PanelControlValues {
            bass_tone_position: 5.5,
            midrange_tone_position: 6.0,
            treble_tone_position: 5.5,
            headroom_amount: 0.0,
            bloom_amount: 0.5,
            reserve_amount: 0.7,
            response_amount: 0.0,
        };
        let elastic = resolve_engine_settings(base, &design);
        let stiff = resolve_engine_settings(
            PanelControlValues {
                response_amount: 1.0,
                ..base
            },
            &design,
        );
        assert!(stiff.dynamics.negative_feedback_db > elastic.dynamics.negative_feedback_db);
        assert!(stiff.dynamics.supply_droop_amount < elastic.dynamics.supply_droop_amount);
        assert_eq!(stiff.preamp, elastic.preamp);

        let high_headroom = resolve_engine_settings(
            PanelControlValues {
                headroom_amount: 1.0,
                ..base
            },
            &design,
        );
        assert!(high_headroom.dynamics.power_pair_drive_db < elastic.dynamics.power_pair_drive_db);
    }

    #[test]
    fn fixed_preamp_stages_do_not_change_with_tone_controls() {
        let design = HaloDesign::default();
        let base = PanelControlValues {
            bass_tone_position: 5.5,
            midrange_tone_position: 6.0,
            treble_tone_position: 0.0,
            headroom_amount: 0.7,
            bloom_amount: 0.45,
            reserve_amount: 0.7,
            response_amount: 0.6,
        };
        let quiet_and_dark = resolve_engine_settings(base, &design);
        let bright = resolve_engine_settings(
            PanelControlValues {
                treble_tone_position: 10.0,
                ..base
            },
            &design,
        );

        assert_eq!(
            bright.preamp.first_stage_drive_db,
            quiet_and_dark.preamp.first_stage_drive_db
        );
        assert_eq!(
            bright.preamp.recovery_stage_drive_db,
            quiet_and_dark.preamp.recovery_stage_drive_db
        );
    }
}
