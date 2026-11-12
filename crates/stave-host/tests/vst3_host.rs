//! The VST3 host end to end, on a real stave plugin (Tilt) linked
//! in-process: load sequence, layout, params, processing, state.

use stave_host::param::find;
use stave_host::instance::Instance;
use stave_host::note::NoteSchedule;
use std::num::NonZeroUsize;

use stave_host::config::{HostConfig, SampleRate};
use stave_host::plugin::{ControllerKind, LoadedPlugin};
use stave_host::drive::Driver;
use stave_measure::signal::{db_to_amp, Silence, Sine};
use stave_measure::tone;

const RATE: f64 = 48_000.0;
const BLOCK: usize = 128;

fn tilt() -> LoadedPlugin {
    LoadedPlugin::from_entry_points(
        stave_host::entry_points!(stave_tilt_vst3),
        HostConfig {
            sample_rate: SampleRate::new(RATE).unwrap(),
            max_block: NonZeroUsize::new(BLOCK).unwrap(),
            ..HostConfig::default()
        },
    )
    .expect("tilt loads")
}

#[test]
fn loads_the_audio_module_and_traces_every_step() {
    let p = tilt();
    let info = p.info();
    assert_eq!(info.class.name, "Stave Tilt");
    assert_eq!(info.class.category, stave_host::plugin::AUDIO_MODULE_CLASS);
    assert_ne!(info.controller, ControllerKind::None);
    assert!(info.stave_telemetry);
    let steps: Vec<&str> = info.load.steps().iter().map(|s| s.step).collect();
    for want in [
        "module",
        "factory",
        "class",
        "initialize component",
        "activate",
    ] {
        assert!(steps.contains(&want), "missing {want} in {steps:?}");
    }
    assert!(
        info.load.steps().iter().all(|s| s.ok),
        "{:?}",
        info.load.steps()
    );
}

#[test]
fn stereo_effect_layout() {
    let l = tilt().layout();
    assert_eq!(
        (l.inputs, l.outputs, l.sample_rate, l.max_block),
        (2, 2, RATE, BLOCK)
    );
    assert!(!l.is_instrument());
}

#[test]
fn params_carry_ranges_and_flags() {
    let params = tilt().params();
    let t = find(&params, "tilt").expect("tilt param");
    assert_eq!((t.min, t.max, t.default), (-6.0, 6.0, 0.0));
    let b = find(&params, "bypass").expect("bypass param");
    assert!(b.flags.bypass, "{b:?}");
}

#[test]
fn output_trim_is_heard_by_the_processor() {
    let mut p = tilt();
    p.set_param("output", -6.0).unwrap();
    let mut d = Driver::new(&mut p);
    let n = 16_384;
    let s = Sine {
        frequency_hz: 1_000.0,
        amplitude: db_to_amp(-12.0),
        sample_rate: RATE,
    }
    .bin_snapped(n);
    d.skip(d.frames_for(0.5), &s, &NoteSchedule::default()); // smoothing settles
    let cap = d.capture(n as u64, &s, &NoteSchedule::default());
    let r = tone::analyze(
        cap.channel(0),
        &tone::ToneAnalysis {
            input: Sine {
                frequency_hz: s.frequency_hz,
                amplitude: s.amplitude,
                sample_rate: RATE,
            },
            max_harmonic_hz: 20_000.0,
        },
    );
    assert!((r.gain_db + 6.0).abs() < 0.05, "{r:?}");
}

#[test]
fn unknown_param_is_an_error() {
    assert!(tilt().set_param("no such knob", 1.0).is_err());
}

#[test]
fn blocks_larger_than_max_block_are_chunked() {
    let mut p = tilt();
    let input = vec![0.25f32; BLOCK * 3 + 5];
    let mut l = vec![0.0f32; input.len()];
    let mut r = vec![0.0f32; input.len()];
    for _ in 0..200 {
        p.process(&[&input, &input], &mut [&mut l, &mut r], &[]);
    }
    assert!(
        (l[BLOCK * 3 + 4] - 0.25).abs() < 1e-3,
        "flat tilt passes DC-ish input"
    );
    assert!(p.stats().blocks >= 800);
}

#[test]
fn state_round_trips_into_a_fresh_instance() {
    let mut a = tilt();
    a.set_param("tilt", 3.5).unwrap();
    a.set_param("output", -2.0).unwrap();
    Driver::new(&mut a).skip(BLOCK as u64, &Silence, &NoteSchedule::default());
    let blob = a.save_state().unwrap();
    let mut b = tilt();
    b.load_state(&blob).unwrap();
    assert_eq!(b.save_state().unwrap(), blob);
    let tilt_b = find(&b.params(), "tilt").unwrap().value;
    assert!(
        (tilt_b - 3.5).abs() < 1e-9,
        "controller follows the restored state: {tilt_b}"
    );
}

#[test]
fn a_refused_state_load_says_why_in_the_plugin_messages() {
    let mut p = tilt();
    let from = p.messages(0).last().map_or(0, |m| m.index + 1);
    assert!(p.load_state(b"STV").is_err());
    let messages = p.messages(from);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(
        messages[0].severity,
        stave_core::diagnostics::Severity::Error
    );
    assert_eq!(
        messages[0].text,
        "setState: state not loaded: chunk shorter than the fixed header"
    );
}
