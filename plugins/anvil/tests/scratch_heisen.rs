//! HEISEN-REPRO: does the probe-emission build zero its input
//! IN-PROCESS, or only across the bench dlopen boundary?
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};
use std::sync::Arc;
#[test]
fn in_process_input_aliveness() {
    let sample_rate = 48_000.0f32;
    let block_size = 256usize;
    let mut plugin = stave_anvil_vst3::Anvil::new();
    let debug_bus = DebugBus::new();
    plugin.debug_attach(debug_bus.clone());
    plugin.activate(sample_rate, block_size);
    let (mut left_input, mut right_input) = (vec![0.0f32; block_size], vec![0.0f32; block_size]);
    let (mut left_output, mut right_output) = (vec![0.0f32; block_size], vec![0.0f32; block_size]);
    let mut output_peak = 0.0f32;
    for block_index in 0..100 {
        for sample_index in 0..block_size {
            let time_seconds = (block_index * block_size + sample_index) as f32 / sample_rate;
            left_input[sample_index] = 0.25 * (core::f32::consts::TAU * 220.0 * time_seconds).sin();
        }
        right_input.copy_from_slice(&left_input);
        let mut input_channel_pointers = [core::ptr::null_mut(); 2];
        let mut output_channel_pointers = [core::ptr::null_mut(); 2];
        let input_buffer = AudioBuffer::from_slices(
            &mut input_channel_pointers,
            [&mut left_input, &mut right_input],
            block_size,
        );
        let output_buffer = AudioBuffer::from_slices(
            &mut output_channel_pointers,
            [&mut left_output, &mut right_output],
            block_size,
        );
        let mut process_context = ProcessCtx {
            sample_rate,
            frames: block_size,
            transport: None,
            input: input_buffer,
            output: output_buffer,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut process_context);
        output_peak = output_peak.max(
            left_output
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs())),
        );
    }
    // read the probes back from the bus
    let used_probe_count = debug_bus
        .probes_used
        .load(std::sync::atomic::Ordering::Acquire) as usize;
    for probe_index in 0..used_probe_count {
        let probe = &debug_bus.probes[probe_index];
        eprintln!(
            "probe {} = {:.2}",
            probe.name_str(),
            f64::from_bits(probe.latest.load(std::sync::atomic::Ordering::Relaxed))
        );
    }
    eprintln!("output peak {:.2} dB", 20.0 * output_peak.max(1e-9).log10());
    assert!(output_peak > 0.05, "in-process output dead: {output_peak}");
    let _ = Arc::strong_count(&debug_bus);
}
