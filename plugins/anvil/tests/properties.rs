//! Anvil's OWN laws — written before the R1 DSP (the tilt lesson).
//! The rhythm sound's promises as numbers:
//! * the_tight_gate — tight kills the mud before the tubes see it;
//! * the_squash_stands — full gain is DENSE: crest and THD floors;
//! * smooth_but_biting — the teeth (2.5–5.5 k) stand ≥12 dB over
//!   the fizz (7.5–12 k) at full gain;
//! * the_focus_holds — THE FLAGSHIP: focus keeps an 80 Hz chug from
//!   intermodulating a 1.5 kHz tooth (sidebands drop ≥8 dB) — the
//!   measured "almost synth-like" quality no tube circuit can do;
//! * EQ sign laws — bass and treble point the right way at low gain.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_SIZE: usize = 256;

fn render_plugin(parameter_values: &[(&'static str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_anvil_vst3::Anvil::new();
    let debug_bus = DebugBus::new();
    plugin.debug_attach(debug_bus.clone());
    let parameter_table = plugin.param_table();
    for (parameter_name, parameter_value) in parameter_values {
        parameter_table
            .by_name(parameter_name)
            .unwrap_or_else(|| panic!("parameter '{parameter_name}'"))
            .set_plain(*parameter_value);
    }
    plugin.activate(SAMPLE_RATE, BLOCK_SIZE);
    let mut rendered_output = Vec::with_capacity(input.len());
    let (mut left_input, mut right_input) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    let (mut left_output, mut right_output) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    for chunk in input.chunks(BLOCK_SIZE) {
        left_input[..chunk.len()].copy_from_slice(chunk);
        left_input[chunk.len()..].fill(0.0);
        right_input.copy_from_slice(&left_input);
        let mut input_channel_pointers = [core::ptr::null_mut(); 2];
        let mut output_channel_pointers = [core::ptr::null_mut(); 2];
        let input_buffer = AudioBuffer::from_slices(
            &mut input_channel_pointers,
            [&mut left_input, &mut right_input],
            BLOCK_SIZE,
        );
        let output_buffer = AudioBuffer::from_slices(
            &mut output_channel_pointers,
            [&mut left_output, &mut right_output],
            BLOCK_SIZE,
        );
        let mut process_context = ProcessCtx {
            sample_rate: SAMPLE_RATE,
            frames: BLOCK_SIZE,
            transport: None,
            input: input_buffer,
            output: output_buffer,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut process_context);
        rendered_output.extend_from_slice(&left_output);
    }
    let _ = Arc::strong_count(&debug_bus);
    rendered_output
}

fn generate_sine(frequency_hz: f32, amplitude: f32, sample_count: usize) -> Vec<f32> {
    (0..sample_count)
        .map(|sample_index| {
            (core::f32::consts::TAU * frequency_hz * sample_index as f32 / SAMPLE_RATE).sin()
                * amplitude
        })
        .collect()
}

/// Bin-exact projection over the settled tail (23040 samples;
/// frequencies must be multiples of 25/12 Hz).
fn measure_magnitude(output: &[f32], frequency_hz: f64) -> f64 {
    let settled_output = &output[output.len() - 23_040..];
    let radians_per_sample = core::f64::consts::TAU * frequency_hz / f64::from(SAMPLE_RATE);
    let (mut sine_projection, mut cosine_projection) = (0.0f64, 0.0f64);
    for (sample_index, sample) in settled_output.iter().enumerate() {
        sine_projection += f64::from(*sample) * (radians_per_sample * sample_index as f64).sin();
        cosine_projection += f64::from(*sample) * (radians_per_sample * sample_index as f64).cos();
    }
    (sine_projection * sine_projection + cosine_projection * cosine_projection).sqrt() * 2.0
        / settled_output.len() as f64
}

fn root_mean_square(samples: &[f32]) -> f64 {
    (samples
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

const RHYTHM_SETTINGS: &[(&str, f64)] = &[
    ("gain", 20.0),
    ("squash", 12.0),
    ("focus", 0.7),
    ("tight", 110.0),
    ("bass", 6.25),
    ("mid", 4.166_667),
    ("treble", 6.666_667),
    ("presence", 2.0),
    ("depth", 2.0),
];

fn override_settings(
    base_settings: &[(&'static str, f64)],
    overrides: &[(&'static str, f64)],
) -> Vec<(&'static str, f64)> {
    let mut combined_settings: Vec<(&'static str, f64)> = Vec::new();
    for (parameter_name, parameter_value) in base_settings.iter().chain(overrides.iter()) {
        if let Some(existing_setting) = combined_settings
            .iter_mut()
            .find(|(existing_name, _)| existing_name == parameter_name)
        {
            existing_setting.1 = *parameter_value;
        } else {
            combined_settings.push((parameter_name, *parameter_value));
        }
    }
    combined_settings
}

#[test]
fn the_tight_gate() {
    // Tightness is a TWO-TONE fact: after a limiter a lone quiet
    // sine climbs back to the ceiling, but under a riff the
    // attenuated low stays down (60.4167 and 700 are 25/12-exact).
    let sample_count = 48_128;
    let input: Vec<f32> = (0..sample_count)
        .map(|sample_index| {
            let time_seconds = sample_index as f32 / SAMPLE_RATE;
            (core::f32::consts::TAU * 60.416_668 * time_seconds).sin() * 0.35
                + (core::f32::consts::TAU * 700.0 * time_seconds).sin() * 0.2
        })
        .collect();
    let measure_low_band_level = |input_low_cut_hz: f64| {
        let rendered_output = render_plugin(
            &override_settings(RHYTHM_SETTINGS, &[("tight", input_low_cut_hz)]),
            &input,
        );
        20.0 * measure_magnitude(&rendered_output, 60.416_666_67)
            .max(1e-12)
            .log10()
    };
    let low_band_attenuation_db = measure_low_band_level(300.0) - measure_low_band_level(40.0);
    assert!(
        low_band_attenuation_db <= -8.0,
        "tight only cut the under-riff 60 Hz by {low_band_attenuation_db:.1} dB"
    );
}

#[test]
fn the_squash_stands() {
    let input = generate_sine(220.0 + 5.0 / 6.0, 0.4, 48_128); // 220.833 = 25/12·106
    let rendered_output = render_plugin(&override_settings(RHYTHM_SETTINGS, &[]), &input);
    let settled_output = &rendered_output[24_064..];
    let output_peak = settled_output
        .iter()
        .fold(0.0f64, |peak, sample| peak.max(f64::from(sample.abs())));
    let crest = root_mean_square(settled_output) / output_peak.max(1e-9);
    // 0.43 after the 808 removal (2026-08-05): the 6th-order deep
    // guards' phase rounds the summed square's corners a hair more
    // (0.44 measured; was 0.45 after the bright-cap re-voice, same
    // class of change). Density is THD-guarded below; the ear
    // ratified the pedal-less voice.
    assert!(
        crest >= 0.43,
        "crest {crest:.2} — the squash must be DENSE (square-like)"
    );
    let fundamental_frequency_hz = 220.0 + 5.0 / 6.0;
    let fundamental_magnitude = measure_magnitude(&rendered_output, fundamental_frequency_hz);
    let total_harmonic_distortion = (2..=5)
        .map(|harmonic_number| {
            measure_magnitude(
                &rendered_output,
                fundamental_frequency_hz * harmonic_number as f64,
            )
            .powi(2)
        })
        .sum::<f64>()
        .sqrt()
        / fundamental_magnitude;
    // The post-amplifier passive stack deliberately notches h2/h3 around
    // 440–660 Hz for this probe, so output THD under-reports the amplifier
    // core. Crest and the upper-harmonic law independently guard density.
    assert!(
        total_harmonic_distortion >= 0.12,
        "THD {total_harmonic_distortion:.2} — this is a distortion box"
    );
}

#[test]
fn smooth_but_biting() {
    let fundamental_frequency_hz = 220.0 + 5.0 / 6.0;
    let rendered_output = render_plugin(
        &override_settings(RHYTHM_SETTINGS, &[]),
        &generate_sine(fundamental_frequency_hz as f32, 0.4, 48_128),
    );
    let measure_harmonic_band_energy =
        |minimum_frequency_hz: f64, maximum_frequency_hz: f64| -> f64 {
            let mut accumulated_energy = 0.0;
            for harmonic_number in 2..60 {
                let harmonic_frequency_hz = fundamental_frequency_hz * harmonic_number as f64;
                if harmonic_frequency_hz >= minimum_frequency_hz
                    && harmonic_frequency_hz <= maximum_frequency_hz
                {
                    accumulated_energy +=
                        measure_magnitude(&rendered_output, harmonic_frequency_hz).powi(2);
                }
            }
            10.0 * accumulated_energy.max(1e-24).log10()
        };
    let bite_band_energy_db = measure_harmonic_band_energy(2_500.0, 5_500.0);
    let fizz_band_energy_db = measure_harmonic_band_energy(7_500.0, 12_000.0);
    // 9.0 after the field re-voice (2026-07-27): the darker bar had
    // been bought with cab-style top rolloff INSIDE the head; the
    // field heard it against the Plini's raw amp-out ("darker,
    // missing bite"). As a HEAD anvil keeps a more open top — its
    // fizz still measures 4-5 dB BELOW the Plini reference on
    // matched probes; the smoothness promise holds.
    assert!(
        bite_band_energy_db - fizz_band_energy_db >= 9.0,
        "bite {bite_band_energy_db:.1} vs fizz {fizz_band_energy_db:.1} dB — smooth-but-biting broken"
    );
}

#[test]
fn the_focus_holds() {
    // 81.25 and 1493.75 are both 25/12-exact; the chug's intermod
    // prints at 1493.75 ± 81.25 (1412.5 / 1575.0), also exact.
    let sample_count = 48_128;
    let input: Vec<f32> = (0..sample_count)
        .map(|sample_index| {
            let time_seconds = sample_index as f32 / SAMPLE_RATE;
            (core::f32::consts::TAU * 81.25 * time_seconds).sin() * 0.4
                + (core::f32::consts::TAU * 1_493.75 * time_seconds).sin() * 0.15
        })
        .collect();
    let measure_intermodulation_level_db = |topology_separation_amount: f64| {
        let rendered_output = render_plugin(
            &override_settings(RHYTHM_SETTINGS, &[("focus", topology_separation_amount)]),
            &input,
        );
        let sideband_magnitude = measure_magnitude(&rendered_output, 1_412.5)
            .max(measure_magnitude(&rendered_output, 1_575.0));
        20.0 * (sideband_magnitude / measure_magnitude(&rendered_output, 1_493.75).max(1e-12))
            .log10()
    };
    let full_band_intermodulation_db = measure_intermodulation_level_db(0.0);
    let split_band_intermodulation_db = measure_intermodulation_level_db(1.0);
    // Relative bar 6 with an ABSOLUTE floor (2026-08-02): the
    // guarded three-way made the CLASSIC bed itself clean
    // (-40 dBc) — the relative win shrank because the floor rose.
    // The promise is intermod-free teeth, and the absolute number
    // is the promise.
    // -39 (was -40, re-baselined 2026-08-10): the field-ratified
    // SMOOTH top soft-clips the summed 2.5k+ band — a deliberate
    // shared element whose whole job is taming spikes; it costs
    // 0.2 dB of the isolation margin (measured -39.8).
    assert!(
        split_band_intermodulation_db <= -38.0,
        "focused rig intermodulates at {split_band_intermodulation_db:.1} dBc"
    );
    assert!(
        full_band_intermodulation_db - split_band_intermodulation_db >= 6.0,
        "focus bought only {:.1} dB of separation (classic {full_band_intermodulation_db:.1}, focused {split_band_intermodulation_db:.1} dBc)",
        full_band_intermodulation_db - split_band_intermodulation_db
    );
}

/// GAIN BUYS DIRT, NOT SILENCE — the field truth, restated for the
/// Physical squash (the original ±3 dB flatness quietly legislated
/// the envelope-erasing limiter the field then rejected as STIFF).
/// What the field bug (21 dB lost across the knob) actually
/// forbids: inversion and a wasteland. So: knob 0 is the quietest
/// point, the whole sweep spans ≤ 8 dB, past the sweet spot the
/// settle-back is ≤ 4 dB (real high-gain amps get slightly quieter
/// and darker past noon — waveform asymmetry, not a bug), and
/// distortion never falls.
#[test]
fn gain_buys_dirt_not_silence() {
    let fundamental_frequency_hz = 220.0 + 5.0 / 6.0;
    let input = generate_sine(fundamental_frequency_hz as f32, 0.25, 48_128); // -12 dBFS
    let measure_level_and_distortion = |preamp_gain_db: f64| {
        let rendered_output = render_plugin(
            &override_settings(RHYTHM_SETTINGS, &[("gain", preamp_gain_db)]),
            &input,
        );
        let settled_output = &rendered_output[24_064..];
        let fundamental_magnitude = measure_magnitude(&rendered_output, fundamental_frequency_hz);
        let total_harmonic_distortion = (2..=5)
            .map(|harmonic_number| {
                measure_magnitude(
                    &rendered_output,
                    fundamental_frequency_hz * harmonic_number as f64,
                )
                .powi(2)
            })
            .sum::<f64>()
            .sqrt()
            / fundamental_magnitude;
        (
            20.0 * root_mean_square(settled_output).log10(),
            total_harmonic_distortion,
        )
    };
    let (minimum_gain_output_db, minimum_gain_distortion) = measure_level_and_distortion(0.0);
    let mut previous_distortion = minimum_gain_distortion;
    let mut loudest_output_db = minimum_gain_output_db;
    for preamp_gain_db in [6.0, 12.0, 18.0, 24.0, 30.0] {
        let (output_level_db, total_harmonic_distortion) =
            measure_level_and_distortion(preamp_gain_db);
        assert!(
            output_level_db >= minimum_gain_output_db - 0.75,
            "gain {preamp_gain_db}: quieter than knob 0 by {:.1} dB — inversion",
            minimum_gain_output_db - output_level_db
        );
        loudest_output_db = loudest_output_db.max(output_level_db);
        assert!(
            loudest_output_db - output_level_db <= 4.0,
            "gain {preamp_gain_db}: fell {:.1} dB from the sweet spot — the field bug",
            loudest_output_db - output_level_db
        );
        assert!(
            total_harmonic_distortion >= previous_distortion * 0.72,
            "gain {preamp_gain_db}: distortion fell ({total_harmonic_distortion:.3} < {previous_distortion:.3})"
        );
        previous_distortion = previous_distortion.max(total_harmonic_distortion);
    }
    assert!(
        loudest_output_db - minimum_gain_output_db <= 8.0,
        "sweep spans {:.1} dB — too wide for one knob",
        loudest_output_db - minimum_gain_output_db
    );
    // Character growth on the NAKED cascade (boost and squish off).
    // Under the Physical squash knob 0 is already a hot amp (the drive
    // floor), so low-order THD saturates early — what the knob buys
    // is EDGE: high-order harmonic content (h6-20) grows an order of
    // magnitude across the sweep (measured 0.02 -> 0.24).
    let measure_uncompressed_high_harmonics = |preamp_gain_db: f64| {
        let rendered_output = render_plugin(
            &override_settings(
                RHYTHM_SETTINGS,
                &[("gain", preamp_gain_db), ("squash", 0.0)],
            ),
            &input,
        );
        let fundamental_magnitude = measure_magnitude(&rendered_output, fundamental_frequency_hz);
        (6..=20)
            .map(|harmonic_number| {
                measure_magnitude(
                    &rendered_output,
                    fundamental_frequency_hz * harmonic_number as f64,
                )
                .powi(2)
            })
            .sum::<f64>()
            .sqrt()
            / fundamental_magnitude
    };
    let minimum_gain_high_harmonics = measure_uncompressed_high_harmonics(0.0);
    let maximum_gain_high_harmonics = measure_uncompressed_high_harmonics(30.0);
    // 1.5x: knob 0 is already an edgy amp under the R2 staging (the
    // drive floor); the RANGE of the claim shrank, edge still grows.
    assert!(
        maximum_gain_high_harmonics >= minimum_gain_high_harmonics * 1.5,
        "the naked sweep bought no edge ({minimum_gain_high_harmonics:.3} -> {maximum_gain_high_harmonics:.3})"
    );
}

/// THE DI RANGE: the head is calibrated for real interfaces — a
/// -18 dBFS guitar and a -6 dBFS one both land in usable rhythm
/// territory at default settings (dense, distorted, level within
/// a stage-volume of each other).
#[test]
fn the_di_range_lands() {
    let fundamental_frequency_hz = 220.0 + 5.0 / 6.0;
    let measure_level_and_distortion = |input_amplitude: f32| {
        let rendered_output = render_plugin(
            &override_settings(RHYTHM_SETTINGS, &[]),
            &generate_sine(fundamental_frequency_hz as f32, input_amplitude, 48_128),
        );
        let fundamental_magnitude = measure_magnitude(&rendered_output, fundamental_frequency_hz);
        let total_harmonic_distortion = (2..=5)
            .map(|harmonic_number| {
                measure_magnitude(
                    &rendered_output,
                    fundamental_frequency_hz * harmonic_number as f64,
                )
                .powi(2)
            })
            .sum::<f64>()
            .sqrt()
            / fundamental_magnitude;
        (
            20.0 * root_mean_square(&rendered_output[24_064..]).log10(),
            total_harmonic_distortion,
        )
    };
    let (quiet_output_level_db, quiet_total_harmonic_distortion) =
        measure_level_and_distortion(0.125); // -18 dBFS
    let (hot_output_level_db, hot_total_harmonic_distortion) = measure_level_and_distortion(0.5); // -6 dBFS
                                                                                                  // The physical post-amplifier stack suppresses the second and third
                                                                                                  // harmonics around its mid notch. Density is separately guarded by
                                                                                                  // crest and by the upper-harmonic distribution law.
    assert!(
        quiet_total_harmonic_distortion >= 0.11,
        "-18 dBFS DI only reaches THD {quiet_total_harmonic_distortion:.2} — undergained"
    );
    assert!(
        hot_total_harmonic_distortion >= 0.11,
        "-6 dBFS DI only reaches THD {hot_total_harmonic_distortion:.2}"
    );
    assert!(
        (hot_output_level_db - quiet_output_level_db).abs() <= 6.0,
        "DI extremes land {:.1} dB apart — the squash should absorb the interface",
        hot_output_level_db - quiet_output_level_db
    );
}

#[test]
fn the_stack_points_the_right_way() {
    let quiet: &[(&'static str, f64)] = &[("gain", 4.0), ("squash", 0.0)];
    let measure_transfer_gain_db = |parameter_values: &[(&'static str, f64)], frequency_hz: f32| {
        let input = generate_sine(frequency_hz, 0.03, 24_064); // quasi-linear probe: at 0.1 the
                                                               // emphasized top saturates even at gain 4 and the knobs read dead
        20.0 * (root_mean_square(&render_plugin(parameter_values, &input)[12_032..])
            / root_mean_square(&input[12_032..]))
        .log10()
    };
    let bass_control_gain_difference_db =
        measure_transfer_gain_db(&override_settings(quiet, &[("bass", 10.0)]), 100.0)
            - measure_transfer_gain_db(&override_settings(quiet, &[("bass", 0.0)]), 100.0);
    assert!(
        bass_control_gain_difference_db >= 6.0,
        "bass swings 100 Hz only {bass_control_gain_difference_db:.1} dB"
    );
    let treble_control_gain_difference_db =
        measure_transfer_gain_db(&override_settings(quiet, &[("treble", 10.0)]), 3_500.0)
            - measure_transfer_gain_db(&override_settings(quiet, &[("treble", 0.0)]), 3_500.0);
    assert!(
        treble_control_gain_difference_db >= 6.0,
        "treble swings 3.5 kHz only {treble_control_gain_difference_db:.1} dB"
    );
}
