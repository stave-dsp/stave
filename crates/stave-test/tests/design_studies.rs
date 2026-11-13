//! Design studies (DSP.md G2, G3) drawn on the docs site's Design
//! studies page: each part's sensitivity across the band, and the spread
//! of a production run's built units with the part grade that dominates
//! it, for circuits in `spice/`. Each study is committed as
//! `design/<case>.design` (plain text the site draws at build time,
//! never images) and re-measured here: a model or a tool that moves the
//! numbers fails until it is re-blessed on purpose:
//!
//! ```text
//! STAVE_BLESS=1 cargo test -p stave-test --test design_studies
//! ```
//!
//! Numbers compare within 1e-6 (relative, with a 1e-9 floor): one
//! platform's `libm` against another's, far below any budget.

use std::fmt::Write as _;
use std::path::PathBuf;

use stave_dsp::circuit::netlist::tolerance::{Law, Tolerance, Tolerances};
use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
use stave_measure::design::{monte_carlo, sensitivity, Bench, Parts, Quantity, Spread};

/// The default bench on the threads the environment allows.
fn bench() -> Bench {
    Bench {
        threads: stave_test::threads(),
        ..Bench::default()
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn deck(name: &str) -> Netlist {
    let path = root().join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Third-octave centres, 20 Hz to 20 kHz.
fn third_octaves() -> Vec<f64> {
    (0..31).map(|i| 20.0 * 2f64.powf(f64::from(i) / 3.0)).collect()
}

fn grade(t: Tolerance) -> String {
    let law = match t.law {
        Law::Uniform => "uniform",
        Law::Gaussian => "gaussian",
    };
    format!("{} {law}", t.fraction * 100.0)
}

/// A study's text: header, parts, then per-quantity lines.
fn describe(case: &str, model: &str, response: &[f64], scalars: &[(&str, Quantity)], spread: &Spread, slopes: &stave_measure::design::Sensitivity) -> String {
    let mut s = format!("# stave design v1: {case}\n");
    s.push_str("# Written by STAVE_BLESS=1 (crates/stave-test/tests/design_studies.rs); CI diffs it (DSP.md G2, G3).\n");
    let _ = writeln!(s, "model {model}");
    let _ = writeln!(s, "units {} seed {}", spread.units(), spread.seed);
    for a in &spread.advice {
        let _ = writeln!(s, "part {} {} share {:.9e}", a.parameter, grade(a.tolerance), a.share);
    }
    let _ = writeln!(s, "verdict {}", spread.verdict());
    for (q, hz) in response.iter().enumerate() {
        let _ = writeln!(
            s,
            "band {hz:.9e} {:.9e} {:.9e} {:.9e} {:.9e}",
            spread.nominal[q],
            spread.percentile(q, 5.0),
            spread.percentile(q, 50.0),
            spread.percentile(q, 95.0)
        );
        for (p, part) in slopes.parameters.iter().enumerate() {
            let _ = writeln!(s, "slope {part} {hz:.9e} {:.9e}", slopes.slopes[p][q]);
        }
    }
    for (k, (name, _)) in scalars.iter().enumerate() {
        let q = response.len() + k;
        let _ = writeln!(
            s,
            "scalar {name} {} {:.9e} {:.9e} {:.9e} {:.9e} {:.9e}",
            spread.quantities[q].unit(),
            spread.nominal[q],
            spread.percentile(q, 5.0),
            spread.percentile(q, 50.0),
            spread.percentile(q, 95.0),
            spread.linear_sigma[q]
        );
        for (p, part) in slopes.parameters.iter().enumerate() {
            let _ = writeln!(s, "scalar-slope {name} {part} {:.9e}", slopes.slopes[p][q]);
        }
    }
    s
}

/// Same words, numbers within 1e-6 relative (1e-9 floor).
fn assert_current(case: &str, text: &str) {
    let path = root().join(format!("design/{case}.design"));
    if std::env::var_os("STAVE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (bless it: STAVE_BLESS=1)", path.display()));
    let (a, b): (Vec<&str>, Vec<&str>) = (committed.lines().collect(), text.lines().collect());
    assert_eq!(a.len(), b.len(), "{case}: the study's shape moved (re-bless on purpose)");
    for (n, (x, y)) in a.iter().zip(&b).enumerate() {
        let (wx, wy): (Vec<&str>, Vec<&str>) = (x.split_whitespace().collect(), y.split_whitespace().collect());
        let same = wx.len() == wy.len()
            && wx.iter().zip(&wy).all(|(p, q)| match (p.parse::<f64>(), q.parse::<f64>()) {
                (Ok(p), Ok(q)) => (p - q).abs() <= 1e-9_f64.max(1e-6 * p.abs().max(q.abs())),
                _ => p == q,
            });
        assert!(same, "{case}, line {}: committed\n  {x}\nnow\n  {y}\n(re-bless on purpose: STAVE_BLESS=1)", n + 1);
    }
}

fn study(case: &str, model: &str, design: &Parts, response: &[f64], scalars: &[(&str, Quantity)], units: usize, seed: u64) -> Spread {
    let bench = &bench();
    let mut quantities: Vec<Quantity> = response.iter().map(|&hz| Quantity::ResponseDb { hz }).collect();
    quantities.extend(scalars.iter().map(|(_, q)| *q));
    let slopes = sensitivity(design, &quantities, bench).unwrap();
    let spread = monte_carlo(design, &quantities, units, seed, bench).unwrap();
    println!("{case}: {}", spread.verdict());
    for (name, share) in spread.advice.iter().map(|a| (&a.parameter, a.share)) {
        println!("  {name}: {:.1} % of the variance it most affects", share * 100.0);
    }
    assert_current(case, &describe(case, model, response, scalars, &spread, &slopes));
    spread
}

#[test]
fn tone_stack_ab763() {
    // The deck's two treble legs are one pot, the bass a 250k pot as a
    // rheostat; both at half rotation.
    let mut n = deck("tone_stack_fender_ab763");
    n.add_variable(Variable::pot("treble", ["rt2", "rt1"], Taper::AUDIO)).unwrap();
    n.add_variable(Variable::rheostat("rb", 250e3, Taper::AUDIO)).unwrap();
    let grades = Tolerances::new()
        .resistors(Tolerance::uniform(0.05))
        .capacitors(Tolerance::uniform(0.10))
        .pots(Tolerance::uniform(0.20));
    let design = Parts::new(&n, &grades);
    let spread = study(
        "tone_stack_ab763",
        "Fender AB763 tone stack (spice/tone_stack_fender_ab763.cir), treble and bass at half: resistors ±5 %, capacitors ±10 %, pots ±20 %, uniform",
        &design,
        &third_octaves(),
        &[],
        1000,
        763,
    );
    // The mid scoop sits on the slope resistor and the mid capacitor; the
    // 250 pF treble cap rules the top. Some part dominates somewhere.
    assert!(spread.advice[0].share > 0.5);
}

#[test]
fn diode_clipper() {
    let n = deck("diode_clipper");
    let grades = Tolerances::new().resistors(Tolerance::uniform(0.05)).capacitors(Tolerance::uniform(0.10));
    let design = Parts::new(&n, &grades).drive("v1");
    let spread = study(
        "diode_clipper",
        "Diode clipper (spice/diode_clipper.cir): 2.2k into 10n, an antiparallel 1N4148 pair; resistor ±5 %, capacitor ±10 %, uniform",
        &design,
        &third_octaves(),
        &[
            ("thd_1k_0.5V", Quantity::ThdDb { hz: 1e3, amplitude: 0.5 }),
            ("thd_1k_2V", Quantity::ThdDb { hz: 1e3, amplitude: 2.0 }),
            ("gain_1k_2V", Quantity::GainDb { hz: 1e3, amplitude: 2.0 }),
        ],
        200,
        4148,
    );
    assert_eq!(spread.parameters, ["r1", "c1"]);
}
