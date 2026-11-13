use super::*;
use crate::kernel::node::Stage;

/// An RBJ low-pass at `hz`, `q` for `rate`.
fn lowpass(hz: f64, q: f64, rate: f64) -> Section {
    let w = std::f64::consts::TAU * hz / rate;
    let alpha = w.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let c = w.cos();
    Section {
        b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0],
        a: [-2.0 * c / a0, (1.0 - alpha) / a0],
    }
}

fn db(c: crate::primitives::spectral::Complex<f64>) -> f64 {
    20.0 * c.norm().log10()
}

#[test]
fn a_section_keeps_its_analog_prototype_at_another_rate() {
    let s = lowpass(1_000.0, 0.707, 48_000.0);
    let same = s.at_rate(48_000.0, 48_000.0);
    assert_eq!(same, s);
    let there = s.at_rate(48_000.0, 96_000.0);
    let back = there.at_rate(96_000.0, 48_000.0);
    for (x, y) in s.b.iter().chain(&s.a).zip(back.b.iter().chain(&back.a)) {
        assert!((x - y).abs() < 1e-12, "{s:?} vs {back:?}");
    }
    // The same analog prototype: equal responses where the two
    // bilinear transforms map the same analog frequency.
    let warp = |hz: f64, rate: f64| 2.0 * rate * (std::f64::consts::PI * hz / rate).tan();
    let unwarp = |omega: f64, rate: f64| rate / std::f64::consts::PI * (omega / (2.0 * rate)).atan();
    for hz in [50.0, 300.0, 1_000.0, 3_000.0, 12_000.0, 20_000.0] {
        let a = Linear::Rational(vec![s]).response(hz, 48_000.0);
        let b = Linear::Rational(vec![there]).response(unwarp(warp(hz, 48_000.0), 96_000.0), 96_000.0);
        assert!((a - b).norm() < 1e-9 * a.norm().max(1e-6), "{hz} Hz: {a:?} vs {b:?}");
    }
    // At the same frequency they differ by the warping alone: under
    // 0.2 dB to 3 kHz for this corner at 1 kHz.
    for hz in [50.0, 300.0, 1_000.0, 3_000.0] {
        let a = Linear::Rational(vec![s]).response(hz, 48_000.0);
        let b = Linear::Rational(vec![there]).response(hz, 96_000.0);
        assert!((db(a) - db(b)).abs() < 0.2, "{hz} Hz: {} vs {}", db(a), db(b));
    }
    assert!(there.is_stable());
}

#[test]
fn fir_blocks_convolve_and_keep_their_rate() {
    let taps = vec![0.5, -0.25, 0.125, 0.0625];
    let x: Vec<f64> = (0..32).map(|i| ((i * 7) % 5) as f64 - 2.0).collect();
    let mut run = linear::Running::new(&Linear::Fir(taps.clone()));
    for n in 0..x.len() {
        let want: f64 = (0..taps.len()).filter(|&k| k <= n).map(|k| taps[k] * x[n - k]).sum();
        assert!((run.tick(x[n]) - want).abs() < 1e-15, "sample {n}");
    }
    let cubic = Nonlinearity::Polynomial {
        coefficients: vec![0.0, 1.0],
        low: -1.0,
        high: 1.0,
    };
    let mut m = WienerHammerstein::hammerstein(48_000.0, cubic, Linear::Fir(taps));
    m.prepare(44_100.0);
    assert_eq!(m.tick(0.5), 0.0);
    assert!(m.rate_mismatch().is_some());
    m.set_rate_policy(crate::learned::RatePolicy::Any);
    assert_eq!(m.tick(0.5), 0.25);
}

#[test]
fn the_spline_reproduces_a_line_and_holds_its_edges() {
    // Unevenly spaced knots, denser near zero.
    let knots: Vec<f64> = (0..9).map(|k| 2.0 * (f64::from(k) / 4.0 - 1.0).powi(3)).collect();
    let values: Vec<f64> = knots.iter().map(|k| 0.5 * k).collect();
    let line = Nonlinearity::Spline { knots: knots.clone(), values };
    for i in 0..=400 {
        let x = -2.0 + 4.0 * f64::from(i) / 400.0;
        assert!((line.at(x) - 0.5 * x).abs() < 1e-12, "{x}: {}", line.at(x));
        let w: f64 = spline_weights(&knots, x).iter().map(|p| p.1).sum();
        assert!((w - 1.0).abs() < 1e-12);
    }
    assert_eq!(line.at(5.0), 1.0);
    assert_eq!(line.at(-5.0), -1.0);
    assert!(line.is_monotone());
    // C¹ at a knot: the slopes either side agree.
    let curve = Nonlinearity::Spline {
        knots: knots.clone(),
        values: knots.iter().map(|k| k.tanh()).collect(),
    };
    let k = knots[5];
    let (e, d) = (1e-7, 1e-9);
    let left = (curve.at(k - e) - curve.at(k - e - d)) / d;
    let right = (curve.at(k + e + d) - curve.at(k + e)) / d;
    assert!((left - right).abs() < 1e-4, "{left} vs {right}");
}

#[test]
fn a_file_reads_back_bit_for_bit() {
    let wh = WienerHammerstein::new(
        48_000.0,
        Linear::Rational(vec![lowpass(800.0, 0.6, 48_000.0)]),
        Nonlinearity::Spline {
            knots: (0..12).map(|k| (f64::from(k) / 11.0 * 5.0 - 2.5).powi(3) / 6.25).collect(),
            values: (0..12).map(|k| (f64::from(k) / 11.0 * 6.0 - 3.0).tanh() * 0.3 + 0.01).collect(),
        },
        Linear::Rational(vec![lowpass(5_000.0, 0.8, 48_000.0), Section::IDENTITY]),
    )
    .with_offset(4.5);
    let ph = ParallelHammerstein::new(
        44_100.0,
        vec![Linear::gain(0.9), Linear::Fir(vec![0.1, 0.05]), Linear::Rational(vec![lowpass(2e3, 1.0, 44_100.0)])],
    )
    .with_offset(-0.25)
    .with_limit(1.5);
    for model in [Block::from(wh), Block::from(ph)] {
        let text = model.to_json("a test \"model\"\n");
        let file = parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(file.note, "a test \"model\"\n");
        let (mut a, mut b) = (model.clone(), file.block);
        a.prepare(a.sample_rate());
        b.prepare(b.sample_rate());
        for i in 0..512 {
            let x = (f64::from(i) * 0.07).sin() * 2.0;
            assert_eq!(a.tick_f64(x).to_bits(), b.tick_f64(x).to_bits(), "sample {i}");
        }
    }
    assert!(matches!(parse("{\"format\": \"stave-block\", \"version\": 2}"), Err(LoadError::Config(_))));
    let unstable = "{\"format\": \"stave-block\", \"version\": 1, \"sample_rate\": 48000, \"architecture\": \"WienerHammerstein\",
      \"input\": {\"sections\": [[1, 0, 0, -2.5, 1.2]]}, \"output\": {\"sections\": []},
      \"nonlinearity\": {\"polynomial\": [0, 1], \"domain\": [-1, 1]}}";
    let e = parse(unstable).unwrap_err();
    assert!(e.to_string().contains("input filter"), "{e}");
}

#[test]
fn adaa_follows_the_plain_curve_at_low_frequencies() {
    let make = |adaa: bool| {
        let mut m = WienerHammerstein::wiener(
            48_000.0,
            Linear::identity(),
            Nonlinearity::Polynomial {
                coefficients: vec![0.0, 1.5, 0.0, -0.5],
                low: -1.0,
                high: 1.0,
            },
        );
        m.set_antialiasing(adaa);
        m.prepare(48_000.0);
        m
    };
    let (mut plain, mut smooth) = (make(false), make(true));
    let mut worst = 0.0f64;
    let mut previous = 0.0;
    for i in 0..4_800 {
        let x = 0.9 * (std::f64::consts::TAU * 100.0 * f64::from(i) / 48_000.0).sin();
        let y = smooth.tick_f64(x);
        // ADAA is the plain curve half a sample late, to the curvature.
        let half: f64 = plain.tick_f64(0.5 * (x + previous));
        previous = x;
        if i > 0 {
            worst = worst.max((y - half).abs());
        }
    }
    assert!(worst < 1e-4, "{worst}");
}
