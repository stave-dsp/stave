use super::*;
use crate::adapt::InstanceChannel;
use crate::config::HostConfig;
use crate::control::ControlError;
use crate::instance::{Instance, Probe};
use crate::validate::{Status, Suite};
use stave_measure::tone_test::ToneTest;

fn config() -> HostConfig {
    HostConfig {
        max_block: std::num::NonZeroUsize::new(64).unwrap(),
        ..HostConfig::default()
    }
}

fn tone() -> ToneTest {
    ToneTest {
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
    let mut peak = ModuleInstance::new(find("filter/peak").unwrap(), &config());
    let names: Vec<_> = peak.controls().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["hz", "db", "q"]);
    let gain = |inst: &mut ModuleInstance| {
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
    let mut m = ModuleInstance::new(find("coupler").unwrap(), &config());
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
    let mut clipper = ModuleInstance::new(find("clipper/diode").unwrap(), &config());
    let input = vec![0.8f32; 64];
    let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
    clipper.process(&[&input, &input], &mut [&mut l, &mut r], &[]);
    let probes = clipper.probes();
    let read = |n: &str| probes.iter().find(|p| p.name == n).unwrap().latest;
    assert_eq!(read(Probe::NEWTON_SOLVES), 128.0);
    assert!((1.0..=3.0).contains(&read(Probe::NEWTON_MEAN_ITERATIONS)));
    assert_eq!(read(Probe::NEWTON_CAPPED), 0.0);
    assert!(find("filter/peak")
        .map(|m| ModuleInstance::new(m, &config()).probes().is_empty())
        .unwrap());
}

#[test]
fn every_module_passes_validation() {
    for module in CATALOG {
        let mut fresh =
            || Ok(Box::new(ModuleInstance::new(module, &config())) as Box<dyn Instance>);
        let report = Suite::default().run(&mut fresh, |_| {});
        let failed: Vec<_> = report
            .checks
            .iter()
            .filter(|c| c.status == Status::Fail)
            .collect();
        assert!(failed.is_empty(), "{}: {failed:#?}", module.name);
    }
}