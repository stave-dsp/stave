use std::f64::consts::{LN_10, PI, TAU};

use stave_dsp::circuit::netlist::tolerance::{Tolerance, Tolerances};
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::Stage;

use super::*;

const RC: &str = "* rc\nV1 in 0 AC 1\nR1 in out 1k\nC1 out 0 100n\n.control\nwrdata o v(out)\n.endc";
const RLC: &str = "* series rlc\nV1 in 0 AC 1\nR1 in a 100\nL1 a out 10m\nC1 out 0 1u\n.control\nwrdata o v(out)\n.endc";

fn netlist(text: &str) -> Netlist {
    Netlist::parse_spice(text).unwrap()
}

/// `|H|` in dB and its phase in degrees for `1/(a + jb)`.
fn response(a: f64, b: f64) -> (f64, f64) {
    (-10.0 * (a * a + b * b).log10(), -b.atan2(a).to_degrees())
}

#[test]
fn a_sweep_follows_an_rc_corner() {
    let design = Parts::new(&netlist(RC), &Tolerances::new());
    let ohms = vec![500.0, 1e3, 2e3, 4e3];
    let hz = 1.5e3;
    let s = sweep(&design, &[("R1", ohms.clone())], &[Quantity::ResponseDb { hz }], &Bench::default()).unwrap();
    assert_eq!(s.axes, ["r1"]);
    for ((at, q), r) in s.rows.iter().zip(&ohms) {
        assert_eq!(at[0], *r);
        let (db, _) = response(1.0, TAU * hz * r * 100e-9);
        assert!((q[0] - db).abs() < 1e-9, "R {r}: {} vs {db}", q[0]);
    }
    // Two axes: the last fastest.
    let two = sweep(&design, &[("r1", vec![1e3, 2e3]), ("c1", vec![1e-7, 2e-7])], &[Quantity::ResponseDb { hz }], &Bench::default()).unwrap();
    let at: Vec<Vec<f64>> = two.rows.iter().map(|(a, _)| a.clone()).collect();
    assert_eq!(at, [vec![1e3, 1e-7], vec![1e3, 2e-7], vec![2e3, 1e-7], vec![2e3, 2e-7]]);
    assert!((two.rows[1].1[0] - two.rows[2].1[0]).abs() < 1e-9, "RC is what counts");
    assert_eq!(sweep(&design, &[("r7", vec![1.0])], &[], &Bench::default()), Err(Error::UnknownParameter("r7".into())));
}

#[test]
fn rc_and_rlc_sensitivities_match_analytic() {
    let bench = Bench::default();
    // RC: |H|² = 1/(1 + b²), b = ωRC; R and C enter alike.
    let (r, c) = (1e3, 100e-9);
    let freqs = [100.0, 1591.549, 10e3];
    let quantities: Vec<Quantity> = freqs
        .iter()
        .flat_map(|&hz| [Quantity::ResponseDb { hz }, Quantity::PhaseDegrees { hz }])
        .collect();
    let s = sensitivity(&Parts::new(&netlist(RC), &Tolerances::new()), &quantities, &bench).unwrap();
    for (k, &hz) in freqs.iter().enumerate() {
        let b = TAU * hz * r * c;
        let d = 1.0 + b * b;
        let db_slope = -10.0 / LN_10 * 2.0 * b * b / d * 0.01;
        let phase_slope = -(b / d) * 180.0 / PI * 0.01;
        let (db, phase) = response(1.0, b);
        assert!((s.nominal[2 * k] - db).abs() < 1e-9, "{hz} Hz: {} vs {db}", s.nominal[2 * k]);
        assert!((s.nominal[2 * k + 1] - phase).abs() < 1e-7);
        for part in ["r1", "c1"] {
            let (got_db, got_phase) = (s.per_percent(part, 2 * k).unwrap(), s.per_percent(part, 2 * k + 1).unwrap());
            println!("RC {part} at {hz} Hz: {got_db:.9} dB/% (analytic {db_slope:.9}), {got_phase:.9} °/% ({phase_slope:.9})");
            assert!((got_db - db_slope).abs() <= 1e-6 * db_slope.abs().max(1e-3), "{part} {hz}: {got_db} vs {db_slope}");
            assert!((got_phase - phase_slope).abs() <= 1e-6 * phase_slope.abs().max(1e-3), "{part} {hz}: {got_phase} vs {phase_slope}");
        }
    }
    // Series RLC into C: H = 1/(a + jb), a = 1 − ω²LC, b = ωRC.
    let (r, l, c) = (100.0, 10e-3, 1e-6);
    let freqs = [300.0, 1591.549, 5e3];
    let quantities: Vec<Quantity> = freqs
        .iter()
        .flat_map(|&hz| [Quantity::ResponseDb { hz }, Quantity::PhaseDegrees { hz }])
        .collect();
    let s = sensitivity(&Parts::new(&netlist(RLC), &Tolerances::new()), &quantities, &bench).unwrap();
    for (k, &hz) in freqs.iter().enumerate() {
        let w = TAU * hz;
        let (a, b, lc) = (1.0 - w * w * l * c, w * r * c, w * w * l * c);
        let d = a * a + b * b;
        // d(a)/d(ln x) and d(b)/d(ln x) for R, L, C.
        for (part, da, db) in [("r1", 0.0, b), ("l1", -lc, 0.0), ("c1", -lc, b)] {
            let db_slope = -10.0 / LN_10 * (2.0 * a * da + 2.0 * b * db) / d * 0.01;
            let phase_slope = -(a * db - b * da) / d * 180.0 / PI * 0.01;
            let (got_db, got_phase) = (s.per_percent(part, 2 * k).unwrap(), s.per_percent(part, 2 * k + 1).unwrap());
            println!("RLC {part} at {hz} Hz: {got_db:.9} dB/% (analytic {db_slope:.9}), {got_phase:.9} °/% ({phase_slope:.9})");
            assert!((got_db - db_slope).abs() <= 1e-6 * db_slope.abs().max(1e-3), "{part} {hz}: {got_db} vs {db_slope}");
            assert!((got_phase - phase_slope).abs() <= 1e-6 * phase_slope.abs().max(1e-3), "{part} {hz}: {got_phase} vs {phase_slope}");
        }
    }
    // At resonance the inductor and capacitor move the peak most, the
    // resistor least.
    let ranked = s.ranked();
    assert_eq!(ranked.last().unwrap().0, "r1", "{ranked:?}");
}

/// `y = x − k·x³` at amplitude `a`: fundamental `a − 3ka³/4`, third
/// harmonic `ka³/4`.
fn cubic_slopes(k: f64, a: f64) -> (f64, f64, f64, f64) {
    let u = 0.75 * k * a * a;
    let thd = 20.0 * ((k * a * a * a / 4.0) / (a * (1.0 - u))).log10();
    let gain = 20.0 * (1.0 - u).log10();
    let thd_slope = 20.0 / LN_10 * (1.0 + u / (1.0 - u)) * 0.01;
    let gain_slope = 20.0 / LN_10 * (-u / (1.0 - u)) * 0.01;
    (thd, gain, thd_slope, gain_slope)
}

#[test]
fn a_stage_thd_sensitivity_matches_analytic() {
    let (k, a, hz) = (0.1, 0.8, 1e3);
    let quantities = [Quantity::ThdDb { hz, amplitude: a }, Quantity::GainDb { hz, amplitude: a }];
    let design = Parameters::new(vec![Parameter::new("k", k)], |v: &[f64]| {
        let k = v[0];
        Ticked(move |x: f64| x - k * x * x * x)
    });
    let s = sensitivity(&design, &quantities, &Bench::default()).unwrap();
    let (thd, gain, thd_slope, gain_slope) = cubic_slopes(k, a);
    println!("cubic: THD {:.9} dB ({thd:.9}), slope {:.9} ({thd_slope:.9}); gain slope {:.9} ({gain_slope:.9})", s.nominal[0], s.slopes[0][0], s.slopes[0][1]);
    assert!((s.nominal[0] - thd).abs() < 1e-9);
    assert!((s.nominal[1] - gain).abs() < 1e-9);
    assert!((s.slopes[0][0] - thd_slope).abs() < 1e-4 * thd_slope.abs());
    assert!((s.slopes[0][1] - gain_slope).abs() < 1e-4 * gain_slope.abs());

    // The same law as a Stage, run at f32 as a host runs it.
    struct Cubic(f32);
    impl Stage for Cubic {
        fn tick(&mut self, x: f32) -> f32 {
            x - self.0 * x * x * x
        }
    }
    let staged = Parameters::new(vec![Parameter::new("k", k)], |v: &[f64]| Staged(Cubic(v[0] as f32)));
    let bench = Bench { step: 1e-2, ..Bench::default() };
    let s = sensitivity(&staged, &quantities, &bench).unwrap();
    println!("cubic stage (f32): THD slope {:.6} ({thd_slope:.6})", s.slopes[0][0]);
    assert!((s.nominal[0] - thd).abs() < 1e-4);
    assert!((s.slopes[0][0] - thd_slope).abs() < 1e-3 * thd_slope.abs());
    // A stage has no linearisation: its small-signal response is measured.
    let s = sensitivity(&staged, &[Quantity::ResponseDb { hz }], &bench).unwrap();
    assert!(s.nominal[0].abs() < 1e-5, "{}", s.nominal[0]);
}

/// `P((1 + a)(1 + b) ≤ z)` for `a ~ U(±ta)`, `b ~ U(±tb)`, by Simpson's
/// rule over `a` (the inner probability exact).
fn product_cdf(z: f64, ta: f64, tb: f64) -> f64 {
    let n = 4000;
    let (lo, hi) = (1.0 - ta, 1.0 + ta);
    let inner = |u: f64| ((z / u - (1.0 - tb)) / (2.0 * tb)).clamp(0.0, 1.0);
    let h = (hi - lo) / n as f64;
    let mut s = inner(lo) + inner(hi);
    for i in 1..n {
        s += inner(lo + i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0 / (hi - lo)
}

fn product_quantile(p: f64, ta: f64, tb: f64) -> f64 {
    let (mut lo, mut hi) = ((1.0 - ta) * (1.0 - tb), (1.0 + ta) * (1.0 + tb));
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if product_cdf(mid, ta, tb) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[test]
fn rc_spread_matches_its_exact_distribution() {
    let (tr, tc) = (0.01, 0.10);
    let rc = netlist(RC);
    let grades = Tolerances::new().resistors(Tolerance::uniform(tr)).capacitors(Tolerance::uniform(tc));
    let design = Parts::new(&rc, &grades);
    let corner = 1.0 / (TAU * 1e3 * 100e-9);
    let freqs = [corner / 2.0, corner, 2.0 * corner];
    let quantities: Vec<Quantity> = freqs.iter().map(|&hz| Quantity::ResponseDb { hz }).collect();
    let units = 20_000;
    let threaded = Bench { threads: 3, ..Bench::default() };
    let spread = monte_carlo(&design, &quantities, units, 2026, &threaded).unwrap();
    assert_eq!(spread.units(), units);
    for (q, &hz) in freqs.iter().enumerate() {
        let w = TAU * hz * 1e3 * 100e-9;
        let db_at = |x: f64| -10.0 * (1.0 + (w * x).powi(2)).log10();
        for p in [5.0, 50.0, 95.0] {
            // dB falls as RC rises: the p-th percentile of dB is RC's (100 − p)-th.
            let x = product_quantile(1.0 - p / 100.0, tr, tc);
            let exact = db_at(x);
            // The order statistic's standard error: √(p(1−p)/N) / f(x), in dB.
            let dx = 1e-6;
            let density = (product_cdf(x + dx, tr, tc) - product_cdf(x - dx, tr, tc)) / (2.0 * dx);
            let pp = p / 100.0;
            let se = (pp * (1.0 - pp) / units as f64).sqrt() / density * ((db_at(x + dx) - db_at(x - dx)) / (2.0 * dx)).abs();
            let got = spread.percentile(q, p);
            println!("{hz:.1} Hz p{p}: {got:.6} dB, exact {exact:.6} dB, |Δ| {:.2e} (budget 4 SE = {:.2e})", (got - exact).abs(), 4.0 * se);
            assert!((got - exact).abs() <= 4.0 * se, "{hz} Hz p{p}: {got} vs {exact} (SE {se:e})");
        }
        let ratio = spread.sigma(q) / spread.linear_sigma[q];
        println!("{hz:.1} Hz: σ {:.5} dB, linear {:.5} dB", spread.sigma(q), spread.linear_sigma[q]);
        assert!((ratio - 1.0).abs() < 0.05, "σ against the linear prediction: {ratio}");
    }
    // R and C enter as RC, so their slopes are equal and the shares go as
    // their variances: 10² : 1².
    assert_eq!(spread.advice[0].parameter, "c1");
    let exact = tc * tc / (tc * tc + tr * tr);
    for s in &spread.advice[0].shares {
        assert!((s - exact).abs() < 1e-6, "{s} vs {exact}");
    }
    println!("{}", spread.verdict());
    // Seeded: the same batch again (on one thread), and the very units
    // Tolerances builds.
    let again = monte_carlo(&design, &quantities[..1], 50, 2026, &Bench::default()).unwrap();
    assert_eq!(again.samples[0][..], spread.samples[0][..50]);
    let unit = grades.unit(&rc, 2026, 7).unwrap();
    let built = Parts::new(&unit, &Tolerances::new());
    let mut circuit = built.build(&[unit.value("r1").unwrap(), unit.value("c1").unwrap()]).unwrap();
    let q = measure(&mut circuit, &quantities[..1], &Bench::default());
    assert_eq!(q[0], spread.samples[0][7]);
}
