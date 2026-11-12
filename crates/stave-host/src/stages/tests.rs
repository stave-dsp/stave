use super::*;
use crate::adapt::InstanceChannel;
use crate::config::HostConfig;
use crate::control::ControlError;
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
fn names_are_unique_and_defaults_in_range() {
    for (i, m) in CATALOG.iter().enumerate() {
        assert!(CATALOG[..i].iter().all(|o| o.name != m.name), "{}", m.name);
        for s in m.settings {
            assert!(s.range.admits(s.default), "{}.{}", m.name, s.name);
        }
    }
}

#[test]
fn settings_are_controls_and_a_change_reaches_the_audio() {
    let mut peak = CatalogInstance::new(find("filter/peak").unwrap(), &config());
    let names: Vec<_> = peak.controls().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["hz", "db", "q"]);
    let gain = |inst: &mut CatalogInstance| {
        tone().gain_db(InstanceChannel {
            instance: inst,
            channel: 0,
        })
    };
    assert!((gain(&mut peak) - 6.0).abs() < 0.1);
    peak.set_control("db", -12.0).unwrap();
    assert!((gain(&mut peak) + 12.0).abs() < 0.1);
}

#[test]
fn out_of_range_and_unknown_settings_are_refused() {
    let mut m = CatalogInstance::new(find("coupler").unwrap(), &config());
    assert!(matches!(
        m.set_control("hz", 1e6),
        Err(ControlError::OutOfRange { .. })
    ));
    assert!(matches!(
        m.set_control("nope", 1.0),
        Err(ControlError::NotFound(_))
    ));
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

#[test]
fn every_module_passes_validation() {
    for stage in CATALOG {
        let mut fresh =
            || Ok(Box::new(CatalogInstance::new(stage, &config())) as Box<dyn Instance>);
        let report = Suite::default().run(&mut fresh, |_| {});
        let failed: Vec<_> = report
            .checks
            .iter()
            .filter(|c| c.status == Status::Fail)
            .collect();
        assert!(failed.is_empty(), "{}: {failed:#?}", stage.name);
    }
}