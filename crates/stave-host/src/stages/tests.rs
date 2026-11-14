use super::*;
use crate::adapt::InstanceChannel;
use crate::config::HostConfig;
use crate::param::ParamError;
use crate::instance::{Instance, TapReading};
use crate::validate::{Status, Suite};
use stave_measure::tone_window::ToneWindow;

fn config() -> HostConfig {
    HostConfig {
        max_block: std::num::NonZeroUsize::new(64).unwrap(),
        ..HostConfig::default()
    }
}

fn tone() -> ToneWindow {
    ToneWindow {
        bin: 100,
        window: 4800,
        sample_rate: 48_000.0,
        amplitude: 0.25,
        warmup_windows: 4,
    }
}

#[test]
fn names_are_unique_and_params_valid() {
    use stave_core::params::Params;
    let all: Vec<&StageSpec> = CATALOG.iter().chain(REFERENCES).collect();
    for (i, m) in all.iter().enumerate() {
        assert!(all[..i].iter().all(|o| o.name != m.name), "{}", m.name);
        let params = (m.params)();
        assert_eq!(params.validate(), Ok(()), "{}", m.name);
        let mut names: Vec<_> = (0..params.count())
            .filter_map(|i| params.by_index(i))
            .map(|p| p.meta().name)
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), params.count(), "{}: duplicate names", m.name);
    }
}

#[test]
fn params_are_listed_and_a_change_reaches_the_audio() {
    let mut peak = CatalogInstance::new(find("filter/peak").unwrap(), &config());
    let names: Vec<_> = peak.params().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["hz", "db", "q"]);
    let gain = |inst: &mut CatalogInstance| {
        tone().gain_db(InstanceChannel {
            instance: inst,
            channel: 0,
        })
    };
    assert!((gain(&mut peak) - 6.0).abs() < 0.1);
    peak.set_param("db", -12.0).unwrap();
    assert!((gain(&mut peak) + 12.0).abs() < 0.1);
}

#[test]
fn out_of_range_and_unknown_params_are_refused() {
    let mut m = CatalogInstance::new(find("coupler").unwrap(), &config());
    assert!(matches!(
        m.set_param("hz", 1e6),
        Err(ParamError::OutOfRange { .. })
    ));
    assert!(matches!(
        m.set_param("nope", 1.0),
        Err(ParamError::NotFound(_))
    ));
    let mut tube = CatalogInstance::new(find("circuit/common_cathode").unwrap(), &config());
    assert!(matches!(
        tube.set_param("tube", 0.5),
        Err(ParamError::NotAnOption { .. })
    ));
    assert_eq!(tube.set_param("tube", 1.0), Ok(()));
    assert_eq!(tube.handle().get("tube"), Some(1.0));
}

#[test]
fn display_uses_the_params_unit() {
    let peak = CatalogInstance::new(find("filter/peak").unwrap(), &config());
    let h = peak.handle();
    assert_eq!(h.display("db", 1.0).as_deref(), Some("24.0 dB"));
    let tube = CatalogInstance::new(find("circuit/common_cathode").unwrap(), &config());
    let source = tube.handle().display("source", 0.0);
    assert_eq!(source.as_deref(), Some("0.00 kΩ"));
}

#[test]
fn the_diode_clipper_reports_its_solver() {
    let mut clipper = CatalogInstance::new(find("clipper/diode").unwrap(), &config());
    let input = vec![0.8f32; 64];
    let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
    clipper.process(&[&input, &input], &mut [&mut l, &mut r], &[]);
    let taps = clipper.taps();
    let read = |n: &str| taps.iter().find(|p| p.name == n).unwrap().latest;
    assert_eq!(read(TapReading::NEWTON_SOLVES), 128.0);
    assert!((1.0..=3.0).contains(&read(TapReading::NEWTON_MEAN_ITERATIONS)));
    assert_eq!(read(TapReading::NEWTON_CAPPED), 0.0);
    assert!(find("filter/peak")
        .map(|m| CatalogInstance::new(m, &config()).taps().is_empty())
        .unwrap());
}

/// Every module through the whole suite. The checks run a thread per
/// module (the modules are independent, and the slowest alone sets the
/// time); the one timing check, `deadline_headroom`, runs afterwards, one
/// module at a time, so it never measures its neighbours' load.
#[test]
fn every_module_passes_validation() {
    const TIMED: &str = "deadline_headroom";
    let suite = Suite::default();
    let failures = |stage: &'static StageSpec, timed: bool| -> Vec<String> {
        let mut fresh =
            || Ok(Box::new(CatalogInstance::new(stage, &config())) as Box<dyn Instance>);
        suite
            .checks
            .iter()
            .filter(|check| (check.name == TIMED) == timed)
            .map(|check| suite.run_check(&mut fresh, check))
            .filter(|c| c.status == Status::Fail)
            .map(|c| format!("{}: {c:#?}", stage.name))
            .collect()
    };
    let mut failed: Vec<String> = std::thread::scope(|s| {
        let runs: Vec<_> = CATALOG
            .iter()
            .map(|stage| s.spawn(move || failures(stage, false)))
            .collect();
        runs.into_iter()
            .flat_map(|run| run.join().expect("a validation run panicked"))
            .collect()
    });
    for stage in CATALOG {
        failed.extend(failures(stage, true));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every reference model opens by name, runs a hot tone at every
/// oversampling factor to a finite output, and reports its solver.
#[test]
fn every_reference_runs() {
    let input: Vec<f32> = (0..4_800).map(|i| 0.3 * (std::f32::consts::TAU * 220.0 * i as f32 / 48_000.0).sin()).collect();
    for spec in REFERENCES {
        let opened = crate::opened::Opened::open(std::path::Path::new(&format!("{PREFIX}{}", spec.name)), config()).unwrap();
        let mut inst = opened.into_instance();
        let oversampled = inst.set_param("oversample", 0.0).is_ok();
        for factor in 0..if oversampled { 4 } else { 1 } {
            if oversampled {
                inst.set_param("oversample", f64::from(factor)).unwrap();
            }
            let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
            let mut peak = 0.0f32;
            for chunk in input.chunks(64).take(if factor < 2 { 75 } else { 10 }) {
                inst.process(&[chunk, chunk], &mut [&mut l, &mut r], &[]);
                assert!(l.iter().all(|v| v.is_finite()), "{} at factor {factor}", spec.name);
                peak = l.iter().fold(peak, |p, v| p.max(v.abs()));
            }
            assert!(peak > 1e-4, "{} at factor {factor}: silent", spec.name);
        }
    }
}

/// Every pedal knob, op-amp, gate and glide setting reaches the audio:
/// the same tone through each extreme differs.
#[test]
fn pedal_settings_reach_the_audio() {
    let input: Vec<f32> = (0..4_800).map(|i| 0.2 * (std::f32::consts::TAU * 220.0 * i as f32 / 48_000.0).sin()).collect();
    let run = |name: &str, setting: &str, value: f64| -> Vec<f32> {
        let opened = crate::opened::Opened::open(std::path::Path::new(&format!("{PREFIX}{name}")), config()).unwrap();
        let mut inst = opened.into_instance();
        inst.set_param(setting, value).unwrap();
        let mut out = Vec::new();
        let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        for chunk in input.chunks(64) {
            inst.process(&[chunk, chunk], &mut [&mut l, &mut r], &[]);
            out.extend_from_slice(&l);
        }
        out
    };
    for (name, setting, a, b) in [
        ("circuit/ts808", "drive", 0.0, 1.0),
        ("circuit/ts808", "tone", 0.0, 1.0),
        ("circuit/ts808_lfo", "glide_ms", 0.0, 50.0),
        ("circuit/klon_centaur", "gain", 0.0, 1.0),
        ("circuit/klon_centaur", "treble", 0.0, 1.0),
        ("circuit/mxr_distortion_plus", "distortion", 0.0, 1.0),
        ("circuit/mxr_distortion_plus", "output", 0.2, 1.0),
        ("circuit/boss_ds1", "distortion", 0.1, 1.0),
        ("circuit/boss_ds1", "tone", 0.0, 1.0),
        ("circuit/proco_rat", "distortion", 0.0, 1.0),
        ("circuit/proco_rat", "filter", 0.0, 1.0),
        ("circuit/big_muff", "tone", 0.0, 1.0),
        ("circuit/fuzz_face", "fuzz", 0.1, 1.0),
        ("circuit/phase90", "mode", 0.0, 1.0),
        ("circuit/common_cathode", "tube", 0.0, 8.0),
    ] {
        let (x, y) = (run(name, setting, a), run(name, setting, b));
        let tail = x.len() / 2;
        let diff: f64 = x[tail..].iter().zip(&y[tail..]).map(|(p, q)| f64::from(p - q).powi(2)).sum();
        let power: f64 = x[tail..].iter().map(|p| f64::from(*p).powi(2)).sum();
        assert!(x.iter().chain(&y).all(|v| v.is_finite()), "{name} {setting}");
        assert!(power > 0.0 && diff > 1e-4 * power, "{name}: {setting} {a} and {b} sound alike ({diff} of {power})");
    }
    // The fitted and the table JRC4558 differ in slew and swing alone,
    // so at 200 mV and 220 Hz only just (measured 3.5e-8 of the power).
    let (x, y) = (run("circuit/ts808", "opamp", 0.0), run("circuit/ts808", "opamp", 1.0));
    let diff: f64 = x.iter().zip(&y).map(|(p, q)| f64::from(p - q).powi(2)).sum();
    assert!(diff > 0.0, "the op-amp choice reaches the audio");
}

/// Every oversampled reference reports its oversampler's latency as
/// soon as the factor is set (before any `process`, as stave-play reads
/// it), and its output lands there: an impulse's peak moves from the 1×
/// position by the reported latency, within a sample or two (the
/// circuit's own response warps a little between rates).
#[test]
fn oversampled_references_report_their_latency() {
    use stave_core::params::Params;
    let peak_at = |name: &str, factor: f64| -> (u32, usize) {
        let opened = crate::opened::Opened::open(std::path::Path::new(&format!("{PREFIX}{name}")), config()).unwrap();
        let mut inst = opened.into_instance();
        inst.set_param("oversample", factor).unwrap();
        let latency = inst.latency();
        let mut input = vec![0.0f32; 4_096];
        input[64] = 0.05;
        let mut out = Vec::new();
        let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        for chunk in input.chunks(64) {
            inst.process(&[chunk, chunk], &mut [&mut l, &mut r], &[]);
            out.extend_from_slice(&l);
        }
        // Past the step a self-biasing circuit settles from, the
        // impulse's largest departure from the sample before it.
        let peak = (64..out.len())
            .max_by(|&a, &b| (out[a] - out[63]).abs().total_cmp(&(out[b] - out[63]).abs()))
            .unwrap();
        (latency, peak)
    };
    for spec in REFERENCES {
        let params = (spec.params)();
        let oversampled = (0..params.count())
            .filter_map(|i| params.by_index(i))
            .any(|p| p.meta().name == "oversample");
        if !oversampled || spec.name == "circuit/phase90" || spec.name == "circuit/ts808_lfo" {
            continue;
        }
        let (base_latency, base) = peak_at(spec.name, 0.0);
        assert_eq!(base_latency, 0, "{} at 1×", spec.name);
        for factor in 1..4 {
            let (latency, peak) = peak_at(spec.name, f64::from(factor));
            assert!(latency > 0, "{} at factor {factor}: reports no latency", spec.name);
            let moved = peak as i64 - base as i64;
            assert!(
                (moved - i64::from(latency)).abs() <= 2,
                "{} at factor {factor}: peak moved {moved}, reported {latency}",
                spec.name
            );
        }
    }
}
