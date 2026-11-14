use super::*;
use crate::circuit::netlist::{Taper, Variable};
use crate::units::seconds;

fn rc() -> Netlist {
    Netlist::parse_spice("* rc\nR1 in out 1k\nC1 out 0 100n").unwrap()
}

#[test]
fn reference_ratio_and_elapsed_composition_match_the_analytic_rc_pole() {
    let base = rc();
    let profile = Ageing::new()
        .part("R1", Drift::new(1.2, seconds(100.0)).unwrap())
        .part("c1", Drift::new(0.8, seconds(100.0)).unwrap());
    assert_eq!(profile.unit(&base, seconds(0.0), 1, 2).unwrap(), base);
    let whole = profile.unit(&base, seconds(100.0), 1, 2).unwrap();
    let half = profile.unit(&base, seconds(50.0), 1, 2).unwrap();
    let twice = profile.unit(&half, seconds(50.0), 1, 2).unwrap();
    for (part, ratio) in [("r1", 1.2), ("c1", 0.8)] {
        assert!((whole.value(part).unwrap() / base.value(part).unwrap() - ratio).abs() < 1e-14);
        assert!((twice.value(part).unwrap() / whole.value(part).unwrap() - 1.0).abs() < 1e-14);
    }
    let pole = |n: &Netlist| {
        1.0 / (std::f64::consts::TAU * n.value("r1").unwrap() * n.value("c1").unwrap())
    };
    assert!((pole(&whole) / pole(&base) - 1.0 / 0.96).abs() < 1e-14);
}

#[test]
fn seeded_drift_is_stable_across_age_order_and_added_parts() {
    let drift = Drift::new(0.8, seconds(10.0))
        .unwrap()
        .spread(Tolerance::uniform(0.2))
        .unwrap();
    let profile = Ageing::new().part("c1", drift);
    let a = profile.unit(&rc(), seconds(10.0), 5, 6).unwrap();
    assert_eq!(a, profile.unit(&rc(), seconds(10.0), 5, 6).unwrap());
    assert_ne!(a, profile.unit(&rc(), seconds(10.0), 5, 7).unwrap());
    let extra = Netlist::parse_spice("* rc\nR0 x 0 1k\nC1 out 0 100n\nR1 in out 1k").unwrap();
    let b = profile.unit(&extra, seconds(10.0), 5, 6).unwrap();
    assert_eq!(a.value("c1"), b.value("c1"));
    let twice = profile.unit(&rc(), seconds(20.0), 5, 6).unwrap();
    let ratio = a.value("c1").unwrap() / 100e-9;
    assert!((twice.value("c1").unwrap() / 100e-9 - ratio * ratio).abs() < 1e-14);
    assert_eq!(a.value("r1"), rc().value("r1"));
}

#[test]
fn pot_track_scales_together_and_tolerance_baseline_is_preserved() {
    let mut n = Netlist::parse_spice("* pot\nRA in w 75k\nRB w 0 25k").unwrap();
    n.add_variable(Variable::pot("volume", ["rb", "ra"], Taper::Linear))
        .unwrap();
    let built = Tolerances::new()
        .pots(Tolerance::uniform(0.1))
        .unit(&n, 1, 2)
        .unwrap();
    let profile = Ageing::new().part("VOLUME", Drift::new(1.1, seconds(1.0)).unwrap());
    let aged = profile.unit(&built, seconds(1.0), 1, 2).unwrap();
    assert!((aged.value("ra").unwrap() / aged.value("rb").unwrap() - 3.0).abs() < 1e-14);
    assert!((aged.value("ra").unwrap() / built.value("ra").unwrap() - 1.1).abs() < 1e-14);
    assert!(Ageing::new()
        .part("ra", Drift::new(1.1, seconds(1.0)).unwrap())
        .unit(&built, seconds(1.0), 1, 2)
        .is_err());
}

#[test]
fn invalid_inputs_and_unrepresentable_values_are_errors_without_mutation() {
    for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Drift::new(ratio, seconds(1.0)).is_err());
    }
    for duration in [0.0, -1.0, f64::INFINITY] {
        assert!(Drift::new(1.1, seconds(duration)).is_err());
    }
    for fraction in [-1.0, 1.0, f64::NAN, f64::INFINITY] {
        assert!(Drift::new(1.1, seconds(1.0))
            .unwrap()
            .spread(Tolerance::uniform(fraction))
            .is_err());
    }
    let base = rc();
    let original = base.clone();
    for ratio in [0.5, 2.0] {
        let p = Ageing::new().part("r1", Drift::new(ratio, seconds(1.0)).unwrap());
        assert!(p.unit(&base, seconds(1e6), 1, 2).is_err());
        assert!(p.unit(&base, seconds(-1.0), 1, 2).is_err());
        assert!(p.unit(&base, seconds(f64::INFINITY), 1, 2).is_err());
        assert_eq!(base, original);
    }
    let unknown = Ageing::new().part("r9", Drift::new(1.0, seconds(1.0)).unwrap());
    assert!(matches!(
        unknown.unit(&base, seconds(0.0), 1, 2),
        Err(NetlistError::UnknownElement { .. })
    ));
    assert_eq!(
        Ageing::new().unit(&base, seconds(f64::MAX), 1, 2).unwrap(),
        base
    );
}
