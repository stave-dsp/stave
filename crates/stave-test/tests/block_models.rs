//! Block-oriented models (DSP.md M3) and the fitting workflow (M4), held
//! to what they can claim.
//!
//! Round trips: a known Wiener–Hammerstein system, and a known
//! polynomial Hammerstein system, each fitted from its own measurements,
//! come back — filters, curve and kernels within budgets.
//!
//! Circuits: the diode clipper, the Tube Screamer's gain stage and the
//! common-cathode triode, each measured through `Circuit<Dk>` at 4× (the
//! capture design's multisines, sweep and bursts, band-limited back to
//! 48 kHz), fitted, and the fitted model characterised against ngspice
//! on the circuit's own R17 plan at 48 kHz, beside the circuit's own
//! `Circuit<Dk>` at 48 kHz on the same plan. The held-out bursts give the
//! simulation error, ERB-band error and NMR (R19) of each model against
//! the oversampled circuit. Budgets are the recorded figures with a
//! margin; where the block model fails a check the circuit passes, the
//! row is a FINDING — the case where block models fall short.
//!
//! Each test runs on its own thread (libtest's); nothing here spawns.

use stave_dsp::learned::block::{Linear, Nonlinearity, ParallelHammerstein, Section, WienerHammerstein};
use stave_dsp::Stage;
use stave_measure::identify::capture::{Capture, Measurements};
use stave_measure::identify::hammerstein;
use stave_measure::identify::periodic::{resample, steady_state, Records};
use stave_measure::identify::wiener_hammerstein::{self as wh, Structure};
use stave_measure::identify::Validation;
use stave_measure::suite::sweep::Sweep;
use stave_measure::suite::Complex;
use stave_test::characterise::model::circuit;
use stave_test::characterise::{deck, Budget, Characterisation, Characterise, Check, Plan, Rates, Tier};

const RATE: f64 = 48_000.0;

/// An RBJ section at `rate`: low-pass, high-pass or peaking by `kind`.
fn rbj(kind: &str, hz: f64, q: f64, gain_db: f64) -> Section {
    let w = std::f64::consts::TAU * hz / RATE;
    let (c, s) = (w.cos(), w.sin());
    let alpha = s / (2.0 * q);
    let a = 10f64.powf(gain_db / 40.0);
    let (b, den) = match kind {
        "lp" => ([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
        "hp" => ([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
        _ => (
            [1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a],
        ),
    };
    Section {
        b: [b[0] / den[0], b[1] / den[0], b[2] / den[0]],
        a: [den[1] / den[0], den[2] / den[0]],
    }
}

/// A one-pole section (bilinear) at `hz`: low-pass or high-pass.
fn one_pole(high: bool, hz: f64) -> Section {
    let k = (std::f64::consts::PI * hz / RATE).tan();
    let a1 = (k - 1.0) / (k + 1.0);
    if high {
        Section {
            b: [1.0 / (1.0 + k), -1.0 / (1.0 + k), 0.0],
            a: [a1, 0.0],
        }
    } else {
        Section {
            b: [k / (1.0 + k), k / (1.0 + k), 0.0],
            a: [a1, 0.0],
        }
    }
}

/// Steady-state periods of `stage`.
fn periodic(stage: impl Stage + Clone) -> impl FnMut(&[f64]) -> Vec<f64> {
    move |x: &[f64]| {
        let mut s = stage.clone();
        s.prepare(RATE as f32);
        let mut tick = |v: f64| f64::from(s.tick(v as f32));
        steady_state(&mut tick, x, 4)
    }
}

/// `b/a`'s deviation from a constant complex gain over `points`: the
/// worst dB and degrees once the mean ratio is taken out.
fn shape_error(a: &[Complex<f64>], b: &[Complex<f64>]) -> (f64, f64) {
    let div = |x: Complex<f64>, y: Complex<f64>| stave_measure::suite::divide(x, y);
    let ratios: Vec<Complex<f64>> = a.iter().zip(b).map(|(x, y)| div(*y, *x)).collect();
    let mean = ratios.iter().fold(Complex::new(0.0, 0.0), |s, r| s + *r).scale(1.0 / ratios.len() as f64);
    ratios.iter().fold((0.0f64, 0.0f64), |(db, deg), r| {
        let d = div(*r, mean);
        (db.max((20.0 * d.norm().log10()).abs()), deg.max(d.arg().to_degrees().abs()))
    })
}

fn grid() -> Vec<f64> {
    (0..=60).map(|i| 20.0 * 1000f64.powf(f64::from(i) / 60.0)).collect()
}

#[test]
fn a_known_wiener_hammerstein_system_comes_back() {
    // A high-pass shelf and low-pass before an asymmetric soft clipper,
    // a resonant low-pass after it.
    let h = Linear::Rational(vec![one_pole(true, 150.0), rbj("peak", 900.0, 0.8, 9.0)]);
    let curve = |u: f64| (1.6 * u + 0.25).tanh() - 0.25f64.tanh() + 0.05 * u;
    let knots: Vec<f64> = (0..64).map(|k| -3.0 + 6.0 * f64::from(k) / 63.0).collect();
    let f = Nonlinearity::Spline {
        values: knots.iter().map(|&u| curve(u)).collect(),
        knots,
    };
    let g = Linear::Rational(vec![rbj("lp", 3_500.0, 1.2, 0.0)]);
    let truth = WienerHammerstein::new(RATE as f32, h.clone(), f.clone(), g.clone());
    let levels = [0.03, 0.15, 0.4, 0.8];
    let records = Records::measure(RATE, 4096, (20.0, 20_000.0), &levels, 4, periodic(truth));
    let fit = wh::fit(&records, &wh::Options::default()).expect("a fit");
    println!(
        "round trip W-H: BLA order {} zeros / {} poles at {:.2e} dB, split {} of {}, error {:?}, shapes {:?}",
        fit.bla.zeros.len(),
        fit.bla.poles.len(),
        fit.bla_error_db,
        fit.split,
        fit.splits_tried,
        fit.error_db,
        fit.shapes
    );
    // Both filters, to a gain.
    let hz = grid();
    let resp = |l: &Linear| hz.iter().map(|&f| l.response(f, RATE)).collect::<Vec<_>>();
    let (hd, hp) = shape_error(&resp(&h), &resp(fit.model.input()));
    let (gd, gp) = shape_error(&resp(&g), &resp(fit.model.output()));
    println!("  H off by {hd:.2e} dB / {hp:.2e}°, G by {gd:.2e} dB / {gp:.2e}°");
    assert!(hd < 0.05 && hp < 2.0 && gd < 0.05 && gp < 2.0, "filters not recovered");
    // The curve, to the gains either side of it: f_fit(u) = γ·f(u/α) + δ
    // with α the input filters' gain ratio (read at 1 kHz).
    let alpha = stave_measure::suite::divide(fit.model.input().response(1_000.0, RATE), h.response(1_000.0, RATE)).re;
    let (lo, hi) = fit.model.nonlinearity().domain();
    // Over the inner 90 % of the fitted domain (its ends are where the
    // data thin out).
    let us: Vec<f64> = (0..200).map(|i| lo + (hi - lo) * (0.05 + 0.9 * f64::from(i) / 199.0)).collect();
    let truth_at: Vec<f64> = us.iter().map(|u| f.at(u / alpha)).collect();
    let fitted: Vec<f64> = us.iter().map(|&u| fit.model.nonlinearity().at(u)).collect();
    let (mt, mf) = (mean(&truth_at), mean(&fitted));
    let gamma = truth_at.iter().zip(&fitted).map(|(t, v)| (t - mt) * (v - mf)).sum::<f64>()
        / truth_at.iter().map(|t| (t - mt).powi(2)).sum::<f64>();
    let range = fitted.iter().fold(0.0f64, |m, v| m.max((v - mf).abs()));
    let curve_error = truth_at
        .iter()
        .zip(&fitted)
        .map(|(t, v)| ((v - mf) - gamma * (t - mt)).abs() / range)
        .fold(0.0, f64::max);
    println!("  curve off by {curve_error:.2e} of its range");
    assert!(curve_error < 0.01, "curve not recovered");
    assert!(fit.monotone);
    for &(rms, e) in &fit.error_db {
        assert!(e < -55.0, "output error {e:.1} dB at {rms}");
    }
    // A Wiener–Hammerstein system's BLA moves only in gain.
    assert!(fit.shapes.iter().all(|s| s.db < 0.5 && s.degrees < 10.0), "{:?}", fit.shapes);
}

/// The capture workflow end to end (DSP.md M4): the stimulus played
/// through a device as one recording — 437 samples of interface latency,
/// a little noise — aligned by cross-correlation, read back, and fitted.
#[test]
fn a_recording_is_aligned_and_fitted() {
    let device = WienerHammerstein::new(
        RATE as f32,
        Linear::Rational(vec![one_pole(true, 80.0), rbj("peak", 700.0, 0.7, 6.0)]),
        Nonlinearity::Polynomial {
            coefficients: vec![0.0, 1.0, 0.15, -0.25],
            low: -1.2,
            high: 1.2,
        },
        Linear::Rational(vec![rbj("lp", 5_000.0, 0.7, 0.0)]),
    );
    let mut capture = Capture::standard(RATE, 0.1);
    capture.realisations = 2;
    capture.sweep_seconds = 1.0;
    let x = capture.samples();
    let mut d = device.clone();
    d.prepare(RATE as f32);
    let mut rng = stave_measure::identify::periodic::Seeded::new(4);
    let mut recorded = vec![0.0; 437];
    recorded.extend(x.iter().map(|&v| d.tick_f64(v) + 1e-5 * rng.normal()));
    // The interface's loopback (the stimulus as the interface returned
    // it) gives the latency exactly; the device's own output, the
    // latency plus its group delay where it is loudest.
    let mut loopback = vec![0.0; 437];
    loopback.extend(x.iter().map(|&v| 0.9 * v));
    let latency = capture.align(&loopback, 4_800);
    assert_eq!(latency, 437, "aligned on the loopback");
    let own = capture.align(&recorded, 4_800);
    println!("recording: latency {latency} from the loopback, {own} from the device's output");
    assert!((437..437 + 8).contains(&own));
    let m = capture.read(&recorded, latency).unwrap();
    let fit = wh::fit(&m.records, &wh::Options::default()).unwrap();
    println!("recording: split {}, error {:?}", fit.split, fit.error_db);
    assert!(fit.error_db.iter().all(|&(_, e)| e < -45.0), "{:?}", fit.error_db);
    let mut model = fit.model.clone();
    model.prepare(RATE as f32);
    let y: Vec<f64> = m.bursts_input.iter().map(|&x| model.tick_f64(x)).collect();
    let v = Validation::of(&m.bursts_output, &y, RATE);
    println!("  held-out bursts: {:.1} dB, ERB {:.3} dB, NMR {:.1} dB", v.error_db, v.erb.max_db, v.nmr.total_db);
    assert!(v.error_db < -45.0);
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

#[test]
fn a_known_polynomial_hammerstein_system_comes_back() {
    let branches = vec![
        Linear::Rational(vec![rbj("lp", 6_000.0, 0.8, 0.0)]),
        Linear::Rational(vec![Section {
            b: [0.3 * one_pole(true, 250.0).b[0], 0.3 * one_pole(true, 250.0).b[1], 0.0],
            ..one_pole(true, 250.0)
        }]),
        Linear::Rational(vec![rbj("peak", 1_500.0, 1.0, 6.0), Section {
            b: [-0.2, 0.0, 0.0],
            a: [0.0, 0.0],
        }]),
    ];
    // The truth runs at 4x on the band-limited sweep (its analog
    // prototype: every section kept through the rate change), so its
    // harmonics above 24 kHz are dropped, not folded, as a measurement
    // through an anti-aliased interface would see them.
    let truth = ParallelHammerstein::new(RATE as f32, branches.clone());
    let up = 4.0 * RATE;
    let analog: Vec<Linear> = branches
        .iter()
        .map(|b| match b {
            Linear::Rational(s) => Linear::Rational(s.iter().map(|x| x.at_rate(RATE, up)).collect()),
            other => other.clone(),
        })
        .collect();
    let run = |x: &[f64]| {
        let mut t = truth.clone();
        t.prepare(up as f32);
        let hi: Vec<f64> = resample(x, 4 * x.len()).iter().map(|&v| t.tick_f64(v)).collect();
        resample(&hi, x.len())
    };
    let sweep = Sweep::synchronized(10.0, 0.47 * RATE, 2.0, 0.8);
    let input = hammerstein::stimulus(&sweep, RATE, 0.01, 0.2);
    let output = run(&input);
    let options = hammerstein::Options {
        order: 3,
        ..hammerstein::Options::default()
    };
    let fit = hammerstein::fit(&sweep, RATE, &input, &output, &options).expect("a fit");
    println!(
        "round trip polynomial Hammerstein: kernel {} samples, dropped {:?}, branch fits {:?} dB",
        fit.kernel_len, fit.dropped, fit.branch_error_db
    );
    for (m, (kernel, truth)) in fit.kernels.iter().zip(&analog).enumerate() {
        let peak = kernel.iter().fold(0.0f64, |p, (_, v)| p.max(v.norm()));
        // Above the sweep's first decade (10 Hz × 10) and below 18 kHz; the
        // rational fit to 16 kHz (its order cannot follow an analog phase to
        // Nyquist).
        let inside = |f: f64| (100.0..18_000.0).contains(&f);
        let worst = kernel
            .iter()
            .filter(|(f, _)| inside(*f))
            .map(|&(f, v)| ((v - truth.response(f, up)).norm() / peak, f))
            .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
        let model = fit.model.branches()[m].clone();
        let fitted = kernel
            .iter()
            .filter(|(f, _)| *f < 16_000.0 && inside(*f))
            .map(|&(f, _)| ((model.response(f, RATE) - truth.response(f, up)).norm() / peak, f))
            .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
        println!(
            "  G{}: kernel off by {:.2e} of its peak (at {:.0} Hz), its rational fit by {:.2e} (at {:.0} Hz)",
            m + 1,
            worst.0,
            worst.1,
            fitted.0,
            fitted.1
        );
        let (worst, fitted) = (worst.0, fitted.0);
        assert!(worst < 3e-3 && fitted < 4e-2, "G{} not recovered", m + 1);
    }
    // On held-out noise, the model is the system: noise below 7 kHz (its
    // cube stays below Nyquist, so the model at 48 kHz folds nothing the
    // band-limited truth drops), inside the sweep's peak.
    let mut rng = stave_measure::identify::periodic::Seeded::new(9);
    let white: Vec<f64> = (0..65_536).map(|_| rng.normal()).collect();
    let mut s = stave_measure::identify::periodic::spectrum(&white);
    for (k, v) in s.iter_mut().enumerate() {
        if k.min(65_536 - k) as f64 * RATE / 65_536.0 > 7_000.0 {
            *v = Complex::new(0.0, 0.0);
        }
    }
    let shaped = stave_measure::identify::periodic::waveform(&s);
    let peak = shaped.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let noise: Vec<f64> = shaped.iter().map(|v| 0.75 * v / peak).collect();
    let ya = run(&noise);
    let mut b = fit.model.clone();
    b.prepare(RATE as f32);
    let yb: Vec<f64> = noise.iter().map(|&x| b.tick_f64(x)).collect();
    let v = Validation::of(&ya, &yb, RATE);
    println!("  held out: {:.1} dB, ERB {:.3} dB, NMR {:.1} dB", v.error_db, v.erb.max_db, v.nmr.total_db);
    assert!(v.error_db < -45.0);
}

/// A circuit deck measured at 4×, its plan and knee.
struct Measured {
    plan: Plan,
    capture: Capture,
    m: Measurements,
}

fn measure(name: &str) -> Measured {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{name}.plan"))).unwrap()).unwrap();
    let knee = plan.knee.expect("a knee").amplitude;
    // 0 dB is the knee's sine, in RMS.
    let mut capture = Capture::standard(RATE, knee / std::f64::consts::SQRT_2);
    capture.realisations = 2;
    capture.sweep_seconds = 1.0;
    capture.bursts_db = vec![-20.0, -6.0, 0.0, 6.0, 12.0];
    let netlist = deck(name);
    let input = plan.input.clone();
    let m = capture.run(4, move || {
        let mut c = circuit(&netlist, &input, 4.0 * RATE);
        Box::new(move |x| c.tick_f64(x))
    });
    Measured { plan, capture, m }
}

/// `model` on the bursts against the oversampled circuit: the whole
/// held-out signal, and each burst's level alone.
fn validate(c: &Capture, m: &Measurements, mut tick: impl FnMut(f64) -> f64) -> (Validation, Vec<(f64, Validation)>) {
    let y: Vec<f64> = m.bursts_input.iter().map(|&x| tick(x)).collect();
    let each = c
        .burst_spans()
        .into_iter()
        .map(|(db, span)| (db, Validation::of(&m.bursts_output[span.clone()], &y[span], RATE)))
        .collect();
    (Validation::of(&m.bursts_output, &y, RATE), each)
}

fn show(what: &str, (v, each): &(Validation, Vec<(f64, Validation)>)) -> String {
    let levels: Vec<String> = each
        .iter()
        .map(|(db, v)| format!("{db:+.0}: {:.1} dB / ERB {:.2} / NMR {:.1}", v.error_db, v.erb.max_db, v.nmr.total_db))
        .collect();
    format!(
        "{what}: held-out error {:.1} dB, ERB worst {:.2} dB (rms {:.2}) at {:.0} Hz, NMR total {:.1} dB worst {:.1} dB\n      by burst level (dB re knee: error / ERB worst / NMR total): {}",
        v.error_db,
        v.erb.max_db,
        v.erb.rms_db,
        v.erb.at_hz,
        v.nmr.total_db,
        v.nmr.max_db,
        levels.join("; ")
    )
}

/// The block model `make`s on the circuit's T0 plan at 48 kHz against
/// ngspice, with `budgets`.
fn characterise<T: Stage + Clone + 'static>(
    name: &str,
    label: &str,
    model: T,
    tick: fn(&mut T, f64) -> f64,
    budgets: &[(Check, f64, f64)],
) -> Characterisation {
    let mut c = Characterise::ngspice(name)
        .named(format!("{name}/{label}"))
        .tier(Tier::T0)
        .oversampled(1)
        .stage(move |rate| {
            let mut m = model.clone();
            m.prepare(rate as f32);
            move |x| tick(&mut m, x)
        });
    for &(check, first, second) in budgets {
        let b = if second.is_nan() { Budget::db(first) } else { Budget::db(first).degrees(second) };
        c = c.budget(check, b);
    }
    c.run()
}

/// The circuit's own `Circuit<Dk>` at 48 kHz on the same plan, for the
/// comparison (no budgets: `characterise.rs` holds those).
fn circuit_at_host_rate(name: &str) -> Characterisation {
    Characterise::ngspice(name).named(format!("{name}/dk-48k")).tier(Tier::T0).oversampled(1).circuit().run()
}

fn rows(c: &Characterisation) -> String {
    let mut s = String::new();
    for check in [Check::Response, Check::Bla, Check::Compression, Check::Harmonics, Check::ThdN, Check::Intermodulation, Check::Dynamics, Check::Aliasing, Check::Anmr] {
        if let Some(r) = c.rows.iter().find(|r| r.check == check && r.class == Rates::Oversampled) {
            s.push_str(&format!("    {:<22} {:.3e} / {:.3e}\n", check.name(), r.measurement.first.error, r.measurement.second.error));
        }
    }
    s
}

fn wh_ticks(m: &mut WienerHammerstein, x: f64) -> f64 {
    m.tick_f64(x)
}

fn ph_ticks(m: &mut ParallelHammerstein, x: f64) -> f64 {
    m.tick_f64(x)
}

/// What a circuit's two fits are held to: per structure, the T0 rows at
/// 48 kHz against ngspice (`(check, first, second)`), and the held-out
/// error at each burst level (−20, −6, 0, +6, +12 dB re the knee), each
/// the recorded figure with a margin (2× the T0 errors, 3 dB on the
/// held-out ones and the ceilings).
struct Expect {
    wiener_hammerstein: (&'static [(Check, f64, f64)], [f64; 5]),
    parallel: (&'static [(Check, f64, f64)], [f64; 5]),
}

/// Fit both structures to `name`, report, and hold each to `expect`.
fn circuit_case(name: &str, expect: &Expect) {
    let measured = measure(name);
    let m = &measured.m;
    let fit = wh::fit(&m.records, &wh::Options::default()).expect("a W-H fit");
    let ph = hammerstein::fit(&m.sweep, RATE, &m.sweep_input, &m.sweep_output, &hammerstein::Options::default())
        .expect("a Hammerstein fit");
    let re_knee = |rms: f64| format!("{:+.0} dB", 20.0 * (rms / measured.capture.reference).log10());
    println!(
        "{name}: knee {:.3e} V; W-H: BLA {} zeros / {} poles (worst {:.3} dB), split {} (best of {}), curve {}monotone, \
         in-band error by level {:?}",
        measured.plan.knee.unwrap().amplitude,
        fit.bla.zeros.len(),
        fit.bla.poles.len(),
        fit.bla_error_db,
        fit.split,
        fit.splits_tried,
        if fit.monotone { "" } else { "NOT " },
        fit.error_db.iter().map(|(r, e)| (re_knee(*r), (e * 10.0).round() / 10.0)).collect::<Vec<_>>(),
    );
    println!(
        "  BLA shape against level (a W-H system's moves only in gain): {:?}",
        fit.shapes
            .iter()
            .map(|s| format!("{}: {:.2} dB at {:.0} Hz, {:.1}°", re_knee(s.rms), s.db, s.at_hz, s.degrees))
            .collect::<Vec<_>>()
    );
    println!(
        "  parallel Hammerstein: kernel {} samples, dropped {:?}, branch fits {:?} dB",
        ph.kernel_len, ph.dropped, ph.branch_error_db
    );
    let mut w = fit.model.clone();
    w.prepare(RATE as f32);
    let held_wh = validate(&measured.capture, m, |x| w.tick_f64(x));
    let mut p = ph.model.clone();
    p.prepare(RATE as f32);
    let held_ph = validate(&measured.capture, m, |x| p.tick_f64(x));
    let mut dk = circuit(&deck(name), &measured.plan.input, RATE);
    let held_dk = validate(&measured.capture, m, |x| dk.tick_f64(x));
    println!("  {}", show("W-H", &held_wh));
    println!("  {}", show("parallel Hammerstein", &held_ph));
    println!("  {}", show("Circuit<Dk> at 48 kHz", &held_dk));
    let host = circuit_at_host_rate(name);
    let c = characterise(name, "wiener_hammerstein", fit.model.clone(), wh_ticks, expect.wiener_hammerstein.0);
    let q = characterise(name, "parallel_hammerstein", ph.model.clone(), ph_ticks, expect.parallel.0);
    println!(
        "  T0 at 48 kHz against ngspice, worst first / second error — Circuit<Dk>:\n{}  W-H:\n{}  parallel Hammerstein:\n{}",
        rows(&host),
        rows(&c),
        rows(&q)
    );
    for (what, held, budget) in [
        ("W-H", &held_wh, expect.wiener_hammerstein.1),
        ("parallel Hammerstein", &held_ph, expect.parallel.1),
    ] {
        for ((db, v), b) in held.1.iter().zip(budget) {
            assert!(v.error_db <= b, "{name} {what}: held-out error {:.1} dB at {db:+} dB re knee, budget {b}", v.error_db);
        }
    }
    c.assert();
    q.assert();
}

use Check::{Aliasing, Anmr, Compression, Response};

/// FINDINGS (the block models fall short where the circuit's filtering
/// moves with level): the clipper's capacitor across its diodes — its
/// BLA's corner moves 3.7 dB by +12 dB re knee — leaves the W–H fit
/// −28.6 dB from the circuit at −20 dB but −10.9 at +12; the power
/// series, fitted at the knee, is −23.8 / −5.2 and aliases audibly at
/// +12 (ANMR −4.2 dB).
#[test]
fn fitted_to_the_diode_clipper() {
    circuit_case(
        "diode_clipper",
        &Expect {
            wiener_hammerstein: (
                &[(Response, 1.0, 6.0), (Compression, 1.5, 3.0), (Aliasing, -67.0, f64::NAN), (Anmr, -22.7, f64::NAN)],
                [-25.6, -25.5, -22.7, -13.9, -7.9],
            ),
            parallel: (
                &[(Response, 1.3, 9.0), (Compression, 7.0, 2.5), (Aliasing, -45.1, f64::NAN), (Anmr, -1.2, f64::NAN)],
                [-20.8, -20.0, -9.7, -4.4, -2.2],
            ),
        },
    );
}

/// FINDINGS: the TS-808 gain stage's feedback diodes shunt its gain
/// shelf as it is driven (the BLA moves 4.0 dB at 90 Hz by +12 dB):
/// W–H −31.4 dB at −20, −11.5 at +12, 0.96 dB of compression error at
/// +12 where `Circuit<Dk>` at 48 kHz has 0.004; the power series −18.5 /
/// −3.4.
#[test]
fn fitted_to_the_tube_screamer() {
    circuit_case(
        "tube_screamer",
        &Expect {
            wiener_hammerstein: (
                &[(Response, 1.1, 4.0), (Compression, 2.0, 4.5), (Aliasing, -77.5, f64::NAN), (Anmr, -22.4, f64::NAN)],
                [-28.4, -28.2, -25.8, -16.6, -8.5],
            ),
            parallel: (
                &[(Response, 6.3, 18.5), (Compression, 13.2, 4.8), (Aliasing, -46.0, f64::NAN), (Anmr, -3.0, f64::NAN)],
                [-15.5, -15.5, -8.8, -2.9, -0.4],
            ),
        },
    );
}

/// The common cathode is a Wiener–Hammerstein system to within its
/// shape's 0.03 dB: the W–H fit matches `Circuit<Dk>` at 48 kHz level
/// for level on the bursts (−57.3 vs −57.2 dB at −20, −30.0 vs −29.4 at
/// +12). FINDING: its power series, fitted at the knee, falls apart above
/// it (−2.7 dB at +12, ANMR +12.7 dB).
#[test]
fn fitted_to_the_common_cathode() {
    circuit_case(
        "common_cathode",
        &Expect {
            wiener_hammerstein: (
                &[(Response, 0.01, 0.02), (Compression, 0.01, 0.01), (Aliasing, -110.0, f64::NAN), (Anmr, -23.7, f64::NAN)],
                [-54.3, -45.4, -39.4, -33.2, -27.0],
            ),
            parallel: (
                &[(Response, 0.05, 3.0), (Compression, 20.0, 0.1), (Aliasing, -35.6, f64::NAN), (Anmr, 15.7, f64::NAN)],
                [-38.2, -23.6, -9.3, -2.6, 0.3],
            ),
        },
    );
}

#[test]
fn structure_restrictions_hold() {
    // A Hammerstein-only fit of a Hammerstein system puts every root after the curve.
    let g = Linear::Rational(vec![rbj("lp", 2_000.0, 0.9, 0.0)]);
    let f = Nonlinearity::Polynomial {
        coefficients: vec![0.0, 1.0, 0.2, -0.3],
        low: -1.2,
        high: 1.2,
    };
    let truth = WienerHammerstein::hammerstein(RATE as f32, f, g);
    let records = Records::measure(RATE, 2048, (20.0, 20_000.0), &[0.05, 0.3, 0.6], 1, periodic(truth));
    let options = wh::Options {
        structure: Structure::Hammerstein,
        ..wh::Options::default()
    };
    let fit = wh::fit(&records, &options).unwrap();
    assert_eq!(fit.splits_tried, 1);
    assert!(fit.split.starts_with("H: 0 zeros, 0 poles"), "{}", fit.split);
    assert!(fit.error_db.iter().all(|&(_, e)| e < -50.0), "{:?}", fit.error_db);
}
