//! Design fits (DSP.md G4) on the real tone stacks: the Fender AB763
//! recovers its own parts and pot rotations from its response, and is
//! fitted to the Marshall 2203's ngspice response — the fit's error in
//! budgets, then the fitted stack held to the 2203's deck by the R1
//! harness itself (`Case::ngspice`, `Metric::MaxDb`).

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::tolerance::Tolerances;
use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
use stave_dsp::circuit::Circuit;
use stave_measure::design::{fit, Bench, Budget, Design, Fitting, Free, Parts, Quantity, Series, Target};
use stave_test::reference::{Case, Metric, Reference};

fn deck(name: &str) -> Netlist {
    let path = format!("{}/spice/{name}.cir", env!("CARGO_MANIFEST_DIR"));
    Netlist::parse_spice(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The AB763 stack with its treble pot and bass rheostat declared, their
/// rotations parameters (`treble.position`, `rb.position`).
fn ab763() -> Parts {
    let mut n = deck("tone_stack_fender_ab763");
    n.add_variable(Variable::pot("treble", ["rt2", "rt1"], Taper::AUDIO)).unwrap();
    n.add_variable(Variable::rheostat("rb", 250e3, Taper::AUDIO)).unwrap();
    Parts::new(&n, &Tolerances::new()).setting("treble").unwrap().setting("rb").unwrap()
}

fn bench() -> Bench {
    Bench {
        threads: stave_test::threads(),
        ..Bench::default()
    }
}

/// Third-octave centres, 20 Hz to 20 kHz.
fn third_octaves() -> Vec<f64> {
    (0..31).map(|i| 20.0 * 2f64.powf(f64::from(i) / 3.0)).collect()
}

/// The design's values with some set by name.
fn values(design: &Parts, set: &[(&str, f64)]) -> Vec<f64> {
    design
        .parameters()
        .iter()
        .map(|p| set.iter().find(|(n, _)| *n == p.name).map_or(p.nominal, |(_, v)| *v))
        .collect()
}

/// The AB763 at treble 0.3, bass 0.7, an 82k slope resistor and a 56n
/// mid capacitor is the target; the stack as drawn (both pots at half,
/// 100k, 47n) recovers all four from its response within 1e-6.
#[test]
fn ab763_recovers_its_parts_and_rotations() {
    let design = ab763();
    let truth = values(&design, &[("treble.position", 0.3), ("rb.position", 0.7), ("r4", 82e3), ("c3", 56e-9)]);
    let quantities: Vec<Quantity> = third_octaves().iter().map(|&hz| Quantity::ResponseDb { hz }).collect();
    let budget = Budget { db: 0.01, degrees: 0.1 };
    let target = Target::measured(&design, &truth, &quantities, &bench(), budget).unwrap();
    let free = [
        Free::new("treble.position", 0.0, 1.0),
        Free::new("rb.position", 0.0, 1.0),
        Free::new("r4", 10e3, 1e6),
        Free::new("c3", 1e-9, 1e-6),
    ];
    let fitting = Fitting {
        bench: bench(),
        starts: 4,
        ..Fitting::default()
    };
    let f = fit(&design, &free, &target, &fitting).unwrap().named("AB763 from its own response");
    println!("{f}");
    for (name, want) in [("treble.position", 0.3), ("rb.position", 0.7), ("r4", 82e3), ("c3", 56e-9)] {
        let got = f.value(name).unwrap();
        assert!((got / want - 1.0).abs() < 1e-6, "{name}: {got} for {want}\n{f}");
    }
    f.assert();
}

/// The AB763 fitted to the Marshall 2203's response at its half settings
/// (`spice/tone_stack_marshall_2203.ngspice.txt`, 61 points from 20 Hz
/// to 20 kHz, as a table): the two pots' rotations and five parts free,
/// the parts then snapped to E12 (the bass capacitor to E6). As drawn
/// the AB763 is 14.0 dB off at 317 Hz (the 2203 has a third of its
/// scoop); fitted, 6.1e-6 dB — the two networks share their order, and
/// seven values reach the 2203's response exactly, the bass rheostat at
/// its stop (470 pF, 16.1k, 45.5 nF, 6.49k, 44.7 nF) — and at E12 (470
/// pF, 18k, 47 nF, 6.8k, 39 nF, the rotations refitted) 0.159 dB at
/// 632 Hz.
#[test]
fn ab763_fitted_to_the_marshall_2203() {
    let design = ab763();
    let reference = Reference::ngspice("tone_stack_marshall_2203");
    let table: Vec<(f64, f64)> = reference
        .abscissa()
        .iter()
        .zip(reference.values())
        .map(|(&hz, &m)| (hz, 20.0 * m.log10()))
        .collect();
    let target = Target::response(&table, BUDGET_DB);
    let free = [
        Free::new("treble.position", 0.0, 1.0),
        Free::new("rb.position", 0.0, 1.0),
        Free::new("c1", 47e-12, 4.7e-9).snap(Series::E12),
        Free::new("r4", 4.7e3, 470e3).snap(Series::E12),
        Free::new("c2", 2.2e-9, 470e-9).snap(Series::E6),
        Free::new("rm", 100.0, 100e3).snap(Series::E12),
        Free::new("c3", 2.2e-9, 470e-9).snap(Series::E12),
    ];
    let fitting = Fitting {
        bench: bench(),
        starts: 6,
        ..Fitting::default()
    };
    let response = |values: &[f64]| {
        let circuit: Circuit<Dk> = design.build(values).unwrap();
        move |hz: f64| circuit.analog_response(hz).unwrap().magnitude()
    };
    let case = || Case::ngspice("tone_stack_marshall_2203").budget(Metric::MaxDb, BUDGET_DB);
    // As drawn, the AB763 is far from the 2203.
    let drawn = case().named("AB763 as drawn vs the 2203").pointwise(response(&values(&design, &[])));
    println!("{drawn}");
    assert!(drawn.error > 10.0, "{drawn}");
    let f = fit(&design, &free, &target, &fitting).unwrap().named("AB763 to the 2203");
    println!("{f}");
    // The R1 harness on the fitted stack reads the fit's own worst point.
    let fitted = case().named("AB763 fitted to the 2203").pointwise(response(&f.values));
    assert!((fitted.error - f.error * BUDGET_DB).abs() < 1e-9, "{fitted}\n{f}");
    fitted.assert();
    let s = f.snapped.as_ref().expect("snapped");
    let snapped = case().named("AB763 fitted to the 2203, E12").budget(Metric::MaxDb, SNAPPED_BUDGET_DB).pointwise(response(&s.values));
    snapped.assert();
}

/// The 2203's response met by an AB763, worst point: measured 6.1e-6 dB
/// (the fit's floor, not a model error), budgeted with room for another
/// platform's rounding along the solver's path.
const BUDGET_DB: f64 = 1e-4;
/// The same with its parts at preferred values: measured 0.159 dB.
const SNAPPED_BUDGET_DB: f64 = 0.2;
