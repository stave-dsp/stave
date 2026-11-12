//! Halo's two-stage clean preamplifier.
//!
//! The two stages are intentionally exposed as separate operations because the
//! product's passive tone circuit sits between them. This is a concrete
//! topology, not a configurable cascade.

use stave_dsp::db_to_amp;
use stave_dsp::FilterSpec;
use stave_dsp::{Bias, Biquad, Coupler, Koren, LevelPolicy, Stage, Triode};

use crate::controls::PreampSettings;
use crate::params::DEFAULT_PREAMP_GAIN_DECIBELS;

const FIRST_STAGE_OUTPUT_TRIM_DB: f32 = -5.0;
const RECOVERY_STAGE_OUTPUT_TRIM_DB: f32 = -7.0;
const MAXIMUM_BRIGHT_CAPACITOR_FEED_FORWARD: f32 = 0.75;

pub struct CleanPreamp {
    bright_capacitor_highpass: Biquad,
    input_stage: Triode,
    input_stage_coupler: Coupler,
    recovery_stage: Triode,
    /// Halo's bloom: each stage's bias moves by this share of its own
    /// cathode's drift (1 = the circuit alone). An effect of the voicing,
    /// applied on top of the physical self-bias.
    input_stage_bloom: f64,
    recovery_stage_bloom: f64,
    output_coupler: Coupler,
    input_stage_output_trim: f32,
    recovery_stage_output_trim: f32,
}

impl CleanPreamp {
    pub fn new() -> Self {
        let mut input_stage = Triode::new();
        input_stage.set_tube(Koren::T12AT7);
        input_stage.set_level_policy(UNITY_AT_CLEAN);

        let mut recovery_stage = Triode::new();
        recovery_stage.set_tube(Koren::T6SN7);
        recovery_stage.set_level_policy(UNITY_AT_CLEAN);

        Self {
            bright_capacitor_highpass: Biquad::new(),
            input_stage,
            input_stage_coupler: Coupler::new(7.0),
            recovery_stage,
            input_stage_bloom: 1.0,
            recovery_stage_bloom: 1.0,
            output_coupler: Coupler::new(5.0),
            input_stage_output_trim: db_to_amp(FIRST_STAGE_OUTPUT_TRIM_DB),
            recovery_stage_output_trim: db_to_amp(RECOVERY_STAGE_OUTPUT_TRIM_DB),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.bright_capacitor_highpass.reset();
        self.bright_capacitor_highpass.set(
            FilterSpec::Highpass {
                hz: 2_800.0,
                q: 0.5,
            },
            sample_rate,
        );
        self.input_stage.prepare(sample_rate);
        self.input_stage_coupler.prepare(sample_rate);
        self.recovery_stage.prepare(sample_rate);
        self.output_coupler.prepare(sample_rate);
    }

    pub fn apply_settings(&mut self, settings: PreampSettings) {
        self.input_stage
            .set_grid_drive(drive_volts(settings.first_stage_drive_db));
        self_bias(&mut self.input_stage, 22e-3);
        self.recovery_stage
            .set_grid_drive(drive_volts(settings.recovery_stage_drive_db));
        self_bias(&mut self.recovery_stage, 85e-3);
        self.input_stage_bloom = f64::from(settings.cathode_bloom_depth) * 0.35;
        self.recovery_stage_bloom = f64::from(settings.cathode_bloom_depth);
    }

    #[inline]
    pub fn process_input_stage(&mut self, input: f32, preamp_gain_decibels: f32) -> f32 {
        // This is the frequency-dependent volume network used by real clean
        // amplifiers, not a corrective EQ after the amplifier. A capacitor
        // bypasses progressively less of the gain control as that control is
        // opened, so low and moderate Gain settings retain pick sparkle while
        // high settings become naturally fuller.
        let gain_position = (preamp_gain_decibels / 18.0).clamp(0.0, 1.0);
        let bright_feed_forward = MAXIMUM_BRIGHT_CAPACITOR_FEED_FORWARD * (1.0 - gain_position);
        let bright_signal = self.bright_capacitor_highpass.process(input);
        let frequency_shaped_input = input + bright_signal * bright_feed_forward;
        let input_level_gain =
            db_to_amp(preamp_gain_decibels - DEFAULT_PREAMP_GAIN_DECIBELS as f32);
        let amplified = self
            .input_stage
            .process_sample(frequency_shaped_input * input_level_gain)
            * self.input_stage_output_trim;
        bloom(&mut self.input_stage, self.input_stage_bloom);
        self.input_stage_coupler.tick(amplified)
    }

    #[inline]
    pub fn process_recovery_stage(&mut self, input: f32) -> f32 {
        let amplified = self.recovery_stage.process_sample(input) * self.recovery_stage_output_trim;
        bloom(&mut self.recovery_stage, self.recovery_stage_bloom);
        self.output_coupler.tick(amplified)
    }

    /// Fixed circuit gain, deliberately excluding the user-controlled input
    /// level. Output calibration must never cancel the Gain knob.
    pub fn fixed_stage_small_signal_gain_db(&self) -> f32 {
        self.input_stage.small_signal_gain_db()
            + FIRST_STAGE_OUTPUT_TRIM_DB
            + self.recovery_stage.small_signal_gain_db()
            + RECOVERY_STAGE_OUTPUT_TRIM_DB
    }
}

/// Cathode self-bias with a bypass cap sized for an RC of `seconds`.
fn self_bias(stage: &mut Triode, seconds: f64) {
    let bypass_farads = seconds / stage.cathode_ohms();
    stage.set_bias(Bias::Cathode { bypass_farads });
}

/// Scale the stage's bias drift to `share` of its cathode's own.
#[inline]
fn bloom(stage: &mut Triode, share: f64) {
    stage.set_grid_bias_direct((share - 1.0) * stage.cathode_shift());
}

/// The drive knob's clean end: grid volts per unit at 0 dB.
const DRIVE_CLEAN_VOLTS: f64 = 0.4;

/// Unity small-signal gain at the drive knob's clean end; gain follows
/// the knob from there.
const UNITY_AT_CLEAN: LevelPolicy = LevelPolicy::Physical {
    reference_drive: DRIVE_CLEAN_VOLTS,
};

/// Halo's drive knob law: 0–20 dB onto grid volts per unit of input,
/// an anchored exponential — clean at 0 (0.4 V), saturation audible by
/// ~20 % of the knob, extreme at the top (110 V).
fn drive_volts(drive_db: f32) -> f64 {
    const MAX_DB: f64 = 20.0;
    const SPAN_VOLTS: f64 = 110.0;
    const CURVE: f64 = 1.5;
    let position = (f64::from(drive_db) / MAX_DB).clamp(0.0, 1.0);
    DRIVE_CLEAN_VOLTS
        + SPAN_VOLTS * (10f64.powf(CURVE * position) - 1.0) / (10f64.powf(CURVE) - 1.0)
}
