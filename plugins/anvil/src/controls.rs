//! The boundary between Anvil's panel and its amplifier design.
//!
//! Panel values are host state. [`AnvilDesign`] is the product's fixed
//! construction. [`resolve_engine_settings`] is the only place a player
//! control may map to more than one engine quantity.

use crate::params::AnvilParameters;

/// The player-facing state, in the units printed on the panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelControlValues {
    pub preamp_gain_db: f32,
    pub input_low_cut_hz: f32,
    pub bass_tone_position: f32,
    pub midrange_tone_position: f32,
    pub treble_tone_position: f32,
    pub presence_tone_db: f32,
    pub depth_tone_db: f32,
    pub topology_separation_amount: f32,
    pub power_pair_overdrive_db: f32,
    pub difference_tone_amount: f32,
    pub high_frequency_emphasis_amount: f32,
}

stave_core::continuous_controls! {
    /// The panel as the processor renders it: each knob glides as its
    /// param declares.
    pub struct Panel<AnvilParameters> => PanelRamps, PanelValues {
        preamp_gain,
        input_low_cut,
        bass_tone,
        midrange_tone,
        treble_tone,
        presence_tone,
        depth_tone,
        topology_separation_amount,
        power_pair_overdrive,
        difference_tone,
        high_frequency_emphasis,
    }
}

impl From<PanelValues> for PanelControlValues {
    fn from(v: PanelValues) -> Self {
        Self {
            preamp_gain_db: v.preamp_gain,
            input_low_cut_hz: v.input_low_cut,
            bass_tone_position: v.bass_tone,
            midrange_tone_position: v.midrange_tone,
            treble_tone_position: v.treble_tone,
            presence_tone_db: v.presence_tone,
            depth_tone_db: v.depth_tone,
            topology_separation_amount: v.topology_separation_amount,
            power_pair_overdrive_db: v.power_pair_overdrive,
            difference_tone_amount: v.difference_tone,
            high_frequency_emphasis_amount: v.high_frequency_emphasis,
        }
    }
}

impl PanelControlValues {
    pub fn read(parameters: &AnvilParameters) -> Self {
        Self {
            preamp_gain_db: parameters.preamp_gain.value(),
            input_low_cut_hz: parameters.input_low_cut.value(),
            bass_tone_position: parameters.bass_tone.value(),
            midrange_tone_position: parameters.midrange_tone.value(),
            treble_tone_position: parameters.treble_tone.value(),
            presence_tone_db: parameters.presence_tone.value(),
            depth_tone_db: parameters.depth_tone.value(),
            topology_separation_amount: parameters.topology_separation_amount.value(),
            power_pair_overdrive_db: parameters.power_pair_overdrive.value(),
            difference_tone_amount: parameters.difference_tone.value(),
            high_frequency_emphasis_amount: parameters.high_frequency_emphasis.value(),
        }
    }
}

/// Non-automatable construction choices for this Anvil model.
///
/// This is ordinary Rust data rather than hidden host parameters. A future
/// Anvil model can persist a versioned design; other amplifiers can compose
/// the same DSP mechanisms without inheriting these choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnvilDesign {
    pub reference_input_level_db: f32,
    pub target_output_level_db: f32,
    pub power_pair_cold_bias_volts: f64,
    pub preamp_stage_count: usize,
    pub preamp_gain_offset_db: f32,
}

impl Default for AnvilDesign {
    fn default() -> Self {
        Self {
            reference_input_level_db: -12.0,
            target_output_level_db: -8.0,
            // The former PARALLEL AMPLIFIER default silently derived this as
            // 0.8 + 12 dB * 0.12. It is now a construction choice.
            power_pair_cold_bias_volts: 2.24,
            preamp_stage_count: 4,
            preamp_gain_offset_db: 6.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputSettings {
    pub highpass_cutoff_hz: f32,
    pub low_band_output_gain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmplifierSettings {
    pub preamp_stage_count: usize,
    pub preamp_drive_db: f32,
    pub power_pair_overdrive_db: f32,
    pub topology_separation_amount: f32,
    pub reference_input_level_db: f32,
    pub blocking_high_shelf_depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneSettings {
    pub bass_control_position: f32,
    pub midrange_control_position: f32,
    pub treble_control_position: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputSettings {
    pub presence_gain_db: f32,
    pub depth_gain_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineSettings {
    pub input: InputSettings,
    pub difference_tone_injection_amount: f32,
    pub amplifier: AmplifierSettings,
    pub tone: ToneSettings,
    pub output_conditioner: OutputSettings,
}

/// Resolve panel macros into objective section settings.
pub fn resolve_engine_settings(
    panel_controls: PanelControlValues,
    design: &AnvilDesign,
) -> EngineSettings {
    let input_low_cut_position =
        ((panel_controls.input_low_cut_hz / 40.0).ln() / (300.0f32 / 40.0).ln()).clamp(0.0, 1.0);
    EngineSettings {
        input: InputSettings {
            highpass_cutoff_hz: panel_controls.input_low_cut_hz,
            low_band_output_gain: 10f32
                .powf(-10.0 * input_low_cut_position * input_low_cut_position / 20.0),
        },
        difference_tone_injection_amount: panel_controls.difference_tone_amount,
        amplifier: AmplifierSettings {
            preamp_stage_count: design.preamp_stage_count,
            preamp_drive_db: design.preamp_gain_offset_db + panel_controls.preamp_gain_db * 0.5,
            power_pair_overdrive_db: panel_controls.power_pair_overdrive_db,
            topology_separation_amount: panel_controls.topology_separation_amount,
            reference_input_level_db: design.reference_input_level_db,
            blocking_high_shelf_depth: panel_controls.high_frequency_emphasis_amount,
        },
        tone: ToneSettings {
            bass_control_position: panel_controls.bass_tone_position,
            midrange_control_position: panel_controls.midrange_tone_position,
            treble_control_position: panel_controls.treble_tone_position,
        },
        output_conditioner: OutputSettings {
            presence_gain_db: panel_controls.presence_tone_db,
            depth_gain_db: panel_controls.depth_tone_db,
        },
    }
}

/// State-semantic application gate: first application fires; an equal value
/// does not. It keeps section comparison explicit without another framework.
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

    fn representative_panel_controls() -> PanelControlValues {
        PanelControlValues {
            preamp_gain_db: 18.0,
            input_low_cut_hz: 120.0,
            bass_tone_position: 6.25,
            midrange_tone_position: 4.583_333,
            treble_tone_position: 7.5,
            presence_tone_db: 2.5,
            depth_tone_db: 2.5,
            topology_separation_amount: 0.5,
            power_pair_overdrive_db: 12.0,
            difference_tone_amount: 0.35,
            high_frequency_emphasis_amount: 0.0,
        }
    }

    #[test]
    fn controls_map_to_named_sections() {
        let settings =
            resolve_engine_settings(representative_panel_controls(), &AnvilDesign::default());
        assert_eq!(settings.input.highpass_cutoff_hz, 120.0);
        assert_eq!(settings.amplifier.preamp_drive_db, 15.0);
        assert_eq!(settings.amplifier.power_pair_overdrive_db, 12.0);
        assert_eq!(settings.difference_tone_injection_amount, 0.35);
        assert_eq!(settings.tone.midrange_control_position, 4.583_333);
        assert_eq!(settings.output_conditioner.presence_gain_db, 2.5);
    }

    #[test]
    fn tight_is_the_only_declared_macro() {
        let mut low_setting = representative_panel_controls();
        low_setting.input_low_cut_hz = 40.0;
        let mut high_setting = representative_panel_controls();
        high_setting.input_low_cut_hz = 300.0;
        let design = AnvilDesign::default();
        let low_settings = resolve_engine_settings(low_setting, &design);
        let high_settings = resolve_engine_settings(high_setting, &design);
        assert_eq!(low_settings.input.low_band_output_gain, 1.0);
        assert!((20.0 * high_settings.input.low_band_output_gain.log10() + 10.0).abs() < 1e-5);
        assert_eq!(low_settings.amplifier, high_settings.amplifier);
        assert_eq!(low_settings.tone, high_settings.tone);
        assert_eq!(
            low_settings.output_conditioner,
            high_settings.output_conditioner
        );
    }

    #[test]
    fn applied_uses_state_not_write_events() {
        let mut last_applied_settings = LastAppliedSettings::new();
        assert!(last_applied_settings.has_changed(3));
        assert!(!last_applied_settings.has_changed(3));
        assert!(last_applied_settings.has_changed(4));
        last_applied_settings.reset();
        assert!(last_applied_settings.has_changed(4));
    }
}
