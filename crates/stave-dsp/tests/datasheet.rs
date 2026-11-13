//! The primitive DATASHEET — ns/sample for every primitive, measured the
//! way a benchmark should (warm-up, repeated runs, median,
//! spread printed so a noisy run discredits itself). Like the
//! project perf reports it DOCUMENTS, never blocks: the numbers
//! belong in front of eyes when composing an engine ("can I afford
//! four of these per lane?"), not in a gate that cries wolf on
//! desktop hardware.
//!
//! Run: `cargo test --release -p stave-dsp --test datasheet -- --nocapture`
//! Debug builds skip (opt-level lies about release cost).
//!
//! Datasheet anchors (M4, apple-m1 target-cpu, 2026-07-06): most
//! primitives sit in the ones of ns/sample — Lfo 2.1, DelayLine
//! write+read_cubic 2.4, RandomWalk 2.6, Slew 2.9, Lag 4.1, Curve f_at
//! 4.4 (f_and_slope_at the same fetch), Allpass1 5.7, Biquad 7.1,
//! Adaa(1) 8.8, Hysteresis 10.5. The outlier is NonlinearRc's
//! Newton solve at ~63 — an implicit nonlinear ODE per sample is
//! the one primitive you budget around.

use stave_dsp::FilterSpec;
use stave_dsp::RcClipperConfig;
use stave_dsp::{AdaaOrder, AdsrConfig};
use std::hint::black_box;
use std::time::Instant;

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::{
    Adaa, Adsr, Allpass1, Biquad, Curve, DelayLine, DiodeClipper, DiodeClipperConfig, Fft,
    Hysteresis, Lag, Lfo, NonlinearRc, Osc, PairSolver, RandomWalk, Shockley, Slew, Stage,
    Waveform,
};

const N: usize = 1 << 17;
const REPEATS: usize = 5;

/// LCG noise in ±0.3 — the same stimulus family as the perf report.
fn noise(buf: &mut [f32]) {
    let mut lcg: u32 = 7;
    for x in buf.iter_mut() {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3;
    }
}

/// Median ns/sample over `REPEATS` timed passes (one warm-up pass),
/// printed with the spread.
fn bench(name: &str, input: &[f32], mut tick: impl FnMut(f32) -> f32) {
    let mut run = || {
        let t0 = Instant::now();
        let mut acc = 0.0f32;
        for &x in input {
            acc += tick(black_box(x));
        }
        black_box(acc);
        t0.elapsed().as_secs_f64() / input.len() as f64 * 1e9
    };
    run(); // warm
    let mut samples: Vec<f64> = (0..REPEATS).map(|_| run()).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[REPEATS / 2];
    let spread = (samples[REPEATS - 1] - samples[0]) / median * 100.0;
    println!("  {name:<28} {median:>7.2} ns/sample  (spread {spread:.0}%)");
}

/// Median ns per point of a whole-buffer operation over `REPEATS`
/// timed batches, for the transforms (cost per transform ÷ length).
fn bench_points(name: &str, points: usize, mut run_once: impl FnMut()) {
    let calls = (N / points).max(8);
    let mut run = || {
        let t0 = Instant::now();
        for _ in 0..calls {
            run_once();
        }
        t0.elapsed().as_secs_f64() / (calls * points) as f64 * 1e9
    };
    run();
    let mut samples: Vec<f64> = (0..REPEATS).map(|_| run()).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[REPEATS / 2];
    let spread = (samples[REPEATS - 1] - samples[0]) / median * 100.0;
    println!("  {name:<28} {median:>7.2} ns/sample  (spread {spread:.0}%)");
}

#[test]
fn primitive_datasheet() {
    if cfg!(debug_assertions) {
        eprintln!("datasheet SKIPPED (debug build — run under `cargo test --release`)");
        return;
    }
    let sr = 48_000.0;
    let mut input = vec![0.0f32; N];
    noise(&mut input);
    println!("stave-dsp primitive datasheet ({N} samples, median of {REPEATS})");

    let mut biquad = Biquad::new(FilterSpec::Lowpass {
        hz: 1_000.0,
        q: 0.707,
    });
    biquad.prepare(sr);
    bench("Biquad (lowpass)", &input, |x| biquad.tick(x));

    let mut ap = Allpass1::new(700.0);
    ap.prepare(sr);
    bench("Allpass1", &input, |x| ap.tick(x));

    let mut curve = Curve::flat();
    curve.rebuild(-4.0, 4.0, 0.0, |x| x.tanh());
    bench("Curve f_at", &input, |x| {
        curve.f_at(f64::from(x) * 3.0) as f32
    });
    bench("Curve f_and_slope_at", &input, |x| {
        curve.f_and_slope_at(f64::from(x) * 3.0).0 as f32
    });

    let mut adaa = Adaa::new(AdaaOrder::First);
    let framed = curve.framed(0.0, 1.0, 1.0);
    bench("Adaa(1) over Curve", &input, |x| {
        adaa.tick(&framed, f64::from(x) * 3.0) as f32
    });

    let mut hys = Hysteresis::new(2.2, 0.5);
    hys.prepare(sr);
    bench("Hysteresis", &input, |x| hys.tick(x));

    let mut slew = Slew::new(120.0, 120.0);
    slew.prepare(sr);
    bench("Slew", &input, |x| slew.tick(x));

    let mut rc = NonlinearRc::diode_clipper(&RcClipperConfig {
        tau_ms: 0.35,
        threshold: 0.5,
        hardness: 2.0,
    });
    rc.prepare(sr);
    bench("NonlinearRc (diode, Newton)", &input, |x| rc.tick(x));

    for (name, solver) in [
        ("DiodeClipper (WDF, Newton)", PairSolver::default()),
        ("DiodeClipper (WDF, Wright omega)", PairSolver::WrightOmega),
    ] {
        let mut wdf = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::D1N4148,
            solver,
        });
        wdf.prepare(sr);
        bench(name, &input, |x| wdf.tick(x));
    }

    let mut line = DelayLine::new();
    line.allocate(4_800);
    bench("DelayLine write+read_cubic", &input, |x| {
        line.write(x);
        line.advance();
        line.read_cubic(1_234.6)
    });

    let mut lag = Lag::new(50.0, 50.0);
    lag.prepare(sr);
    bench("Lag", &input, |x| lag.tick(x));

    let mut lfo = Lfo::new(1.3);
    lfo.prepare(sr);
    bench("Lfo", &input, |x| lfo.tick(x));

    let mut wander = RandomWalk::new(140.0, 30.0, 7);
    wander.prepare(sr);
    bench("RandomWalk", &input, |x| wander.tick(x));

    let mut adsr = Adsr::new();
    adsr.prepare(sr);
    adsr.set(&AdsrConfig {
        attack_ms: 2.0,
        decay_ms: 50.0,
        sustain: 0.7,
        release_ms: 100.0,
    });
    adsr.gate_on();
    bench("Adsr", &input, |x| adsr.tick(x));

    let mut osc_sine = Osc::new(Waveform::Sine);
    osc_sine.prepare(sr);
    osc_sine.set_hz(220.0);
    bench("Osc (sine)", &input, |x| osc_sine.tick(x));

    let mut osc_saw = Osc::new(Waveform::Saw);
    osc_saw.prepare(sr);
    osc_saw.set_hz(220.0);
    bench("Osc (saw, polyBLEP)", &input, |x| osc_saw.tick(x));

    for n in [512usize, 4096] {
        let fft = Fft::<f32>::new(n);
        let mut data: Vec<Complex<f32>> =
            input[..n].iter().map(|&x| Complex::new(x, 0.0)).collect();
        bench_points(&format!("Fft f32 complex {n}"), n, || {
            fft.forward(black_box(&mut data))
        });
        let mut bins = vec![Complex::default(); fft.real_bins()];
        bench_points(&format!("Fft f32 real {n}"), n, || {
            fft.forward_real(black_box(&input[..n]), &mut bins)
        });
    }
}
