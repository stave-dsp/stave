//! Halo's product-owned clean tone circuit.

use stave_dsp::{PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents};

use crate::controls::ToneSettings;

const LEVEL_REFERENCE_FREQUENCY_HZ: f64 = 700.0;
const MINIMUM_COMPENSATED_MAGNITUDE: f64 = 0.08;
const MAXIMUM_LEVEL_COMPENSATION_GAIN: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HaloToneStackDesign {
    pub components: PassiveToneStackComponents,
}

impl HaloToneStackDesign {
    /// A wide-band American clean network with boutique control authority.
    ///
    /// The 100 nF / 47 nF bass and midrange capacitors and higher slope
    /// resistance create the broad, low-loss high-frequency shoulder and
    /// characteristic clean scoop. The 25 kΩ midrange control preserves more
    /// range than a fixed or 10 kΩ vintage implementation.
    pub const fn wide_clean() -> Self {
        Self {
            components: PassiveToneStackComponents {
                treble_capacitance_farads: 270e-12,
                bass_capacitance_farads: 100e-9,
                midrange_capacitance_farads: 47e-9,
                treble_potentiometer_ohms: 250e3,
                bass_potentiometer_ohms: 250e3,
                midrange_potentiometer_ohms: 25e3,
                slope_resistance_ohms: 82e3,
            },
        }
    }
}

pub struct ToneSection {
    tone_stack: PassiveToneStack,
    level_compensation_gain: f32,
}

impl ToneSection {
    pub fn new() -> Self {
        Self {
            tone_stack: PassiveToneStack::new(HaloToneStackDesign::wide_clean().components),
            level_compensation_gain: 1.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.tone_stack.prepare(sample_rate);
        self.level_compensation_gain = (1.0
            / self
                .tone_stack
                .magnitude_at(LEVEL_REFERENCE_FREQUENCY_HZ)
                .max(MINIMUM_COMPENSATED_MAGNITUDE)) as f32;
        self.level_compensation_gain = self
            .level_compensation_gain
            .min(MAXIMUM_LEVEL_COMPENSATION_GAIN);
    }

    pub fn apply_settings(&mut self, settings: ToneSettings) {
        self.tone_stack
            .set_controls(PassiveToneControlPositions::new(
                f64::from(settings.treble_control_position / 10.0),
                f64::from(settings.bass_control_position / 10.0),
                f64::from(settings.midrange_control_position / 10.0),
            ));
    }

    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.tone_stack.process(input) * self.level_compensation_gain
    }

    #[cfg(test)]
    fn magnitude_at(&self, frequency_hz: f64) -> f64 {
        self.tone_stack.magnitude_at(frequency_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decibel_difference(high: f64, low: f64) -> f64 {
        20.0 * (high / low).log10()
    }

    #[test]
    fn each_control_has_material_authority() {
        let mut tone = ToneSection::new();
        tone.prepare(48_000.0);

        tone.apply_settings(ToneSettings {
            bass_control_position: 0.0,
            midrange_control_position: 5.0,
            treble_control_position: 5.0,
        });
        let bass_down = tone.magnitude_at(100.0);
        tone.apply_settings(ToneSettings {
            bass_control_position: 10.0,
            midrange_control_position: 5.0,
            treble_control_position: 5.0,
        });
        let bass_authority = decibel_difference(tone.magnitude_at(100.0), bass_down);
        assert!(
            bass_authority > 4.0,
            "bass authority was {bass_authority:.2} dB"
        );

        tone.apply_settings(ToneSettings {
            bass_control_position: 5.0,
            midrange_control_position: 0.0,
            treble_control_position: 5.0,
        });
        let midrange_down = tone.magnitude_at(700.0);
        tone.apply_settings(ToneSettings {
            bass_control_position: 5.0,
            midrange_control_position: 10.0,
            treble_control_position: 5.0,
        });
        let midrange_authority = decibel_difference(tone.magnitude_at(700.0), midrange_down);
        assert!(
            midrange_authority > 6.0,
            "midrange authority was {midrange_authority:.2} dB"
        );

        tone.apply_settings(ToneSettings {
            bass_control_position: 5.0,
            midrange_control_position: 5.0,
            treble_control_position: 0.0,
        });
        let treble_down = tone.magnitude_at(5_000.0);
        tone.apply_settings(ToneSettings {
            bass_control_position: 5.0,
            midrange_control_position: 5.0,
            treble_control_position: 10.0,
        });
        let treble_authority = decibel_difference(tone.magnitude_at(5_000.0), treble_down);
        assert!(
            treble_authority > 6.0,
            "treble authority was {treble_authority:.2} dB"
        );
    }
}
