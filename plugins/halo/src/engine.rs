//! Halo's product-specific clean amplifier composition.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{Environment, ProcessCtx, Tail, Tap};
use stave_dsp::db_to_amp;
use stave_dsp::PerChannel;

mod clean_power_amplifier;
mod clean_preamp;
mod dynamic_headroom;
mod input_conditioner;
mod output_conditioner;
mod output_transformer;
mod reservoir;
mod tone_section;

use clean_power_amplifier::CleanPowerAmplifier;
use clean_preamp::CleanPreamp;
use input_conditioner::InputConditioner;
use output_conditioner::OutputConditioner;
use output_transformer::OutputTransformer;
use tone_section::ToneSection;

use crate::controls::{
    resolve_engine_settings, DynamicSettings, HaloDesign, InputSettings, LastAppliedSettings,
    Panel, PanelControlValues, PreampSettings, ToneSettings,
};
use crate::params::{HaloParameters, DEFAULT_PREAMP_GAIN_DECIBELS};

struct ProcessingLane {
    input_conditioner: InputConditioner,
    clean_preamp: CleanPreamp,
    tone_section: ToneSection,
    clean_power_amplifier: CleanPowerAmplifier,
    output_transformer: OutputTransformer,
    output_conditioner: OutputConditioner,
}

impl ProcessingLane {
    fn new() -> Self {
        Self {
            input_conditioner: InputConditioner::new(),
            clean_preamp: CleanPreamp::new(),
            tone_section: ToneSection::new(),
            clean_power_amplifier: CleanPowerAmplifier::new(),
            output_transformer: OutputTransformer::new(),
            output_conditioner: OutputConditioner::new(),
        }
    }

    fn prepare(&mut self, sample_rate: f32, design: &HaloDesign) {
        self.input_conditioner.prepare(sample_rate);
        self.clean_preamp.prepare(sample_rate);
        self.tone_section.prepare(sample_rate);
        self.clean_power_amplifier
            .prepare(sample_rate, design.power_pair_cold_bias_volts);
        self.output_transformer.prepare(sample_rate);
        self.output_conditioner.prepare(sample_rate);
    }

    #[inline]
    fn process_sample(&mut self, input: f32, preamp_gain_decibels: f32) -> f32 {
        let conditioned_input = self.input_conditioner.process_sample(input);
        let input_stage_output = self
            .clean_preamp
            .process_input_stage(conditioned_input, preamp_gain_decibels);
        let tone_stack_output = self.tone_section.process_sample(input_stage_output);
        let preamp_output = self.clean_preamp.process_recovery_stage(tone_stack_output);
        let power_amplifier_output = self.clean_power_amplifier.process_sample(preamp_output);
        let transformer_output = self
            .output_transformer
            .process_sample(power_amplifier_output);
        self.output_conditioner.process_sample(transformer_output)
    }
}

pub struct HaloEngine {
    telemetry_bus: Option<Arc<stave_core::DebugBus>>,
    parameters: Arc<HaloParameters>,
    amplifier: Amplifier,
    panel: Panel,
    preamp_gain_control: ContinuousControl,
    output_level_control: ContinuousControl,
    build_tap: Tap,
    transient_tap: Tap,
    plate_draw_tap: Tap,
    low_reservoir_tap: Tap,
    middle_reservoir_tap: Tap,
    high_reservoir_tap: Tap,
}

/// The panel re-resolves into the amplifier this often while it moves:
/// a glide steps under −30 dB per interval.
const CONTROL_INTERVAL: usize = 16;

/// The lanes and the settings last applied to them.
struct Amplifier {
    sample_rate: f32,
    lanes: PerChannel<ProcessingLane, 2>,
    design: HaloDesign,
    last_input_settings: LastAppliedSettings<InputSettings>,
    last_preamp_settings: LastAppliedSettings<PreampSettings>,
    last_tone_settings: LastAppliedSettings<ToneSettings>,
    last_dynamic_settings: LastAppliedSettings<DynamicSettings>,
    makeup_gain: f32,
}

impl Amplifier {
    /// Prepare every lane, voice it for `panel`, and settle it at its
    /// operating point: run until the output stays under 1e-6 for 256
    /// samples, at most one second.
    fn prepare(&mut self, sample_rate: f32, panel: PanelControlValues) {
        self.sample_rate = sample_rate;
        for lane in self.lanes.iter_mut() {
            lane.prepare(sample_rate, &self.design);
        }
        self.last_input_settings.reset();
        self.last_preamp_settings.reset();
        self.last_tone_settings.reset();
        self.last_dynamic_settings.reset();
        self.apply(panel);
        for lane in self.lanes.iter_mut() {
            let mut quiet = 0u32;
            for _ in 0..sample_rate as usize {
                let y = lane.process_sample(0.0, DEFAULT_PREAMP_GAIN_DECIBELS as f32);
                quiet = if y.abs() < 1e-6 { quiet + 1 } else { 0 };
                if quiet >= 256 {
                    break;
                }
            }
        }
    }

    fn apply(&mut self, panel: PanelControlValues) {
        let settings = resolve_engine_settings(panel, &self.design);
        let input_changed = self.last_input_settings.has_changed(settings.input);
        let preamp_changed = self.last_preamp_settings.has_changed(settings.preamp);
        let tone_changed = self.last_tone_settings.has_changed(settings.tone);
        let dynamics_changed = self.last_dynamic_settings.has_changed(settings.dynamics);

        for lane in self.lanes.iter_mut() {
            if input_changed {
                lane.input_conditioner.apply_settings(settings.input);
            }
            if preamp_changed {
                lane.clean_preamp.apply_settings(settings.preamp);
            }
            if tone_changed {
                lane.tone_section.apply_settings(settings.tone);
            }
            if dynamics_changed {
                lane.clean_power_amplifier.apply_settings(settings.dynamics);
            }
        }

        if dynamics_changed {
            let representative_lane = &self.lanes[0];
            let expected_small_signal_output_db = self.design.reference_input_level_db
                + representative_lane
                    .clean_preamp
                    .fixed_stage_small_signal_gain_db()
                + representative_lane
                    .clean_power_amplifier
                    .small_signal_gain_db();
            self.makeup_gain = db_to_amp(
                (self.design.target_output_level_db - expected_small_signal_output_db)
                    .clamp(-30.0, 30.0),
            );
        }
    }
}

impl stave_core::Processor for HaloEngine {
    type Params = HaloParameters;

    fn new(parameters: Arc<HaloParameters>) -> Self {
        Self {
            telemetry_bus: None,
            amplifier: Amplifier {
                sample_rate: 48_000.0,
                lanes: PerChannel::new(|_| ProcessingLane::new()),
                design: HaloDesign::default(),
                last_input_settings: LastAppliedSettings::new(),
                last_preamp_settings: LastAppliedSettings::new(),
                last_tone_settings: LastAppliedSettings::new(),
                last_dynamic_settings: LastAppliedSettings::new(),
                makeup_gain: 1.0,
            },
            panel: Panel::new(&parameters),
            preamp_gain_control: ContinuousControl::new(&parameters.preamp_gain),
            output_level_control: ContinuousControl::new(&parameters.output_level),
            parameters,
            build_tap: Tap::new("build"),
            transient_tap: Tap::new("transient"),
            plate_draw_tap: Tap::new("plate_draw"),
            low_reservoir_tap: Tap::new("low_reservoir"),
            middle_reservoir_tap: Tap::new("mid_reservoir"),
            high_reservoir_tap: Tap::new("high_reservoir"),
        }
    }

    fn activate(&mut self, environment: Environment) {
        let rate = f64::from(environment.sample_rate);
        self.preamp_gain_control
            .prepare(rate, &self.parameters.preamp_gain);
        self.output_level_control
            .prepare(rate, &self.parameters.output_level);
        self.panel.prepare(rate, &self.parameters);
        self.amplifier.prepare(
            environment.sample_rate,
            PanelControlValues::read(&self.parameters),
        );
    }

    fn process(&mut self, context: &mut ProcessCtx<'_>) {
        let parameters = self.parameters.clone();
        let mut panel = self.panel.begin_block(context.params, &parameters);
        let mut preamp_gain_values = self
            .preamp_gain_control
            .begin_block(context.params, &parameters.preamp_gain);
        let mut output_level_values = self
            .output_level_control
            .begin_block(context.params, &parameters.output_level);
        let amplifier = &mut self.amplifier;
        let audio_lanes = context.lanes::<2>();
        let active_channel_count = audio_lanes.active().min(2);
        audio_lanes.run(|audio_frame| {
            let panel_values = panel.next();
            if audio_frame.index() % CONTROL_INTERVAL == 0 {
                amplifier.apply(panel_values.into());
            }
            let preamp_gain_decibels = preamp_gain_values.next();
            let output_gain = amplifier.makeup_gain * db_to_amp(output_level_values.next());
            for channel_index in 0..active_channel_count {
                let output = amplifier.lanes[channel_index]
                    .process_sample(audio_frame.input(channel_index), preamp_gain_decibels)
                    * output_gain;
                audio_frame.write(channel_index, output);
            }
        });

        if let Some(telemetry_bus) = self.telemetry_bus.clone() {
            let measurements = self.amplifier.lanes[0].clean_power_amplifier.measurements();
            self.transient_tap
                .set(&telemetry_bus, f64::from(measurements.transient_amount));
            self.plate_draw_tap
                .set(&telemetry_bus, f64::from(measurements.plate_draw));
            self.low_reservoir_tap.set(
                &telemetry_bus,
                f64::from(measurements.low_frequency_reservoir_voltage),
            );
            self.middle_reservoir_tap.set(
                &telemetry_bus,
                f64::from(measurements.middle_frequency_reservoir_voltage),
            );
            self.high_reservoir_tap.set(
                &telemetry_bus,
                f64::from(measurements.high_frequency_reservoir_voltage),
            );
        }
    }

    fn attach_telemetry(&mut self, telemetry_bus: Arc<stave_core::DebugBus>) {
        self.build_tap.set(
            &telemetry_bus,
            env!("STAVE_BUILD_STAMP").parse::<f64>().unwrap_or(0.0),
        );
        self.transient_tap.set(&telemetry_bus, 0.0);
        self.plate_draw_tap.set(&telemetry_bus, 1.0);
        self.low_reservoir_tap.set(&telemetry_bus, 1.0);
        self.middle_reservoir_tap.set(&telemetry_bus, 1.0);
        self.high_reservoir_tap.set(&telemetry_bus, 1.0);
        self.telemetry_bus = Some(telemetry_bus);
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::None
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn component_costs_for_the_record() {
        const SAMPLE_COUNT: usize = 96_000;
        let mut lane = ProcessingLane::new();
        let design = HaloDesign::default();
        lane.prepare(48_000.0, &design);
        let settings = resolve_engine_settings(
            PanelControlValues {
                bass_tone_position: 5.5,
                midrange_tone_position: 6.0,
                treble_tone_position: 5.5,
                headroom_amount: 0.7,
                bloom_amount: 0.45,
                reserve_amount: 0.7,
                response_amount: 0.6,
            },
            &design,
        );
        lane.input_conditioner.apply_settings(settings.input);
        lane.clean_preamp.apply_settings(settings.preamp);
        lane.tone_section.apply_settings(settings.tone);
        lane.clean_power_amplifier.apply_settings(settings.dynamics);

        let start = std::time::Instant::now();
        let mut output = 0.0;
        for sample_index in 0..SAMPLE_COUNT {
            let input =
                (core::f32::consts::TAU * 220.0 * sample_index as f32 / 48_000.0).sin() * 0.25;
            output = lane.process_sample(
                std::hint::black_box(input),
                DEFAULT_PREAMP_GAIN_DECIBELS as f32,
            );
        }
        std::hint::black_box(output);
        eprintln!(
            "halo power measurements: {:?}",
            lane.clean_power_amplifier.measurements()
        );
        eprintln!(
            "halo complete lane ns/sample: {:.1}",
            start.elapsed().as_secs_f64() * 1e9 / SAMPLE_COUNT as f64
        );
    }
}
