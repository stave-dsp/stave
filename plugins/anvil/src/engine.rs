//! Anvil's product-specific amplifier composition.
//!
//! ```text
//! engine.rs              orchestration (resolve → apply → render)
//! parallel_amplifier.rs  blend between complete amplifier topologies
//! parallel_amplifier/    nonlinear branch and split-band topology
//! output_conditioner.rs  post-amplifier response and bandwidth
//! ```
//! Path per lane: input high-pass → difference-tone generation → parallel
//! amplifier → high-band limiting → passive tone stack → output conditioner.
//! Reusable mechanisms live in stave-dsp; this topology stays concrete because
//! it is Anvil's product design.

extern crate alloc;
use alloc::sync::Arc;
use stave_dsp::db_to_amp;
use stave_dsp::FilterSpec;

use stave_core::params::ContinuousControl;
use stave_core::{Environment, ProcessCtx, Tail, Tap};
use stave_dsp::{Biquad, PerChannel};

mod difference_tone;
mod high_band_limiter;
mod output_conditioner;
mod parallel_amplifier;
mod tone_section;

use difference_tone::DifferenceToneGenerator;
use high_band_limiter::HighBandLimiter;
use output_conditioner::OutputConditioner;
use parallel_amplifier::ParallelAmplifier;
pub use parallel_amplifier::RcCoupledTriodeCascade;
use tone_section::ToneSection;

use crate::controls::{
    resolve_engine_settings, AmplifierSettings, AnvilDesign, InputSettings, LastAppliedSettings,
    OutputSettings, Panel, PanelControlValues, ToneSettings,
};
use crate::params::AnvilParameters;

struct ProcessingLane {
    input_highpass_filter: Biquad,
    difference_tone_generator: DifferenceToneGenerator,
    /// Post-parallel amplifier slew (field-ratified in the SMOOTH pair): big
    /// fast swings round, small ones pass — level-dependent
    /// transient smoothing, the Miller-physics cousin.
    slew_limiter: stave_dsp::Slew,
    previous_amplifier_output: f32,
    // THE OVERSAMPLER IS GONE (field-ratified 2026-08-06): measured
    // at the ratified voice, the 2x kernel bought 2 dB of alias
    // floor (-45.8 vs -43.8 dBc worst inharmonic, 3.5 kHz probe at
    // 44.1k) for ~45% of the plugin's CPU — the ADAA curves ARE
    // the antialiasing. The parallel amplifier runs at the host rate; zero
    // latency came with it.
    parallel_amplifier: ParallelAmplifier,
    high_band_limiter: HighBandLimiter,
    tone_section: ToneSection,
    output_conditioner: OutputConditioner,
}

/// REGEN (movement rail): a whisper of the parallel amplifier's own output fed
/// back to its input — the guitarist standing near the cab, dosed.
/// The one mechanism the field's ear kept naming ("feels like
/// feedback"). <= -99 = off, bit-identical.
const AMPLIFIER_FEEDBACK_DB: f32 = -100.0;

/// The engine's declared observation points.
struct AnvilTaps {
    build: Tap,
    classic_out_db: Tap,
    low_out_db: Tap,
    high_out_db: Tap,
    pair_in_db: Tap,
    draw: Tap,
    v_min: Tap,
    v_max: Tap,
    draw_mean: Tap,
}

impl Default for AnvilTaps {
    fn default() -> Self {
        Self {
            build: Tap::new("build"),
            classic_out_db: Tap::new("classic_out_db"),
            low_out_db: Tap::new("low_out_db"),
            high_out_db: Tap::new("high_out_db"),
            pair_in_db: Tap::new("pair_in_db"),
            draw: Tap::new("draw"),
            v_min: Tap::new("v_min"),
            v_max: Tap::new("v_max"),
            draw_mean: Tap::new("draw_mean"),
        }
    }
}

pub struct AnvilEngine {
    telemetry_bus: Option<Arc<stave_core::DebugBus>>,
    taps: AnvilTaps,
    parameters: Arc<AnvilParameters>,
    amplifier: Amplifier,
    panel: Panel,
    output_level_control: ContinuousControl,
}

/// The panel re-resolves into the amplifier this often while it moves:
/// a 20 ms glide steps under −30 dB per interval.
const CONTROL_INTERVAL: usize = 16;

/// The lanes and the settings last applied to them.
struct Amplifier {
    sample_rate: f32,
    lanes: PerChannel<ProcessingLane, 2>,
    design: AnvilDesign,
    last_input_settings: LastAppliedSettings<InputSettings>,
    last_difference_tone_injection_amount: LastAppliedSettings<f32>,
    last_amplifier_settings: LastAppliedSettings<AmplifierSettings>,
    last_tone_settings: LastAppliedSettings<ToneSettings>,
    last_output_settings: LastAppliedSettings<OutputSettings>,
    /// Computed makeup: the parallel amplifier reports its small-signal
    /// gain and saturated ceiling, so the expected output for the
    /// reference input is min(input + gain, ceiling), and makeup is the
    /// target minus that.
    makeup_gain: f32,
}

impl Amplifier {
    /// Prepare every lane, voice it for `panel`, and settle it at its
    /// real operating point (bias, blocking, cathode, supply), so silence
    /// in is silence out from the first block: run until the output stays
    /// under 1e-5 for 256 samples, at most one second.
    fn prepare(&mut self, sample_rate: f32, panel: PanelControlValues) {
        self.sample_rate = sample_rate;
        for lane in self.lanes.iter_mut() {
            lane.input_highpass_filter.reset();
            lane.difference_tone_generator.prepare(sample_rate);
            lane.slew_limiter.set_rates(8.0, 8.0, sample_rate);
            lane.parallel_amplifier
                .prepare(sample_rate, self.design.power_pair_cold_bias_volts);
            lane.high_band_limiter.prepare(sample_rate);
            lane.tone_section.prepare(sample_rate);
            lane.output_conditioner.prepare(sample_rate);
        }
        self.last_input_settings.reset();
        self.last_difference_tone_injection_amount.reset();
        self.last_amplifier_settings.reset();
        self.last_tone_settings.reset();
        self.last_output_settings.reset();
        self.apply(panel);
        for lane in self.lanes.iter_mut() {
            let mut quiet = 0u32;
            for _ in 0..sample_rate as usize {
                quiet = if process_lane_sample(lane, 0.0).abs() < 1e-5 {
                    quiet + 1
                } else {
                    0
                };
                if quiet >= 256 {
                    break;
                }
            }
        }
    }

    fn apply(&mut self, panel: PanelControlValues) {
        let settings = resolve_engine_settings(panel, &self.design);
        let input_settings_changed = self.last_input_settings.has_changed(settings.input);
        let difference_tone_settings_changed = self
            .last_difference_tone_injection_amount
            .has_changed(settings.difference_tone_injection_amount);
        let amplifier_settings_changed =
            self.last_amplifier_settings.has_changed(settings.amplifier);
        let tone_settings_changed = self.last_tone_settings.has_changed(settings.tone);
        let output_settings_changed = self
            .last_output_settings
            .has_changed(settings.output_conditioner);

        for processing_lane in self.lanes.iter_mut() {
            if input_settings_changed {
                processing_lane.input_highpass_filter.set(
                    FilterSpec::Highpass {
                        hz: settings.input.highpass_cutoff_hz,
                        q: 0.71,
                    },
                    self.sample_rate,
                );
            }
            if difference_tone_settings_changed {
                processing_lane
                    .difference_tone_generator
                    .set_injection_amount(settings.difference_tone_injection_amount);
            }
            if input_settings_changed || amplifier_settings_changed {
                processing_lane
                    .parallel_amplifier
                    .apply_settings(settings.amplifier, settings.input);
            }
            if tone_settings_changed {
                processing_lane.tone_section.apply_settings(settings.tone);
            }
            if output_settings_changed {
                processing_lane
                    .output_conditioner
                    .apply_settings(settings.output_conditioner);
            }
        }
        if amplifier_settings_changed || tone_settings_changed {
            let expected_amplifier_output_db = self.lanes[0]
                .parallel_amplifier
                .expected_output_level_db(self.design.reference_input_level_db);
            let tone_section_level_compensation_gain =
                self.lanes[0].tone_section.level_compensation_gain();
            self.makeup_gain = db_to_amp(
                (self.design.target_output_level_db - expected_amplifier_output_db)
                    .clamp(-40.0, 60.0),
            ) * tone_section_level_compensation_gain;
        }
    }
}

#[inline]
fn process_lane_sample(processing_lane: &mut ProcessingLane, input_sample: f32) -> f32 {
    let conditioned_input = processing_lane.input_highpass_filter.process(input_sample)
        + if AMPLIFIER_FEEDBACK_DB > -99.0 {
            db_to_amp(AMPLIFIER_FEEDBACK_DB) * processing_lane.previous_amplifier_output
        } else {
            0.0
        };
    let generated_input = processing_lane
        .difference_tone_generator
        .process_sample(conditioned_input);
    let unbounded_amplifier_output = processing_lane
        .parallel_amplifier
        .process_sample(generated_input);
    let amplifier_output = processing_lane
        .high_band_limiter
        .process_sample(unbounded_amplifier_output);
    processing_lane.previous_amplifier_output = amplifier_output;
    let slew_limited_output =
        stave_dsp::kernel::node::Stage::tick(&mut processing_lane.slew_limiter, amplifier_output);
    let tone_stack_output = processing_lane
        .tone_section
        .process_sample(slew_limited_output);
    processing_lane
        .output_conditioner
        .process_sample(tone_stack_output)
}

impl stave_core::Processor for AnvilEngine {
    type Params = AnvilParameters;

    fn new(parameters: Arc<AnvilParameters>) -> Self {
        Self {
            telemetry_bus: None,
            taps: AnvilTaps::default(),
            amplifier: Amplifier {
                sample_rate: 48_000.0,
                lanes: PerChannel::new(|_| ProcessingLane {
                    input_highpass_filter: Biquad::new(),
                    difference_tone_generator: DifferenceToneGenerator::new(),
                    slew_limiter: stave_dsp::Slew::new(8.0, 8.0),
                    previous_amplifier_output: 0.0,
                    parallel_amplifier: ParallelAmplifier::new(),
                    high_band_limiter: HighBandLimiter::new(),
                    tone_section: ToneSection::new(),
                    output_conditioner: OutputConditioner::new(),
                }),
                design: AnvilDesign::default(),
                last_input_settings: LastAppliedSettings::new(),
                last_difference_tone_injection_amount: LastAppliedSettings::new(),
                last_amplifier_settings: LastAppliedSettings::new(),
                last_tone_settings: LastAppliedSettings::new(),
                last_output_settings: LastAppliedSettings::new(),
                makeup_gain: 1.0,
            },
            panel: Panel::new(&parameters),
            output_level_control: ContinuousControl::new(&parameters.output_level),
            parameters,
        }
    }

    fn activate(&mut self, environment: Environment) {
        let rate = f64::from(environment.sample_rate);
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
            let final_output_gain =
                db_to_amp(output_level_values.next()) * amplifier.makeup_gain * 0.5;
            for channel_index in 0..active_channel_count {
                let output_sample = process_lane_sample(
                    &mut amplifier.lanes[channel_index],
                    audio_frame.input(channel_index),
                ) * final_output_gain;
                audio_frame.write(channel_index, output_sample);
            }
        });
        if let Some(telemetry_bus) = self.telemetry_bus.as_deref() {
            let measurements = self.amplifier.lanes[0]
                .parallel_amplifier
                .take_measurements();
            let amplitude_to_decibels = |amplitude: f32| 20.0 * amplitude.max(1e-9).log10();
            self.taps.classic_out_db.set(
                telemetry_bus,
                f64::from(amplitude_to_decibels(measurements.full_band_output_peak)),
            );
            self.taps.low_out_db.set(
                telemetry_bus,
                f64::from(amplitude_to_decibels(measurements.low_band_output_peak)),
            );
            self.taps.high_out_db.set(
                telemetry_bus,
                f64::from(amplitude_to_decibels(measurements.high_band_output_peak)),
            );
            self.taps.pair_in_db.set(
                telemetry_bus,
                f64::from(amplitude_to_decibels(
                    measurements.full_band_power_pair_input_peak,
                )),
            );
            self.taps.draw.set(
                telemetry_bus,
                f64::from(measurements.full_band_power_pair_draw_peak),
            );
            self.taps.v_min.set(
                telemetry_bus,
                f64::from(measurements.full_band_supply_voltage_minimum),
            );
            self.taps.v_max.set(
                telemetry_bus,
                f64::from(measurements.full_band_supply_voltage_maximum),
            );
            self.taps.draw_mean.set(
                telemetry_bus,
                f64::from(
                    measurements.full_band_power_pair_draw_sum
                        / measurements.full_band_power_pair_draw_sample_count.max(1.0),
                ),
            );
        }
    }

    fn attach_telemetry(&mut self, telemetry_bus: Arc<stave_core::DebugBus>) {
        // The render self-identifies: `build` = compile timestamp
        // (build.rs). A bench measurement without a fresh stamp is
        // fiction — the poisoned-measurement day's law.
        self.taps.build.set(
            &telemetry_bus,
            env!("STAVE_BUILD_STAMP").parse::<f64>().unwrap_or(0.0),
        );
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

    fn measure_nanoseconds_per_sample(mut process_sample: impl FnMut(f32) -> f32) -> f64 {
        const SAMPLES: usize = 96_000;
        let start = std::time::Instant::now();
        let mut output_sample = 0.0;
        for sample_index in 0..SAMPLES {
            let input_sample =
                (core::f32::consts::TAU * 220.0 * sample_index as f32 / 48_000.0).sin() * 0.25;
            output_sample = process_sample(std::hint::black_box(input_sample));
        }
        std::hint::black_box(output_sample);
        start.elapsed().as_secs_f64() * 1e9 / SAMPLES as f64
    }

    #[test]
    fn component_costs_for_the_record() {
        let design = AnvilDesign::default();
        let settings = resolve_engine_settings(
            PanelControlValues {
                preamp_gain_db: 18.0,
                input_low_cut_hz: 120.0,
                bass_tone_position: 6.25,
                midrange_tone_position: 4.583_333,
                treble_tone_position: 7.5,
                presence_tone_db: 2.5,
                depth_tone_db: 2.5,
                topology_separation_amount: 0.5,
                power_pair_overdrive_db: 12.0,
                difference_tone_amount: 0.0,
                high_frequency_emphasis_amount: 0.0,
            },
            &design,
        );
        let mut amplifier = ParallelAmplifier::new();
        amplifier.prepare(48_000.0, design.power_pair_cold_bias_volts);
        amplifier.apply_settings(settings.amplifier, settings.input);
        let mut tone_section = ToneSection::new();
        tone_section.prepare(48_000.0);
        tone_section.apply_settings(settings.tone);
        let mut output_conditioner = OutputConditioner::new();
        output_conditioner.prepare(48_000.0);
        output_conditioner.apply_settings(settings.output_conditioner);

        let amplifier_nanoseconds_per_sample =
            measure_nanoseconds_per_sample(move |input_sample| {
                amplifier.process_sample(input_sample)
            });
        let tone_section_nanoseconds_per_sample =
            measure_nanoseconds_per_sample(move |input_sample| {
                tone_section.process_sample(input_sample)
            });
        let output_conditioner_nanoseconds_per_sample =
            measure_nanoseconds_per_sample(move |input_sample| {
                output_conditioner.process_sample(input_sample)
            });
        eprintln!(
            "anvil component ns/sample: amplifier={amplifier_nanoseconds_per_sample:.1}, \
             tone={tone_section_nanoseconds_per_sample:.1}, \
             output={output_conditioner_nanoseconds_per_sample:.1}"
        );
    }
}
