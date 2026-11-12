use stave_dsp::Biquad;
use stave_dsp::FilterSpec;

pub struct OutputConditioner {
    direct_current_highpass: Biquad,
    ultrasonic_lowpass: Biquad,
}

impl OutputConditioner {
    pub fn new() -> Self {
        Self {
            direct_current_highpass: Biquad::new(),
            ultrasonic_lowpass: Biquad::new(),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.direct_current_highpass.reset();
        self.direct_current_highpass
            .set(FilterSpec::Highpass { hz: 7.0, q: 0.71 }, sample_rate);
        self.ultrasonic_lowpass.reset();
        self.ultrasonic_lowpass.set(
            FilterSpec::Lowpass {
                hz: 22_000.0,
                q: 0.71,
            },
            sample_rate,
        );
    }

    #[inline]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let direct_current_rejected = self.direct_current_highpass.process(input);
        self.ultrasonic_lowpass.process(direct_current_rejected)
    }
}
