//! Output conditioning after the amplifier: fixed voicing,
//! presence/depth, transformer bandwidth, and DC rejection.

use stave_dsp::Biquad;
use stave_dsp::FilterSpec;

use crate::controls::OutputSettings;

pub struct OutputConditioner {
    fixed_low_mid_peak: Biquad,
    fixed_high_shelf: Biquad,
    presence_high_shelf: Biquad,
    depth_low_shelf: Biquad,
    first_high_frequency_lowpass: Biquad,
    second_high_frequency_lowpass: Biquad,
    direct_current_highpass: Biquad,
    sample_rate: f32,
}

impl OutputConditioner {
    pub fn new() -> Self {
        Self {
            fixed_low_mid_peak: Biquad::new(),
            fixed_high_shelf: Biquad::new(),
            presence_high_shelf: Biquad::new(),
            depth_low_shelf: Biquad::new(),
            first_high_frequency_lowpass: Biquad::new(),
            second_high_frequency_lowpass: Biquad::new(),
            direct_current_highpass: Biquad::new(),
            sample_rate: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        for filter in [
            &mut self.fixed_low_mid_peak,
            &mut self.fixed_high_shelf,
            &mut self.presence_high_shelf,
            &mut self.depth_low_shelf,
            &mut self.first_high_frequency_lowpass,
            &mut self.second_high_frequency_lowpass,
            &mut self.direct_current_highpass,
        ] {
            filter.reset();
        }
        self.first_high_frequency_lowpass.set(
            FilterSpec::Lowpass {
                hz: 10_400.0,
                q: 0.6,
            },
            self.sample_rate,
        );
        self.second_high_frequency_lowpass.set(
            FilterSpec::Lowpass {
                hz: 9_500.0,
                q: 0.55,
            },
            self.sample_rate,
        );
        self.fixed_high_shelf.set(
            FilterSpec::HighShelfRbj {
                hz: 1_400.0,
                db: 8.5,
            },
            self.sample_rate,
        );
        self.fixed_low_mid_peak.set(
            FilterSpec::Peak {
                hz: 330.0,
                db: 2.5,
                q: 0.9,
            },
            self.sample_rate,
        );
        self.direct_current_highpass
            .set(FilterSpec::Highpass { hz: 10.0, q: 0.71 }, self.sample_rate);
    }

    pub fn apply_settings(&mut self, settings: OutputSettings) {
        self.presence_high_shelf.set(
            FilterSpec::HighShelfRbj {
                hz: 4_500.0,
                db: settings.presence_gain_db,
            },
            self.sample_rate,
        );
        self.depth_low_shelf.set(
            FilterSpec::LowShelfRbj {
                hz: 90.0,
                db: settings.depth_gain_db,
            },
            self.sample_rate,
        );
    }

    #[inline]
    pub fn process_sample(&mut self, input_sample: f32) -> f32 {
        let low_mid_shaped = self.fixed_low_mid_peak.process(input_sample);
        let fixed_response = self.fixed_high_shelf.process(low_mid_shaped);
        let presence_shaped = self.presence_high_shelf.process(fixed_response);
        let depth_shaped = self.depth_low_shelf.process(presence_shaped);
        let first_band_limited = self.first_high_frequency_lowpass.process(depth_shaped);
        let second_band_limited = self
            .second_high_frequency_lowpass
            .process(first_band_limited);
        self.direct_current_highpass.process(second_band_limited)
    }
}
