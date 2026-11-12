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
    for (i, m) in CATALOG.iter().enumerate() {
        assert!(CATALOG[..i].iter().all(|o| o.name != m.name), "{}", m.name);
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
