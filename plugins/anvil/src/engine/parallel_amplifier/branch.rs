//! One complete nonlinear tube-amplifier signal path.
//!
//! A branch owns its preamp cascade, power pair, local power supply, and
//! branch-specific voicing. It knows nothing about frequency-band routing or
//! topology blending.

use stave_dsp::db_to_amp;
use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, Koren, PowerSupply, PowerSupplyConfig, PushPull};

use super::rc_coupled_triode_cascade::UNITY_AT_CLEAN;
use super::RcCoupledTriodeCascade;

/// Anvil's B+: a valve rectifier (a junction plus ~250 Ω, GZ34-class)
/// into a CLC filter.
const SUPPLY: PowerSupplyConfig = PowerSupplyConfig {
    rectifier_ohms: 250.0,
    ..PowerSupplyConfig::SILICON_CHOKE
};
/// The stand-in pair's draw (× its idle) mapped onto a 2×EL34-class
/// section's current: 60 mA at idle, ~260 mA flat out.
const IDLE_LOAD_AMPS: f64 = 0.060;
const LOAD_AMPS_PER_DRAW: f64 = 0.004;
/// Seconds of idle running at prepare: past the switch-on surge.
const SUPPLY_SETTLE_SECONDS: f64 = 2.0;
const PREAMP_SUPPLY_DROOP_AMOUNT: f32 = 0.35;

const NEGATIVE_FEEDBACK_DB: f32 = 0.0;

const DEEP_BLOCKING_DEPTH: f32 = 0.9;
const DEEP_BLOCKING_RECOVERY_MS: f32 = 220.0;
const INTERSTAGE_DIVIDER_DB: f32 = -40.0;
const SOFT_KNEE_WIDTH_DB: f32 = 9.0;

#[derive(Clone, Copy)]
struct HighFrequencyEmphasisDesign {
    corner_hz: f32,
    input_gain_db: f32,
    output_gain_db: f32,
}

/// Activation-time construction choices for one nonlinear branch.
#[derive(Clone, Copy)]
pub(super) struct BranchDesign {
    power_pair_imbalance: f64,
    blocking_depth: f32,
    blocking_recovery_ms: f32,
    high_frequency_emphasis: Option<HighFrequencyEmphasisDesign>,
}

impl BranchDesign {
    const HIGH_FREQUENCY_EMPHASIS: HighFrequencyEmphasisDesign = HighFrequencyEmphasisDesign {
        corner_hz: 480.0,
        input_gain_db: 8.0,
        output_gain_db: -3.0,
    };

    pub(super) const fn full_band() -> Self {
        Self {
            power_pair_imbalance: 0.06,
            blocking_depth: DEEP_BLOCKING_DEPTH,
            blocking_recovery_ms: DEEP_BLOCKING_RECOVERY_MS,
            high_frequency_emphasis: Some(Self::HIGH_FREQUENCY_EMPHASIS),
        }
    }

    pub(super) const fn low_band() -> Self {
        Self {
            power_pair_imbalance: 0.18,
            blocking_depth: 0.25,
            blocking_recovery_ms: DEEP_BLOCKING_RECOVERY_MS,
            high_frequency_emphasis: None,
        }
    }

    pub(super) const fn low_mid_band() -> Self {
        Self {
            power_pair_imbalance: 0.18,
            blocking_depth: DEEP_BLOCKING_DEPTH,
            blocking_recovery_ms: DEEP_BLOCKING_RECOVERY_MS,
            high_frequency_emphasis: None,
        }
    }

    pub(super) const fn high_band() -> Self {
        Self {
            power_pair_imbalance: 0.0,
            blocking_depth: 0.25,
            // Zero preserves the physical coupling-RC recovery.
            blocking_recovery_ms: 0.0,
            high_frequency_emphasis: Some(Self::HIGH_FREQUENCY_EMPHASIS),
        }
    }
}

pub(super) struct AmplifierBranch {
    design: BranchDesign,
    preamp: RcCoupledTriodeCascade,
    power_pair: PushPull,
    power_supply: PowerSupply,
    idle_supply_volts: f64,
    supply_scale: f32,
    power_pair_input_gain: f32,
    previous_power_pair_input: f32,
    blocking_high_shelf_depth: f32,
    high_shelf_update_counter: u32,
    sample_rate: f32,
    input_high_shelf: Biquad,
    output_high_shelf: Biquad,
}

impl AmplifierBranch {
    pub(super) fn new(design: BranchDesign) -> Self {
        Self {
            design,
            preamp: RcCoupledTriodeCascade::new(),
            power_pair: PushPull::new(),
            power_supply: PowerSupply::new(SUPPLY),
            idle_supply_volts: 1.0,
            supply_scale: 1.0,
            power_pair_input_gain: 1.0,
            previous_power_pair_input: 0.0,
            blocking_high_shelf_depth: 0.0,
            high_shelf_update_counter: 0,
            sample_rate: 96_000.0,
            input_high_shelf: Biquad::new(),
            output_high_shelf: Biquad::new(),
        }
    }

    pub(super) fn prepare(&mut self, sample_rate: f32, power_pair_cold_bias_volts: f64) {
        self.sample_rate = sample_rate;
        self.input_high_shelf.reset();
        self.output_high_shelf.reset();
        if let Some(emphasis) = self.design.high_frequency_emphasis {
            self.input_high_shelf.set(
                FilterSpec::HighShelfRbj {
                    hz: emphasis.corner_hz,
                    db: emphasis.input_gain_db,
                },
                sample_rate,
            );
            self.output_high_shelf.set(
                FilterSpec::HighShelfRbj {
                    hz: emphasis.corner_hz,
                    db: emphasis.output_gain_db,
                },
                sample_rate,
            );
        }

        self.preamp.prepare(sample_rate);
        self.preamp.set_tube(Koren::T12AX7);
        self.preamp.set_level_policy(UNITY_AT_CLEAN);
        self.preamp.set_coupling_hz(12.0);
        self.preamp.set_blocking_depth(self.design.blocking_depth);
        self.preamp
            .set_blocking_recovery_ms(self.design.blocking_recovery_ms);
        self.preamp.set_cathode_bias(0.35, 40.0);

        self.power_pair.prepare(sample_rate);
        self.power_pair.set_class_ab(power_pair_cold_bias_volts);
        self.power_pair
            .set_imbalance(self.design.power_pair_imbalance);
        self.power_pair.set_drive_db(0.0);

        self.power_supply.prepare(f64::from(sample_rate));
        self.idle_supply_volts = self
            .power_supply
            .settle(IDLE_LOAD_AMPS, SUPPLY_SETTLE_SECONDS);
        self.supply_scale = 1.0;
    }

    pub(super) fn apply_live_settings(
        &mut self,
        preamp_stage_count: usize,
        preamp_drive_db: f32,
        power_pair_overdrive_db: f32,
        reference_input_level_db: f32,
    ) {
        self.preamp.set_stage_count(preamp_stage_count);
        self.preamp.set_drive_db(preamp_drive_db);
        self.preamp.set_interstage_gain_db(INTERSTAGE_DIVIDER_DB);
        self.power_pair.set_feedback_db(NEGATIVE_FEEDBACK_DB);

        // The live overdrive control selects how far the expected signal rides
        // past the power-pair knee. Cold bias remains an activation-time design
        // choice and is never remapped by this control.
        let requested_overdrive_depth_db = power_pair_overdrive_db * 1.75 - 13.0;
        let power_pair_input_knee_db =
            self.power_pair.ceiling_db() - self.power_pair.small_signal_gain_db();
        self.power_pair_input_gain = db_to_amp(
            requested_overdrive_depth_db + power_pair_input_knee_db
                - self.expected_preamp_output_db(reference_input_level_db),
        );
    }

    pub(super) fn set_blocking_high_shelf_depth(&mut self, blocking_high_shelf_depth: f32) {
        self.blocking_high_shelf_depth = blocking_high_shelf_depth.clamp(0.0, 1.0);
        if self.blocking_high_shelf_depth <= 0.001 {
            self.high_shelf_update_counter = 0;
        }
    }

    pub(super) fn expected_output_level_db(&self, input_level_db: f32) -> f32 {
        let preamp_output_level_db = self.expected_preamp_output_db(input_level_db);
        soft_limit_db(
            preamp_output_level_db
                + 20.0 * self.power_pair_input_gain.max(1e-9).log10()
                + self.power_pair.small_signal_gain_db(),
            self.power_pair.ceiling_db(),
        )
    }

    #[inline]
    pub(super) fn process_sample(&mut self, input_sample: f32) -> f32 {
        if self.design.high_frequency_emphasis.is_some() && self.blocking_high_shelf_depth > 0.001 {
            self.high_shelf_update_counter = self.high_shelf_update_counter.wrapping_add(1);
            if self.high_shelf_update_counter & 31 == 0 {
                let blocking_voltage = self.preamp.blocking_voltage(0) as f32;
                let target_high_shelf_gain_db = (14.0 - 6.0 * blocking_voltage).clamp(2.0, 14.0);
                let high_shelf_gain_db =
                    8.0 + self.blocking_high_shelf_depth * (target_high_shelf_gain_db - 8.0);
                self.input_high_shelf.set(
                    FilterSpec::HighShelfRbj {
                        hz: 480.0,
                        db: high_shelf_gain_db,
                    },
                    self.sample_rate,
                );
            }
        }

        let emphasized_input = if self.design.high_frequency_emphasis.is_some() {
            self.input_high_shelf.process(input_sample)
        } else {
            input_sample
        };
        let preamp_output = self.preamp.process_sample(emphasized_input);
        let power_pair_input = preamp_output * self.power_pair_input_gain;
        self.previous_power_pair_input = power_pair_input;
        let power_pair_output = self.power_pair.process_sample(power_pair_input);

        let load_amps =
            IDLE_LOAD_AMPS + LOAD_AMPS_PER_DRAW * (f64::from(self.power_pair.draw()) - 1.0);
        let supply_scale = (self.power_supply.tick(load_amps) / self.idle_supply_volts) as f32;
        self.supply_scale = supply_scale;
        self.power_pair.set_supply_scale(supply_scale);
        self.preamp
            .set_supply_scale(1.0 - PREAMP_SUPPLY_DROOP_AMOUNT * (1.0 - supply_scale));

        if self.design.high_frequency_emphasis.is_some() {
            self.output_high_shelf.process(power_pair_output)
        } else {
            power_pair_output
        }
    }

    pub(super) fn previous_power_pair_input(&self) -> f32 {
        self.previous_power_pair_input
    }

    pub(super) fn power_pair_draw(&self) -> f32 {
        self.power_pair.draw()
    }

    /// B+ relative to its settled idle voltage.
    pub(super) fn power_supply_voltage(&self) -> f32 {
        self.supply_scale
    }

    fn expected_preamp_output_db(&self, input_level_db: f32) -> f32 {
        let stage_gain_db = self.preamp.stage_gain_db();
        let interstage_gain_db = self.preamp.interstage_db();
        let stage_ceiling_db = self.preamp.ceiling_db();
        let mut output_level_db = input_level_db;
        for stage_index in 0..self.preamp.stage_count() {
            let current_stage_gain_db = if stage_index > 0 {
                stage_gain_db + interstage_gain_db
            } else {
                stage_gain_db
            };
            output_level_db =
                soft_limit_db(output_level_db + current_stage_gain_db, stage_ceiling_db);
        }
        output_level_db
    }

    #[cfg(test)]
    fn power_pair_cold_bias_volts(&self) -> f64 {
        self.power_pair.class_ab()
    }
}

#[inline]
fn soft_limit_db(unlimited_level_db: f32, ceiling_db: f32) -> f32 {
    let normalized_overdrive = (unlimited_level_db - ceiling_db) / SOFT_KNEE_WIDTH_DB;
    if normalized_overdrive > 20.0 {
        ceiling_db
    } else {
        unlimited_level_db - SOFT_KNEE_WIDTH_DB * (1.0 + normalized_overdrive.exp()).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_pair_overdrive_never_changes_cold_bias() {
        for branch_design in [
            BranchDesign::full_band(),
            BranchDesign::low_band(),
            BranchDesign::low_mid_band(),
            BranchDesign::high_band(),
        ] {
            let mut branch = AmplifierBranch::new(branch_design);
            branch.prepare(48_000.0, 2.24);
            branch.apply_live_settings(4, 15.0, 0.0, -12.0);
            let initial_cold_bias_volts = branch.power_pair_cold_bias_volts();
            branch.apply_live_settings(4, 15.0, 18.0, -12.0);
            assert_eq!(initial_cold_bias_volts, 2.24);
            assert_eq!(branch.power_pair_cold_bias_volts(), initial_cold_bias_volts);
        }
    }
}
