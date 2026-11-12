//! Post-topology high-band peak control.
//!
//! Only the complementary high band is limited, so pick spikes are rounded
//! without asking the low-frequency parallel amplifier to share another nonlinearity.

use stave_dsp::LinkwitzRiley4;

const HIGH_BAND_LIMITER_CROSSOVER_HZ: f32 = 2_500.0;
const HIGH_BAND_DRIVE: f32 = 3.0;

pub struct HighBandLimiter {
    lowpass: LinkwitzRiley4,
    highpass: LinkwitzRiley4,
}

impl HighBandLimiter {
    pub fn new() -> Self {
        Self {
            lowpass: LinkwitzRiley4::new(),
            highpass: LinkwitzRiley4::new(),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.lowpass
            .set_lowpass(HIGH_BAND_LIMITER_CROSSOVER_HZ, sample_rate);
        self.highpass
            .set_highpass(HIGH_BAND_LIMITER_CROSSOVER_HZ, sample_rate);
        self.lowpass.reset();
        self.highpass.reset();
    }

    #[inline]
    pub fn process_sample(&mut self, input_sample: f32) -> f32 {
        let high_band_sample = self.highpass.process(input_sample);
        let low_band_sample = self.lowpass.process(input_sample);
        low_band_sample + (high_band_sample * HIGH_BAND_DRIVE).tanh() / HIGH_BAND_DRIVE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_output_root_mean_square(frequency_hz: f32) -> f64 {
        let mut limiter = HighBandLimiter::new();
        limiter.prepare(48_000.0);
        let mut energy = 0.0f64;
        for sample_index in 0..48_000 {
            let input =
                (core::f32::consts::TAU * frequency_hz * sample_index as f32 / 48_000.0).sin();
            let output = limiter.process_sample(input);
            if sample_index >= 24_000 {
                energy += f64::from(output) * f64::from(output);
            }
        }
        (energy / 24_000.0).sqrt()
    }

    #[test]
    fn limiting_is_confined_to_the_high_band() {
        let low_band_level = measure_output_root_mean_square(500.0);
        let high_band_level = measure_output_root_mean_square(5_000.0);
        assert!(
            low_band_level > 0.65,
            "low band was limited: {low_band_level}"
        );
        assert!(
            high_band_level < low_band_level * 0.8,
            "high band escaped: low={low_band_level}, high={high_band_level}"
        );
    }
}
