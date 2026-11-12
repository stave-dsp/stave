use stave_dsp::Biquad;
use stave_dsp::FilterSpec;

use crate::controls::InputSettings;

pub struct InputConditioner {
    subsonic_highpass: Biquad,
    radio_frequency_lowpass: Biquad,
    sample_rate: f32,
}

impl InputConditioner {
    pub fn new() -> Self {
        Self {
            subsonic_highpass: Biquad::new(),
            radio_frequency_lowpass: Biquad::new(),
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.subsonic_highpass.reset();
        self.radio_frequency_lowpass.reset();
    }

    pub fn apply_settings(&mut self, settings: InputSettings) {
        self.subsonic_highpass.set(
            FilterSpec::Highpass {
                hz: settings.highpass_cutoff_hz,
                q: core::f32::consts::FRAC_1_SQRT_2,
            },
            self.sample_rate,
        );
        self.radio_frequency_lowpass.set(
            FilterSpec::Lowpass {
                hz: settings.lowpass_cutoff_hz,
                q: core::f32::consts::FRAC_1_SQRT_2,
            },
            self.sample_rate,
        );
    }

    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let highpassed = self.subsonic_highpass.process(input);
        self.radio_frequency_lowpass.process(highpassed)
    }
}
