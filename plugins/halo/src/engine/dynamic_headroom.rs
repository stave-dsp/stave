//! Halo's impossible energy-control plane.
//!
//! No band is rendered or recombined here. The filters only measure where the
//! signal's energy lives, and those measurements select among independent power
//! reservoirs. Consequently this system can change dynamic interaction without
//! imposing a multiband crossover on the audio path.

use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, EnvelopeFollower, EnvelopeTimes};

use super::reservoir::Reservoir;

use crate::controls::DynamicSettings;

/// The feedback-heavy clean pair moves only a small fraction above its own
/// idle current before audible clipping. Halo declares that excursion as full
/// reservoir demand instead of inheriting a high-gain topology's draw scale.
const FULL_RESERVOIR_DRAW_EXCURSION_ABOVE_IDLE: f32 = 0.000_25;

pub struct FrequencySelectivePowerSupply {
    low_frequency_detector: Biquad,
    high_frequency_detector: Biquad,
    low_frequency_envelope: EnvelopeFollower,
    middle_frequency_envelope: EnvelopeFollower,
    high_frequency_envelope: EnvelopeFollower,
    low_frequency_reservoir: Reservoir,
    middle_frequency_reservoir: Reservoir,
    high_frequency_reservoir: Reservoir,
    reserve_amount: f32,
    low_frequency_voltage: f32,
    middle_frequency_voltage: f32,
    high_frequency_voltage: f32,
}

impl FrequencySelectivePowerSupply {
    pub fn new() -> Self {
        Self {
            low_frequency_detector: Biquad::new(),
            high_frequency_detector: Biquad::new(),
            low_frequency_envelope: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 3.0,
                release_ms: 100.0,
                slow_release_ms: None,
            }),
            middle_frequency_envelope: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 2.0,
                release_ms: 75.0,
                slow_release_ms: None,
            }),
            high_frequency_envelope: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 1.0,
                release_ms: 55.0,
                slow_release_ms: None,
            }),
            low_frequency_reservoir: Reservoir::new(),
            middle_frequency_reservoir: Reservoir::new(),
            high_frequency_reservoir: Reservoir::new(),
            reserve_amount: 0.0,
            low_frequency_voltage: 1.0,
            middle_frequency_voltage: 1.0,
            high_frequency_voltage: 1.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.low_frequency_detector.reset();
        self.low_frequency_detector
            .set(FilterSpec::Lowpass { hz: 220.0, q: 0.71 }, sample_rate);
        self.high_frequency_detector.reset();
        self.high_frequency_detector.set(
            FilterSpec::Highpass {
                hz: 1_600.0,
                q: 0.71,
            },
            sample_rate,
        );
        self.low_frequency_envelope.prepare(sample_rate);
        self.middle_frequency_envelope.prepare(sample_rate);
        self.high_frequency_envelope.prepare(sample_rate);
        self.low_frequency_reservoir.prepare(sample_rate);
        self.middle_frequency_reservoir.prepare(sample_rate);
        self.high_frequency_reservoir.prepare(sample_rate);
    }

    pub fn apply_settings(&mut self, settings: DynamicSettings) {
        self.reserve_amount = settings.reserve_amount;
        for reservoir in [
            &mut self.low_frequency_reservoir,
            &mut self.middle_frequency_reservoir,
            &mut self.high_frequency_reservoir,
        ] {
            reservoir.set(
                settings.supply_droop_amount,
                settings.supply_droop_time_ms,
                settings.supply_recovery_time_ms,
            );
        }
    }

    /// Advance all reservoirs and return the voltage selected by the current
    /// spectral energy distribution.
    #[inline]
    pub fn process_sample(&mut self, signal: f32, normalized_plate_draw: f32) -> f32 {
        let low_frequency_signal = self.low_frequency_detector.process(signal);
        let high_frequency_signal = self.high_frequency_detector.process(signal);
        let middle_frequency_signal = signal - low_frequency_signal - high_frequency_signal;

        let low_energy = self.low_frequency_envelope.tick(low_frequency_signal.abs());
        let middle_energy = self
            .middle_frequency_envelope
            .tick(middle_frequency_signal.abs());
        let high_energy = self
            .high_frequency_envelope
            .tick(high_frequency_signal.abs());
        let energy_sum = low_energy + middle_energy + high_energy + 1e-9;
        let low_weight = low_energy / energy_sum;
        let middle_weight = middle_energy / energy_sum;
        let high_weight = high_energy / energy_sum;

        let common_draw = ((normalized_plate_draw - 1.0).max(0.0)
            / FULL_RESERVOIR_DRAW_EXCURSION_ABOVE_IDLE)
            .clamp(0.0, 1.5);
        let shared_amount = 1.0 - self.reserve_amount;
        let low_draw = common_draw * (shared_amount + self.reserve_amount * low_weight * 3.0);
        let middle_draw = common_draw * (shared_amount + self.reserve_amount * middle_weight * 3.0);
        let high_draw = common_draw * (shared_amount + self.reserve_amount * high_weight * 3.0);

        self.low_frequency_voltage = self.low_frequency_reservoir.tick(low_draw);
        self.middle_frequency_voltage = self.middle_frequency_reservoir.tick(middle_draw);
        self.high_frequency_voltage = self.high_frequency_reservoir.tick(high_draw);

        let energy_weighted_voltage = low_weight * self.low_frequency_voltage
            + middle_weight * self.middle_frequency_voltage
            + high_weight * self.high_frequency_voltage;
        // The best-reserved rail can lend its voltage to the shared audio
        // path. `max` is continuous and the rails themselves move slowly, so
        // this introduces no selector click. This is Halo's impossible part:
        // bass demand cannot pull the whole amplifier below a simultaneously
        // available chime reservoir.
        let protected_voltage = self
            .low_frequency_voltage
            .max(self.middle_frequency_voltage)
            .max(self.high_frequency_voltage);
        energy_weighted_voltage
            + self.reserve_amount * (protected_voltage - energy_weighted_voltage)
    }

    pub fn voltages(&self) -> (f32, f32, f32) {
        (
            self.low_frequency_voltage,
            self.middle_frequency_voltage,
            self.high_frequency_voltage,
        )
    }
}

pub struct TransientHeadroomController {
    fast_envelope: EnvelopeFollower,
    body_envelope: EnvelopeFollower,
}

impl TransientHeadroomController {
    pub fn new() -> Self {
        Self {
            fast_envelope: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 0.15,
                release_ms: 18.0,
                slow_release_ms: None,
            }),
            body_envelope: EnvelopeFollower::new(EnvelopeTimes {
                attack_ms: 32.0,
                release_ms: 180.0,
                slow_release_ms: None,
            }),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.fast_envelope.prepare(sample_rate);
        self.body_envelope.prepare(sample_rate);
    }

    #[inline]
    pub fn process_sample(&mut self, signal: f32) -> f32 {
        let magnitude = signal.abs();
        let fast = self.fast_envelope.tick(magnitude);
        let body = self.body_envelope.tick(magnitude);
        ((fast - body).max(0.0) / fast.max(1e-6)).clamp(0.0, 1.0)
    }
}

pub struct AdaptiveHarmonicController {
    even_harmonic_direct_current_blocker: Biquad,
    bloom_amount: f32,
    headroom_amount: f32,
}

impl AdaptiveHarmonicController {
    pub fn new() -> Self {
        Self {
            even_harmonic_direct_current_blocker: Biquad::new(),
            bloom_amount: 0.0,
            headroom_amount: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.even_harmonic_direct_current_blocker.reset();
        self.even_harmonic_direct_current_blocker
            .set(FilterSpec::Highpass { hz: 8.0, q: 0.71 }, sample_rate);
    }

    pub fn apply_settings(&mut self, settings: DynamicSettings) {
        self.bloom_amount = settings.bloom_amount;
        self.headroom_amount = settings.headroom_amount;
    }

    #[inline]
    pub fn process_sample(
        &mut self,
        power_pair_input: f32,
        physical_output: f32,
        small_signal_gain: f32,
        transient_amount: f32,
    ) -> f32 {
        let linear_reference = power_pair_input * small_signal_gain;
        let nonlinear_residual = physical_output - linear_reference;
        let sustain_amount = 1.0 - transient_amount;
        let physical_residual_amount = (0.24
            + 0.76 * self.bloom_amount * (1.0 - self.headroom_amount * transient_amount))
            .clamp(0.15, 1.0);
        let even_harmonic = self
            .even_harmonic_direct_current_blocker
            .process(linear_reference * linear_reference)
            * (0.8 * self.bloom_amount * sustain_amount);
        linear_reference + nonlinear_residual * physical_residual_amount + even_harmonic
    }
}
