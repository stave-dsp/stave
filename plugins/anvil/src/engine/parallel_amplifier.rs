//! Blend between two complete nonlinear amplifier topologies.
//!
//! The full-band branch behaves like a conventional amplifier: the entire
//! signal shares each nonlinear stage. [`SplitBandAmplifier`] separates the
//! signal before amplification, so low-frequency energy cannot consume the
//! high-frequency branches' nonlinear headroom. This facade aligns and blends
//! those topology outputs.

mod branch;
mod rc_coupled_triode_cascade;
mod split_band_amplifier;

use stave_dsp::Biquad;
use stave_dsp::FilterSpec;

use branch::{AmplifierBranch, BranchDesign};
pub use rc_coupled_triode_cascade::RcCoupledTriodeCascade;
use split_band_amplifier::SplitBandAmplifier;

use crate::controls::{AmplifierSettings, InputSettings};

const FULL_BAND_PHASE_ALIGNMENT_HZ: f32 = 160.0;
const FULL_BAND_PHASE_ALIGNMENT_QUALITY: f32 = 0.7;

#[derive(Default, Clone, Copy)]
pub struct AmplifierMeasurements {
    pub full_band_output_peak: f32,
    pub low_band_output_peak: f32,
    pub high_band_output_peak: f32,
    pub full_band_power_pair_input_peak: f32,
    pub full_band_power_pair_draw_peak: f32,
    pub full_band_supply_voltage_minimum: f32,
    pub full_band_supply_voltage_maximum: f32,
    pub full_band_power_pair_draw_sum: f32,
    pub full_band_power_pair_draw_sample_count: f32,
}

pub struct ParallelAmplifier {
    full_band_branch: AmplifierBranch,
    split_band_amplifier: SplitBandAmplifier,
    full_band_phase_alignment: Biquad,
    topology_separation_amount: f32,
    measurements: AmplifierMeasurements,
}

impl ParallelAmplifier {
    pub fn new() -> Self {
        Self {
            full_band_branch: AmplifierBranch::new(BranchDesign::full_band()),
            split_band_amplifier: SplitBandAmplifier::new(),
            full_band_phase_alignment: Biquad::new(),
            topology_separation_amount: 0.7,
            measurements: AmplifierMeasurements::default(),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32, power_pair_cold_bias_volts: f64) {
        self.full_band_phase_alignment.reset();
        self.full_band_phase_alignment.set(
            FilterSpec::Allpass {
                hz: FULL_BAND_PHASE_ALIGNMENT_HZ,
                q: FULL_BAND_PHASE_ALIGNMENT_QUALITY,
            },
            sample_rate,
        );
        self.full_band_branch
            .prepare(sample_rate, power_pair_cold_bias_volts);
        self.split_band_amplifier
            .prepare(sample_rate, power_pair_cold_bias_volts);
        self.measurements = AmplifierMeasurements::default();
    }

    pub fn apply_settings(
        &mut self,
        amplifier_settings: AmplifierSettings,
        input_settings: InputSettings,
    ) {
        self.topology_separation_amount = amplifier_settings
            .topology_separation_amount
            .clamp(0.0, 1.0);

        let branch_drive_offset_db = (amplifier_settings.preamp_drive_db - 6.0).max(0.0) * 0.6;
        let branch_drive_db = amplifier_settings.preamp_drive_db + branch_drive_offset_db;

        self.full_band_branch
            .set_blocking_high_shelf_depth(amplifier_settings.blocking_high_shelf_depth);
        self.full_band_branch.apply_live_settings(
            amplifier_settings.preamp_stage_count,
            branch_drive_db,
            amplifier_settings.power_pair_overdrive_db,
            amplifier_settings.reference_input_level_db,
        );
        self.split_band_amplifier.apply_settings(
            amplifier_settings,
            input_settings,
            branch_drive_db,
        );
    }

    pub fn expected_output_level_db(&self, input_level_db: f32) -> f32 {
        self.full_band_branch
            .expected_output_level_db(input_level_db)
    }

    pub fn take_measurements(&mut self) -> AmplifierMeasurements {
        let split_band_measurements = self.split_band_amplifier.take_measurements();
        self.measurements.low_band_output_peak = split_band_measurements.low_band_output_peak;
        self.measurements.high_band_output_peak = split_band_measurements.high_band_output_peak;

        let measurements = self.measurements;
        self.measurements = AmplifierMeasurements::default();
        measurements
    }

    #[inline]
    pub fn process_sample(&mut self, input_sample: f32) -> f32 {
        let split_band_output = self.split_band_amplifier.process_sample(input_sample);
        if self.topology_separation_amount >= 0.999 {
            return split_band_output;
        }

        let full_band_output = self.full_band_branch.process_sample(input_sample);
        self.measurements.full_band_output_peak = self
            .measurements
            .full_band_output_peak
            .max(full_band_output.abs());
        self.measurements.full_band_power_pair_input_peak = self
            .measurements
            .full_band_power_pair_input_peak
            .max(self.full_band_branch.previous_power_pair_input().abs());

        let power_pair_draw = self.full_band_branch.power_pair_draw();
        self.measurements.full_band_power_pair_draw_peak = self
            .measurements
            .full_band_power_pair_draw_peak
            .max(power_pair_draw);
        self.measurements.full_band_power_pair_draw_sum += power_pair_draw;
        self.measurements.full_band_power_pair_draw_sample_count += 1.0;

        let power_supply_voltage = self.full_band_branch.power_supply_voltage();
        self.measurements.full_band_supply_voltage_minimum =
            if self.measurements.full_band_supply_voltage_minimum == 0.0 {
                power_supply_voltage
            } else {
                self.measurements
                    .full_band_supply_voltage_minimum
                    .min(power_supply_voltage)
            };
        self.measurements.full_band_supply_voltage_maximum = self
            .measurements
            .full_band_supply_voltage_maximum
            .max(power_supply_voltage);

        let phase_aligned_full_band_output =
            self.full_band_phase_alignment.process(full_band_output);
        phase_aligned_full_band_output
            + (split_band_output - phase_aligned_full_band_output) * self.topology_separation_amount
    }
}
