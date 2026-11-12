//! Anvil's product-specific passive tone circuit and panel mapping.

mod anvil_tone_stack;

use crate::controls::ToneSettings;

use anvil_tone_stack::{AnvilToneControlPositions, AnvilToneStack};

const LEVEL_COMPENSATION_REFERENCE_HZ: f64 = 700.0;
const MINIMUM_REPORTED_MAGNITUDE: f64 = 0.031_622_776_601_683_79;

pub(super) struct ToneSection {
    tone_stack: AnvilToneStack,
}

impl ToneSection {
    pub(super) fn new() -> Self {
        Self {
            tone_stack: AnvilToneStack::default(),
        }
    }

    pub(super) fn prepare(&mut self, sample_rate: f32) {
        self.tone_stack.prepare(sample_rate);
    }

    pub(super) fn apply_settings(&mut self, settings: ToneSettings) {
        self.tone_stack.set_controls(AnvilToneControlPositions::new(
            f64::from(settings.treble_control_position / 10.0),
            f64::from(settings.bass_control_position / 10.0),
            f64::from(settings.midrange_control_position / 10.0),
        ));
    }

    pub(super) fn level_compensation_gain(&self) -> f32 {
        (1.0 / self
            .tone_stack
            .magnitude_at(LEVEL_COMPENSATION_REFERENCE_HZ)
            .max(MINIMUM_REPORTED_MAGNITUDE)) as f32
    }

    #[inline]
    pub(super) fn process_sample(&mut self, input_sample: f32) -> f32 {
        self.tone_stack.process(input_sample)
    }
}
