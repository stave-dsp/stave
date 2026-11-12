//! THE RIG LAW — anvil→tolex measured AS THE PAIR. Field-driven
//! (2026-07-26): the first rig read vintage-dark (-17 dB himids;
//! the user's ears called it before the instrument did), and FOCUS
//! darkened because a round low branch starved the chug's own upper
//! harmonics. Now law-pinned to the modern shape. Low-E saw chug
//! proxy through anvil (defaults) then tolex (defaults=BREAKDOWN
//! and INIT), band energies re the thump band.
use rustfft::{num_complex::Complex, FftPlanner};
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};
use std::sync::Arc;

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_SIZE: usize = 256;
const SAMPLE_COUNT: usize = 96_256;

fn process_through_plugin<P: Plugin>(plugin: &mut P, input: &[f32]) -> Vec<f32> {
    let debug_bus = DebugBus::new();
    plugin.debug_attach(debug_bus.clone());
    plugin.activate(SAMPLE_RATE, BLOCK_SIZE);
    let mut rendered_output = Vec::with_capacity(input.len());
    let (mut left_input, mut right_input) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    let (mut left_output, mut right_output) = (vec![0.0f32; BLOCK_SIZE], vec![0.0f32; BLOCK_SIZE]);
    for input_chunk in input.chunks(BLOCK_SIZE) {
        left_input[..input_chunk.len()].copy_from_slice(input_chunk);
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

fn set_parameter_values<P: Plugin>(plugin: &mut P, parameter_values: &[(&str, f64)]) {
    let parameter_table = plugin.param_table();
    for (parameter_name, parameter_value) in parameter_values {
        if let Some(parameter) = parameter_table.by_name(parameter_name) {
            parameter.set_plain(*parameter_value)
        }
    }
}

fn measure_frequency_bands(signal: &[f32]) -> Vec<(String, f64)> {
    const FOURIER_TRANSFORM_SAMPLE_COUNT: usize = 131_072;
    let mut transform_planner = FftPlanner::<f32>::new();
    let forward_transform = transform_planner.plan_fft_forward(FOURIER_TRANSFORM_SAMPLE_COUNT);
    let settled_signal = &signal[signal.len() - FOURIER_TRANSFORM_SAMPLE_COUNT.min(signal.len())..];
    let mut frequency_bins: Vec<Complex<f32>> = settled_signal
        .iter()
        .map(|sample| Complex::new(*sample, 0.0))
        .collect();
    frequency_bins.resize(FOURIER_TRANSFORM_SAMPLE_COUNT, Complex::new(0.0, 0.0));
    forward_transform.process(&mut frequency_bins);
    let bin_width_hz = f64::from(SAMPLE_RATE) / FOURIER_TRANSFORM_SAMPLE_COUNT as f64;
    let measure_band_energy = |minimum_frequency_hz: f64, maximum_frequency_hz: f64| -> f64 {
        let minimum_bin = (minimum_frequency_hz / bin_width_hz) as usize;
        let maximum_bin = (maximum_frequency_hz / bin_width_hz) as usize;
        10.0 * frequency_bins[minimum_bin..maximum_bin]
            .iter()
            .map(|frequency_bin| f64::from(frequency_bin.norm_sqr()))
            .sum::<f64>()
            .max(1e-30)
            .log10()
    };
    let thump_band_energy = measure_band_energy(80.0, 200.0);
    [
        ("thump 80-200", 0.0),
        (
            "lowmid 200-500",
            measure_band_energy(200.0, 500.0) - thump_band_energy,
        ),
        (
            "mid 500-1.2k",
            measure_band_energy(500.0, 1_200.0) - thump_band_energy,
        ),
        (
            "himid 1.2-3k",
            measure_band_energy(1_200.0, 3_000.0) - thump_band_energy,
        ),
        (
            "presence 3-6k",
            measure_band_energy(3_000.0, 6_000.0) - thump_band_energy,
        ),
        (
            "fizz 6-10k",
            measure_band_energy(6_000.0, 10_000.0) - thump_band_energy,
        ),
    ]
    .iter()
    .map(|(band_name, relative_energy_db)| (band_name.to_string(), *relative_energy_db))
    .collect()
}

#[test]
fn rig_balance() {
    // Low-E chug proxy: band-limited saw, -12 dBFS-ish peak.
    let fundamental_frequency_hz = 82.4f32;
    let input: Vec<f32> = (0..SAMPLE_COUNT)
        .map(|sample_index| {
            let time_seconds = sample_index as f32 / SAMPLE_RATE;
            let mut sample = 0.0f32;
            for harmonic_number in 1..=40 {
                let harmonic_frequency_hz = fundamental_frequency_hz * harmonic_number as f32;
                if harmonic_frequency_hz < 18_000.0 {
                    sample += (core::f32::consts::TAU * harmonic_frequency_hz * time_seconds).sin()
                        / harmonic_number as f32;
                }
            }
            sample * 0.18
        })
        .collect();

    let mut anvil_plugin = stave_anvil_vst3::Anvil::new();
    let amplifier_output = process_through_plugin(&mut anvil_plugin, &input);
    eprintln!("== ANVIL alone (defaults) ==");
    for (band_name, relative_energy_db) in measure_frequency_bands(&amplifier_output) {
        eprintln!("  {band_name:<16} {relative_energy_db:+6.1} dB");
    }
    for topology_separation_amount in [0.0f64, 1.0] {
        let mut comparison_anvil_plugin = stave_anvil_vst3::Anvil::new();
        set_parameter_values(
            &mut comparison_anvil_plugin,
            &[("focus", topology_separation_amount)],
        );
        let comparison_output = process_through_plugin(&mut comparison_anvil_plugin, &input);
        eprintln!("== ANVIL focus {topology_separation_amount} ==");
        for (band_name, relative_energy_db) in measure_frequency_bands(&comparison_output) {
            eprintln!("  {band_name:<16} {relative_energy_db:+6.1} dB");
        }
    }
    for cabinet_preset_name in ["BREAKDOWN(default)", "INIT"] {
        let mut tolex_plugin = stave_tolex_vst3::Tolex::new();
        if cabinet_preset_name == "INIT" {
            set_parameter_values(
                &mut tolex_plugin,
                &[
                    ("resonance", 80.0),
                    ("q", 0.85),
                    ("mass", 1.0),
                    ("cone", 0.0),
                    ("breakup", 0.6),
                    ("doping", 0.35),
                    ("drive", 0.0),
                    ("damping", 0.75),
                    ("back", 0.0),
                    ("cab", 60.0),
                    ("material", 0.0),
                    ("speakers", 0.0),
                    ("model", 0.0),
                    ("mic", 3.0),
                    ("position", 0.25),
                ],
            );
        }
        let complete_rig_output = process_through_plugin(&mut tolex_plugin, &amplifier_output);
        eprintln!("== RIG anvil -> tolex {cabinet_preset_name} ==");
        for (band_name, relative_energy_db) in measure_frequency_bands(&complete_rig_output) {
            eprintln!("  {band_name:<16} {relative_energy_db:+6.1} dB");
        }
    }
    eprintln!(
        "(modern metalcore reference, post-rig: himid/presence within ~0..-8 dB of thump; vintage-dark shows -15..-25)"
    );

    // THE LAW: through the default rig (anvil defaults -> tolex
    // BREAKDOWN), the modern shape holds. Brackets re-baselined
    // 2026-07-27 after the field re-voice (thinner/darker vs the
    // Plini head-to-head): the quadratic tight trim raised THUMP
    // itself, so every thump-relative band sits lower; the head's
    // top opened (deemph -3, ceilings 10.4k/9.5k). Measured chain:
    // -7.1 / -13.3 / -11.6 / -1.2 / -15.9. PROVISIONAL until the
    // pair is ear-ratified — fizz especially (a head this open may
    // need the pair pulled back; tighten these when the ear rules).
    let mut law_anvil_plugin = stave_anvil_vst3::Anvil::new();
    let law_amplifier_output = process_through_plugin(&mut law_anvil_plugin, &input);
    let mut law_tolex_plugin = stave_tolex_vst3::Tolex::new();
    let law_rig_output = process_through_plugin(&mut law_tolex_plugin, &law_amplifier_output);
    let measured_bands = measure_frequency_bands(&law_rig_output);
    let find_band_energy = |band_name_prefix: &str| {
        measured_bands
            .iter()
            .find(|(band_name, _)| band_name.starts_with(band_name_prefix))
            .unwrap()
            .1
    };
    assert!(
        (-9.5..=4.0).contains(&find_band_energy("lowmid")),
        "lowmid {:.1} — body wrong",
        find_band_energy("lowmid")
    );
    // Mid floor -18 (re-baselined 2026-08-05): FUZZBED is the
    // ear-ratified voice — the girth branches' bias-choke density
    // deepens the scoop ~1 dB. The ear ruled; the bracket follows.
    assert!(
        (-18.0..=-4.0).contains(&find_band_energy("mid")),
        "mid {:.1} — the scoop is the shape",
        find_band_energy("mid")
    );
    assert!(
        find_band_energy("himid") >= -14.0,
        "himid {:.1} — vintage-dark regression",
        find_band_energy("himid")
    );
    assert!(
        find_band_energy("presence") >= -8.0,
        "presence {:.1} — the bite is gone",
        find_band_energy("presence")
    );
    assert!(
        find_band_energy("fizz") <= -12.0,
        "fizz {:.1} — smooth broke",
        find_band_energy("fizz")
    );
}
