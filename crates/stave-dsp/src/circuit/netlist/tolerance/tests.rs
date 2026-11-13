use super::*;
use crate::circuit::netlist::Taper;

fn moments(t: Tolerance, n: u64) -> (f64, f64, f64) {
    let draws: Vec<f64> = (0..n).map(|u| t.factor(42, u, "r1") - 1.0).collect();
    let mean = draws.iter().sum::<f64>() / n as f64;
    let var = draws.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let worst = draws.iter().fold(0.0f64, |m, d| m.max(d.abs()));
    (mean, var.sqrt(), worst)
}

#[test]
fn draws_have_their_laws_moments() {
    let n = 100_000;
    for t in [Tolerance::uniform(0.05), Tolerance::gaussian(0.10)] {
        let (mean, sd, worst) = moments(t, n);
        // Standard errors: the mean's σ/√n, the deviation's about σ/√(2n).
        let sigma = t.sigma();
        assert!(mean.abs() < 4.0 * sigma / (n as f64).sqrt(), "{t:?}: mean {mean:e}");
        assert!((sd / sigma - 1.0).abs() < 4.0 / (2.0 * n as f64).sqrt(), "{t:?}: σ {sd} vs {sigma}");
        assert!(worst <= t.fraction, "{t:?}: {worst} outside the band");
        assert!(worst > 0.95 * t.fraction, "{t:?}: the band is reached ({worst})");
    }
    assert_eq!(Tolerance::EXACT.factor(1, 2, "r1"), 1.0);
}

#[test]
fn a_draw_depends_on_seed_unit_and_name_only() {
    let t = Tolerance::uniform(0.1);
    assert_eq!(t.factor(1, 2, "R1"), t.factor(1, 2, "r1"), "names are case-insensitive");
    assert_ne!(t.factor(1, 2, "r1"), t.factor(1, 2, "r2"));
    assert_ne!(t.factor(1, 2, "r1"), t.factor(1, 3, "r1"));
    assert_ne!(t.factor(1, 2, "r1"), t.factor(2, 2, "r1"));
    // A part added to the netlist leaves the others' draws alone.
    let grades = Tolerances::new().resistors(t);
    let a = Netlist::parse_spice("* a\nV1 in 0 1\nR1 in out 1k\nR2 out 0 1k").unwrap();
    let b = Netlist::parse_spice("* b\nV1 in 0 1\nR0 in x 5k\nR1 x out 1k\nR2 out 0 1k").unwrap();
    let (ua, ub) = (grades.unit(&a, 9, 4).unwrap(), grades.unit(&b, 9, 4).unwrap());
    assert_eq!(ua.value("r1"), ub.value("r1"));
    assert_eq!(ua.value("r2"), ub.value("r2"));
}

#[test]
fn a_pots_track_is_one_part() {
    let mut n = Netlist::parse_spice("* pot\nV1 in 0 1\nRA in w 75k\nRB w 0 25k\nC1 w 0 1n").unwrap();
    n.add_variable(Variable::pot("volume", ["rb", "ra"], Taper::Linear)).unwrap();
    let grades = Tolerances::new().pots(Tolerance::uniform(0.2)).capacitors(Tolerance::uniform(0.1));
    let parts = grades.parts(&n);
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["volume", "c1"]);
    assert_eq!(parts[0].nominal, 100e3);
    let u = grades.unit(&n, 3, 0).unwrap();
    let (ra, rb) = (u.value("ra").unwrap(), u.value("rb").unwrap());
    assert!((rb / (ra + rb) - 0.25).abs() < 1e-15, "the wiper stays put");
    assert!((ra + rb - 100e3).abs() > 1.0, "the track moved");
}

#[test]
fn overrides_name_parts_and_values_stay_valid() {
    let n = Netlist::parse_spice("* rc\nV1 in 0 1\nR1 in out 1k\nC1 out 0 1u").unwrap();
    let exact_r = Tolerances::new()
        .resistors(Tolerance::uniform(0.5))
        .part("R1", Tolerance::EXACT);
    assert_eq!(exact_r.unit(&n, 1, 1).unwrap().value("r1"), Some(1e3));
    let wrong = Tolerances::new().part("r9", Tolerance::uniform(0.1));
    assert!(matches!(wrong.unit(&n, 1, 1), Err(NetlistError::UnknownElement { .. })));
    let mut m = n.clone();
    assert!(m.set_value("r1", -1.0).is_err());
    assert!(m.set_value("v1", 1.0).is_err());
    m.set_value("c1", 2e-6).unwrap();
    assert_eq!(m.value("c1"), Some(2e-6));
}
