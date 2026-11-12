//! Signal-derived sub-octave generation without pitch tracking.
//!
//! Even-order products from the low/low-mid input create difference tones;
//! a power chord therefore generates its octave below from the chord's own
//! frequencies. This is an idealized Anvil mechanism, not an emulated pedal.

use stave_dsp::Biquad;
use stave_dsp::FilterSpec;

const GENERATION_BAND_HZ: f32 = 650.0;
const OUTPUT_BAND_HZ: f32 = 175.0;
const GENERATOR_DRIVE: f32 = 16.0;
const DIFFERENCE_TONE_INJECTION_GAIN: f32 = 1.5;

pub struct DifferenceToneGenerator {
    generation_band: Biquad,
    output_lowpass_1: Biquad,
    output_lowpass_2: Biquad,
    injection_amount: f32,
}

impl DifferenceToneGenerator {
    pub fn new() -> Self {
        Self {
            generation_band: Biquad::new(),
            output_lowpass_1: Biquad::new(),
            output_lowpass_2: Biquad::new(),
            injection_amount: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.generation_band.set(
            FilterSpec::Lowpass {
                hz: GENERATION_BAND_HZ,
                q: 0.6,
            },
            sample_rate,
        );
        self.output_lowpass_1.set(
            FilterSpec::Lowpass {
                hz: OUTPUT_BAND_HZ,
                q: 0.541,
            },
            sample_rate,
        );
        self.output_lowpass_2.set(
            FilterSpec::Lowpass {
                hz: OUTPUT_BAND_HZ,
                q: 1.307,
            },
            sample_rate,
        );
        self.generation_band.reset();
        self.output_lowpass_1.reset();
        self.output_lowpass_2.reset();
    }

    pub fn set_injection_amount(&mut self, injection_amount: f32) {
        self.injection_amount = injection_amount.clamp(0.0, 1.0);
    }

    #[inline]
    pub fn process_sample(&mut self, input_sample: f32) -> f32 {
        if self.injection_amount <= 0.001 {
            return input_sample;
        }
        let generation_band_sample = self.generation_band.process(input_sample) * GENERATOR_DRIVE;
        let even_order_product = (generation_band_sample * generation_band_sample.abs()).tanh();
        let difference_tone_sample = self
            .output_lowpass_2
            .process(self.output_lowpass_1.process(even_order_product));
        input_sample
            + self.injection_amount * DIFFERENCE_TONE_INJECTION_GAIN * difference_tone_sample
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_amount_is_bit_transparent() {
        let mut generator = DifferenceToneGenerator::new();
        generator.prepare(48_000.0);
        for bits in [0u32, 1, 0x3e80_0000, 0xbe80_0000] {
            let input = f32::from_bits(bits);
            assert_eq!(generator.process_sample(input).to_bits(), bits);
        }
    }

    #[test]
    fn two_notes_generate_their_difference_tone() {
        let sample_rate = 48_000.0;
        let mut generator = DifferenceToneGenerator::new();
        generator.prepare(sample_rate);
        generator.set_injection_amount(1.0);
        let mut projection = 0.0f64;
        let sample_count = 48_000;
        for sample_index in 0..sample_count {
            let phase = core::f32::consts::TAU * sample_index as f32 / sample_rate;
            let input = 0.2 * (300.0 * phase).sin() + 0.2 * (400.0 * phase).sin();
            let output = generator.process_sample(input);
            if sample_index >= sample_count / 2 {
                projection += f64::from(output)
                    * (core::f64::consts::TAU * 100.0 * sample_index as f64 / 48_000.0).sin();
            }
        }
        let magnitude = projection.abs() * 4.0 / sample_count as f64;
        assert!(magnitude > 0.01, "difference tone too small: {magnitude}");
    }
}
