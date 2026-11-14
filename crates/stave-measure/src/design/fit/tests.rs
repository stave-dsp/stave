use std::f64::consts::TAU;

use stave_dsp::circuit::netlist::tolerance::Tolerances;
use stave_dsp::circuit::netlist::Netlist;

use super::*;

type Tick = Ticked<Box<dyn FnMut(f64) -> f64>>;
use crate::design::{Design, Parameter, Parameters, Parts, Subject, Ticked};

/// An RC low-pass into a fixed 10k load: R1 sets the DC loss, C1 the
/// corner, so both are determined by the response.
fn rc(r: &str, c: &str) -> Netlist {
    Netlist::parse_spice(&format!("* rc\nV1 in 0 AC 1\nR1 in out {r}\nC1 out 0 {c}\nR2 out 0 10k\n.control\nwrdata o v(out)\n.endc"))
        .unwrap()
}

fn octaves() -> Vec<f64> {
    (0..10).map(|i| 50.0 * 2f64.powi(i)).collect()
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a / b - 1.0).abs() <= rel
}

#[test]
fn an_rc_recovers_its_parts_from_its_response_and_phase() {
    let bench = Bench::default();
    let truth = Parts::new(&rc("4.7k", "33n"), &Tolerances::new());
    let q: Vec<Quantity> = octaves()
        .iter()
        .flat_map(|&hz| [Quantity::ResponseDb { hz }, Quantity::PhaseDegrees { hz }])
        .collect();
    let target = Target::measured(&truth, &[4.7e3, 33e-9, 10e3], &q, &bench, Budget::default()).unwrap();
    let built = Parts::new(&rc("1k", "100n"), &Tolerances::new());
    let free = [Free::new("r1", 100.0, 1e6), Free::new("c1", 1e-9, 1e-6)];
    let f = fit(&built, &free, &target, &Fitting::default()).unwrap().named("rc");
    println!("{f}");
    assert!(near(f.value("r1").unwrap(), 4.7e3, 1e-6) && near(f.value("c1").unwrap(), 33e-9, 1e-6), "{f}");
    assert_eq!(f.value("r2"), Some(10e3), "a part not freed stays");
    assert!(f.error < 1e-4 && f.passed());
    assert_eq!(f.rows.len(), 20);
    assert!(f.audibility.is_err() && f.snapped.is_none() && f.capture.is_none());
    assert_eq!(f.stages.len(), 4);
    // The report is R1's form: the error against the budget, the worst point.
    let text = f.to_string();
    assert!(text.starts_with("fit `rc` ok: max |error| / budget") && text.contains("  worst     "), "{text}");
}

#[test]
fn snapping_reports_preferred_values_and_their_cost() {
    let bench = Bench::default();
    let truth = Parts::new(&rc("4.62k", "34.1n"), &Tolerances::new());
    let q: Vec<Quantity> = octaves().iter().map(|&hz| Quantity::ResponseDb { hz }).collect();
    let target = Target::measured(&truth, &[4.62e3, 34.1e-9, 10e3], &q, &bench, Budget { db: 0.01, degrees: 1.0 }).unwrap();
    let built = Parts::new(&rc("1k", "100n"), &Tolerances::new());
    let free = [Free::new("r1", 100.0, 1e6).snap(Series::E24), Free::new("c1", 1e-9, 1e-6).snap(Series::E12)];
    let f = fit(&built, &free, &target, &Fitting::default()).unwrap();
    println!("{f}");
    assert!(near(f.value("r1").unwrap(), 4.62e3, 1e-6), "{f}");
    let s = f.snapped.as_ref().expect("snapped");
    assert_eq!(s.parts.len(), 2);
    let r = s.parts.iter().find(|p| p.0 == "r1").unwrap().3;
    let c = s.parts.iter().find(|p| p.0 == "c1").unwrap().3;
    assert!([4.3e3, 4.7e3].iter().any(|v| near(r, *v, 1e-9)) && [33e-9, 39e-9].iter().any(|v| near(c, *v, 1e-9)), "{s:?}");
    // The cost is real and the snapped error is the snapped values' own.
    assert!(s.cost > 0.0 && s.error > f.error, "{s:?}");
    let direct = Target::measured(&built, &s.values, &q, &bench, Budget::default()).unwrap();
    let Target::Points(points) = direct else { unreachable!() };
    for (row, p) in s.rows.iter().zip(&points) {
        assert!((row.model - p.value).abs() < 1e-12);
    }
    // Every combination of neighbours was tried: none beats the one kept.
    for rr in [4.3e3, 4.7e3] {
        for cc in [33e-9, 39e-9] {
            let t = Target::measured(&built, &[rr, cc, 10e3], &q, &bench, Budget::default()).unwrap();
            let Target::Points(p) = t else { unreachable!() };
            let Target::Points(goal) = &target else { unreachable!() };
            let worst = p.iter().zip(goal).map(|(a, b)| ((a.value - b.value) / 0.01).abs()).fold(0.0, f64::max);
            assert!(worst >= s.error - 1e-9, "{rr} {cc}: {worst} < {}", s.error);
        }
    }
}

/// A memoryless tanh at drive `g`, measured on a short bench.
fn tanh_design() -> Parameters<impl Fn(&[f64]) -> Tick + Sync> {
    Parameters::new(vec![Parameter::new("drive", 1.0)], |v: &[f64]| {
        let g = v[0];
        Ticked(Box::new(move |x: f64| (g * x).tanh()) as Box<dyn FnMut(f64) -> f64>)
    })
}

#[test]
fn a_drive_recovered_from_a_harmonic_vs_level_curve() {
    let bench = Bench {
        settle_seconds: 0.0,
        ..Bench::default()
    };
    let levels = [0.05, 0.1, 0.2, 0.4, 0.8];
    let truth = tanh_design();
    let q: Vec<Quantity> = levels.iter().map(|&amplitude| Quantity::HarmonicDb { hz: 1e3, amplitude, harmonic: 3 }).collect();
    let Target::Points(points) = Target::measured(&truth, &[3.0], &q, &bench, Budget::default()).unwrap() else { unreachable!() };
    // As a table: (level, dBc), the way a datasheet's curve reads.
    let table: Vec<(f64, f64)> = levels.iter().zip(&points).map(|(a, p)| (*a, p.value)).collect();
    assert!(table[0].1 < table[4].1, "H3 rises with level: {table:?}");
    let target = Target::harmonic(1e3, 3, &table, 0.1);
    let fitting = Fitting { bench, ..Fitting::default() };
    let f = fit(&tanh_design(), &[Free::new("drive", 0.1, 30.0)], &target, &fitting).unwrap();
    println!("{f}");
    assert!(near(f.value("drive").unwrap(), 3.0, 1e-6), "{f}");
    assert!(f.rows[0].label.starts_with("H3 1 kHz @ 0.05"));
}

/// A one-pole low-pass at `hz` with an output gain, at `rate`.
fn one_pole(rate: f64) -> Parameters<impl Fn(&[f64]) -> Tick + Sync> {
    Parameters::new(vec![Parameter::new("cutoff", 1e3), Parameter::new("gain", 1.0)], move |v: &[f64]| {
        let (a, k) = (1.0 - (-TAU * v[0] / rate).exp(), v[1]);
        let mut y = 0.0;
        Ticked(Box::new(move |x: f64| {
            y += a * (x - y);
            k * y
        }) as Box<dyn FnMut(f64) -> f64>)
    })
}

/// Seeded noise, faded in and out over 10 ms (a fractional alignment
/// of an abrupt edge would ring).
fn noise(n: usize, rate: f64) -> Vec<f64> {
    let mut seed = 0x5eed_u32;
    let fade = (0.01 * rate) as usize;
    (0..n)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let w = (i.min(n - 1 - i) as f64 / fade as f64).min(1.0);
            0.5 * (f64::from(seed) / f64::from(u32::MAX) - 0.5) * (0.5 - 0.5 * (std::f64::consts::PI * w).cos())
        })
        .collect()
}

#[test]
fn a_capture_fit_recovers_the_model_and_nulls_inaudibly() {
    let rate = 48_000.0;
    let input = noise(24_000, rate);
    let mut truth = one_pole(rate).build(&[2_000.0, 0.8]).unwrap();
    let output: Vec<f64> = input.iter().map(|&x| truth.tick(x)).collect();
    let target = Target::capture(&input, &output, rate, 1e-3);
    let free = [Free::new("cutoff", 20.0, 20e3), Free::new("gain", 0.1, 10.0)];
    let f = fit(&one_pole(rate), &free, &target, &Fitting::default()).unwrap().named("one-pole capture");
    println!("{f}");
    assert!(near(f.value("cutoff").unwrap(), 2_000.0, 1e-6) && near(f.value("gain").unwrap(), 0.8, 1e-6), "{f}");
    let null = f.audibility.as_ref().expect("an audio-domain verdict");
    assert!(null.inaudible(), "{f}");
    assert!(f.capture.is_some_and(|(nrms, b)| nrms < 1e-6 && b == 1e-3) && f.rows.is_empty());
    assert!(f.to_string().contains("audible   inaudible"));
    // Bounds that exclude the truth: the fit pins at a bound, misses its
    // budget, and the residual is heard.
    let wrong = [Free::new("cutoff", 100.0, 1_000.0), Free::new("gain", 0.1, 10.0)];
    let f = fit(&one_pole(rate), &wrong, &target, &Fitting::default()).unwrap();
    println!("{f}");
    assert!(f.free[0].value > 950.0 && !f.passed(), "{f}");
    assert!(!f.audibility.as_ref().unwrap().inaudible(), "{f}");
    assert!(f.to_string().contains("FAILED") && f.to_string().contains("AUDIBLE"));
}

#[test]
fn several_starts_find_the_better_minimum() {
    // A target the single start from the far end of a log range can
    // reach too: several starts never do worse than one.
    let bench = Bench::default();
    let truth = Parts::new(&rc("4.7k", "33n"), &Tolerances::new());
    let q: Vec<Quantity> = octaves().iter().map(|&hz| Quantity::ResponseDb { hz }).collect();
    let target = Target::measured(&truth, &[4.7e3, 33e-9, 10e3], &q, &bench, Budget::default()).unwrap();
    let built = Parts::new(&rc("1k", "100n"), &Tolerances::new());
    let free = [Free::new("r1", 100.0, 1e6).from(900e3), Free::new("c1", 1e-9, 1e-6).from(1.1e-9)];
    let one = fit(&built, &free, &target, &Fitting::default()).unwrap();
    let five = fit(&built, &free, &target, &Fitting { starts: 5, ..Fitting::default() }).unwrap();
    assert!(five.error <= one.error, "{five}\n{one}");
}

#[test]
fn bad_requests_are_errors() {
    let built = Parts::new(&rc("1k", "100n"), &Tolerances::new());
    let target = Target::response(&[(1e3, -3.0)], 0.1);
    let fitting = Fitting::default();
    assert_eq!(fit(&built, &[], &target, &fitting).unwrap_err(), Error::Empty("free parameters"));
    assert_eq!(fit(&built, &[Free::new("r9", 1.0, 2.0)], &target, &fitting).unwrap_err(), Error::UnknownParameter("r9".into()));
    assert!(matches!(fit(&built, &[Free::new("r1", 2.0, 1.0)], &target, &fitting), Err(Error::Build(_))));
    assert_eq!(fit(&built, &[Free::new("r1", 1.0, 2.0)], &Target::Points(vec![]), &fitting).unwrap_err(), Error::Empty("target"));
}
