//! Halo's product laws. These measure the audible promises rather than its
//! implementation details.

use stave_core::{AudioBuffer, Plugin, ProcessCtx};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_SIZE: usize = 256;

fn render_plugin(parameter_values: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_halo_vst3::Halo::new();
    let parameter_table = plugin.param_table();
    for (parameter_name, parameter_value) in parameter_values {
        parameter_table
            .by_name(parameter_name)
            .unwrap_or_else(|| panic!("parameter '{parameter_name}'"))
            .set_plain(*parameter_value);
    }
    plugin.activate(SAMPLE_RATE, BLOCK_SIZE);

    let mut rendered = Vec::with_capacity(input.len());
    let (mut left_input, mut right_input) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    let (mut left_output, mut right_output) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    for input_chunk in input.chunks(BLOCK_SIZE) {
        left_input[..input_chunk.len()].copy_from_slice(input_chunk);
        left_input[input_chunk.len()..].fill(0.0);
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
        rendered.extend_from_slice(&left_output[..input_chunk.len()]);
    }
    rendered
}

fn sine(frequency_hz: f32, amplitude: f32, sample_count: usize) -> Vec<f32> {
    (0..sample_count)
        .map(|sample_index| {
            (core::f32::consts::TAU * frequency_hz * sample_index as f32 / SAMPLE_RATE).sin()
                * amplitude
        })
        .collect()
}

fn magnitude_at(samples: &[f32], frequency_hz: f64, start: usize, length: usize) -> f64 {
    let radians_per_sample = core::f64::consts::TAU * frequency_hz / f64::from(SAMPLE_RATE);
    let (mut sine_projection, mut cosine_projection) = (0.0f64, 0.0f64);
    for (window_index, sample) in samples[start..start + length].iter().enumerate() {
        let absolute_sample_index = start + window_index;
        sine_projection +=
            f64::from(*sample) * (radians_per_sample * absolute_sample_index as f64).sin();
        cosine_projection +=
            f64::from(*sample) * (radians_per_sample * absolute_sample_index as f64).cos();
    }
    (sine_projection * sine_projection + cosine_projection * cosine_projection).sqrt() * 2.0
        / length as f64
}

fn root_mean_square(samples: &[f32]) -> f64 {
    (samples
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

#[test]
fn default_level_is_ready_to_play() {
    let input = sine(250.0, 0.25, 48_128);
    let output = render_plugin(&[], &input);
    let settled = &output[24_000..48_000];
    let output_level_db = 20.0 * root_mean_square(settled).max(1e-12).log10();
    let peak = settled
        .iter()
        .fold(0.0f32, |current, sample| current.max(sample.abs()));
    eprintln!("default output: {output_level_db:.2} dBFS rms, peak {peak:.3}");
    assert!(
        (-18.0..=-4.0).contains(&output_level_db),
        "default output was {output_level_db:.2} dBFS rms"
    );
    assert!(peak < 1.5, "default output escaped: {peak}");
}

#[test]
fn bloom_makes_the_sustain_richer_than_the_attack() {
    let fundamental_frequency_hz = 250.0;
    let input = sine(fundamental_frequency_hz as f32, 0.25, 48_128);
    let output = render_plugin(&[("bloom", 1.0), ("headroom", 1.0)], &input);
    const WINDOW_LENGTH: usize = 1_920;
    let second_harmonic_ratio_db = |start: usize| {
        20.0 * (magnitude_at(
            &output,
            2.0 * fundamental_frequency_hz,
            start,
            WINDOW_LENGTH,
        ) / magnitude_at(&output, fundamental_frequency_hz, start, WINDOW_LENGTH)
            .max(1e-12))
        .log10()
    };
    let attack_second_harmonic_db = second_harmonic_ratio_db(480);
    let sustain_second_harmonic_db = second_harmonic_ratio_db(36_000);
    eprintln!(
        "H2 attack {attack_second_harmonic_db:.1} dBc, sustain {sustain_second_harmonic_db:.1} dBc"
    );
    assert!(
        attack_second_harmonic_db <= -30.0,
        "high-headroom attack H2 was {attack_second_harmonic_db:.1} dBc"
    );
    // An audible change in a harmonic's level, not a fitted value.
    const AUDIBLY_RICHER_DB: f64 = 1.0;
    assert!(
        sustain_second_harmonic_db >= attack_second_harmonic_db + AUDIBLY_RICHER_DB,
        "bloom added only {:.1} dB of sustained H2",
        sustain_second_harmonic_db - attack_second_harmonic_db
    );
}

#[test]
fn reserve_protects_chime_from_bass_power_demand() {
    let sample_count = 96_128;
    let high_frequency_hz = 1_500.0;
    let bass_frequency_hz = 100.0;
    let high_only = sine(high_frequency_hz as f32, 0.12, sample_count);
    let high_and_bass: Vec<f32> = (0..sample_count)
        .map(|sample_index| {
            let time = sample_index as f32 / SAMPLE_RATE;
            (core::f32::consts::TAU * high_frequency_hz as f32 * time).sin() * 0.12
                + (core::f32::consts::TAU * bass_frequency_hz as f32 * time).sin() * 0.38
        })
        .collect();
    let measure_chime_behavior = |reserve: f64| {
        let settings = [
            ("reserve", reserve),
            ("bloom", 1.0),
            ("headroom", 1.0),
            ("gain", 0.0),
            ("response", 0.0),
        ];
        let high_only_output = render_plugin(&settings, &high_only);
        let mixed_output = render_plugin(&settings, &high_and_bass);
        let start = sample_count - 24_000;
        let high_magnitude = magnitude_at(&mixed_output, high_frequency_hz, start, 24_000);
        let level_change_db = 20.0
            * (high_magnitude
                / magnitude_at(&high_only_output, high_frequency_hz, start, 24_000).max(1e-12))
            .log10();
        let sideband_magnitude = magnitude_at(
            &mixed_output,
            high_frequency_hz - bass_frequency_hz,
            start,
            24_000,
        )
        .max(magnitude_at(
            &mixed_output,
            high_frequency_hz + bass_frequency_hz,
            start,
            24_000,
        ));
        let sideband_level_db = 20.0 * (sideband_magnitude / high_magnitude.max(1e-12)).log10();
        (level_change_db, sideband_level_db)
    };
    let (shared_loss_db, shared_sideband_db) = measure_chime_behavior(0.0);
    let (independent_loss_db, independent_sideband_db) = measure_chime_behavior(1.0);
    eprintln!(
        "bass effect on chime: shared {shared_loss_db:+.2} dB/{shared_sideband_db:.1} dBc, independent {independent_loss_db:+.2} dB/{independent_sideband_db:.1} dBc"
    );
    assert!(
        independent_loss_db >= shared_loss_db + 0.4,
        "reserve improved chime retention by only {:.2} dB",
        independent_loss_db - shared_loss_db
    );
}

#[test]
fn tone_controls_point_in_their_named_directions() {
    let sample_count = 48_128;
    let measure_gain = |frequency_hz: f64, parameter_name: &'static str, value: f64| {
        let input = sine(frequency_hz as f32, 0.03, sample_count);
        let output = render_plugin(&[(parameter_name, value)], &input);
        magnitude_at(&output, frequency_hz, sample_count - 23_040, 23_040)
    };
    assert!(
        20.0 * (measure_gain(100.0, "bass", 10.0) / measure_gain(100.0, "bass", 0.0)).log10() > 4.0
    );
    assert!(
        20.0 * (measure_gain(700.0, "mid", 10.0) / measure_gain(700.0, "mid", 0.0)).log10() > 4.0
    );
    assert!(
        20.0 * (measure_gain(5_000.0, "treble", 10.0) / measure_gain(5_000.0, "treble", 0.0))
            .log10()
            > 4.0
    );
}

#[test]
fn gain_behaves_like_an_amplifier_volume_control() {
    let sample_count = 48_128;
    let input = sine(440.0, 0.01, sample_count);
    let gain_down = render_plugin(&[("gain", 0.0)], &input);
    let gain_up = render_plugin(&[("gain", 18.0)], &input);
    let start = sample_count - 23_040;
    let level_change_db = 20.0
        * (magnitude_at(&gain_up, 440.0, start, 23_040)
            / magnitude_at(&gain_down, 440.0, start, 23_040).max(1e-12))
        .log10();
    eprintln!("gain control level change: {level_change_db:+.2} dB");
    assert!(
        level_change_db >= 12.0,
        "full gain range raised a small signal by only {level_change_db:.2} dB"
    );
}

#[test]
fn maximum_treble_has_glassy_high_frequency_extension() {
    let sample_count = 48_128;
    let measure_gain = |frequency_hz: f64| {
        let input = sine(frequency_hz as f32, 0.01, sample_count);
        let output = render_plugin(&[("treble", 10.0)], &input);
        magnitude_at(&output, frequency_hz, sample_count - 23_040, 23_040) / 0.01
    };
    let presence_gain = measure_gain(1_000.0);
    let glass_gain = measure_gain(6_000.0);
    let glass_relative_to_presence_db = 20.0 * (glass_gain / presence_gain.max(1e-12)).log10();
    let air_relative_to_presence_db =
        20.0 * (measure_gain(10_000.0) / presence_gain.max(1e-12)).log10();
    for frequency_hz in [3_000.0, 6_000.0, 8_000.0, 10_000.0, 12_000.0] {
        let relative_db = 20.0 * (measure_gain(frequency_hz) / presence_gain.max(1e-12)).log10();
        eprintln!("maximum-treble {frequency_hz:.0} Hz relative to 1 kHz: {relative_db:+.2} dB");
    }
    assert!(
        glass_relative_to_presence_db >= 6.0,
        "maximum treble produced only {glass_relative_to_presence_db:.2} dB at 6 kHz"
    );
    assert!(
        air_relative_to_presence_db >= 3.0,
        "maximum treble produced only {air_relative_to_presence_db:.2} dB at 10 kHz"
    );
}
