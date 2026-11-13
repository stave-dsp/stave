use super::*;
use crate::kernel::node::Stage;
use crate::primitives::spectral::ImpulseResponse;
use crate::physical::speaker::{Driver, LargeSignal, ThieleSmall};
use crate::primitives::time::interpolators::{Interpolator, Sinc};
use core::f64::consts::TAU;

const RATE: f64 = 48_000.0;

/// A minimum-phase "cabinet": the impulse response of three resonant
/// all-pole sections (minimum phase, since it has no zeros), `len`
/// samples, delayed by a whole `delay`.
fn all_pole(len: usize, delay: usize, tune: f64) -> Vec<f32> {
    let mut h = vec![0.0f64; len];
    if delay < len {
        h[delay] = 1.0;
    }
    for (hz, r) in [(110.0 * tune, 0.995), (1_900.0 * tune, 0.985), (3_400.0 / tune, 0.97)] {
        let w = TAU * hz / RATE;
        let (a1, a2) = (-2.0 * r * w.cos(), r * r);
        let (mut y1, mut y2) = (0.0, 0.0);
        for v in &mut h {
            let y = *v - a1 * y1 - a2 * y2;
            y2 = y1;
            y1 = y;
            *v = y;
        }
    }
    let peak = h.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    h.iter().map(|v| (v / peak) as f32).collect()
}

/// A smooth band-limited "capture" whose onset is at `delay` samples
/// (any real): a raised-cosine rise into damped tones, evaluated in
/// continuous time, so a fractional delay is exact up to its sampling.
fn smooth(len: usize, delay: f64) -> Vec<f64> {
    (0..len)
        .map(|n| {
            let t = (n as f64 - delay) / RATE;
            if t <= 0.0 {
                return 0.0;
            }
            let rise = 0.6e-3;
            let env = if t < rise { 0.5 - 0.5 * (core::f64::consts::PI * t / rise).cos() } else { 1.0 };
            let body: f64 = [(180.0, 60.0, 1.0), (1_300.0, 300.0, 0.6), (2_700.0, 500.0, 0.5), (5_000.0, 900.0, 0.2)]
                .iter()
                .map(|&(hz, sigma, amp)| amp * (-sigma * t).exp() * (TAU * hz * t).sin())
                .sum();
            env * body
        })
        .collect()
}

fn to_f32(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

/// The stage's impulse response, `len` samples.
fn impulse(stage: &mut impl Stage, len: usize) -> Vec<f64> {
    (0..len).map(|n| f64::from(stage.tick(if n == 0 { 1.0 } else { 0.0 }))).collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// `‖a − b‖/‖b‖`, dB.
fn residual(a: &[f64], b: &[f64]) -> f64 {
    let e: f64 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
    let r: f64 = b.iter().map(|y| y * y).sum();
    10.0 * (e / r).log10()
}

#[test]
fn shifts_are_exact_for_whole_samples_and_band_limited_otherwise() {
    let src = smooth(2_000, 20.0);
    let mut out = vec![0.0; 2_000];
    shift(&src, 7.0, &mut out);
    assert!(out[7..].iter().zip(&src).all(|(a, b)| a == b));
    shift(&src, -3.0, &mut out);
    assert!(out.iter().zip(&src[3..]).all(|(a, b)| a == b));
    // A fractional move of the smooth capture is the capture evaluated
    // at the moved onset.
    let mut worst = f64::NEG_INFINITY;
    for by in [0.25, 0.5, 3.71, -2.4] {
        shift(&src, by, &mut out);
        let want = smooth(2_000, 20.0 + by);
        worst = worst.max(residual(&out[40..1_900], &want[40..1_900]));
    }
    eprintln!("fractional shift residual {worst:.1} dB");
    assert!(worst < -70.0, "{worst}");
}

#[test]
fn arrivals_are_recovered() {
    // Whole samples on a minimum-phase response.
    let zero = arrival(&all_pole(8_192, 0, 1.0));
    for d in [5usize, 37] {
        let a = arrival(&all_pole(8_192, d, 1.0)) - zero;
        assert!((a - d as f64).abs() < 1e-3, "{d}: {a}");
    }
    eprintln!("a minimum-phase response's own arrival: {zero:.4} samples");
    // Fractions, relative to the same shape's arrival.
    let base = arrival(&to_f32(&smooth(8_192, 10.0)));
    let mut worst = 0.0f64;
    for d in [10.25, 13.5, 17.8, 42.33] {
        let a = arrival(&to_f32(&smooth(8_192, d)));
        worst = worst.max(((a - base) - (d - 10.0)).abs());
    }
    eprintln!("fractional arrivals within {worst:.5} samples");
    assert!(worst < 0.01, "{worst}");
}

#[test]
fn a_captured_position_is_its_capture() {
    let (h0, h1) = (smooth(3_000, 9.37), smooth(3_000, 23.0).iter().map(|v| 0.5 * v).collect::<Vec<_>>());
    let mut worst = f64::NEG_INFINITY;
    for (at, h) in [(0.0, &h0), (1.0, &h1)] {
        let mut cab = Captured::new(vec![vec![
            Position::new(0.0, ImpulseResponse::mono(&to_f32(&h0))),
            Position::new(1.0, ImpulseResponse::mono(&to_f32(&h1))),
        ]]);
        cab.set_placement(0, at);
        cab.prepare(RATE as f32);
        let y = impulse(&mut cab, 3_000);
        worst = worst.max(residual(&y, h));
    }
    eprintln!("captured positions: residual {worst:.1} dB");
    assert!(worst < -120.0, "{worst}");
}

#[test]
fn placement_interpolates_the_arrival() {
    // One shape captured 12.0 and 30.5 samples late: halfway, the shape
    // at 21.25 — not the two captures' comb.
    let n = 8_192;
    let mut cab = Captured::new(vec![vec![
        Position::new(0.0, ImpulseResponse::mono(&to_f32(&smooth(n, 12.0)))),
        Position::new(2.0, ImpulseResponse::mono(&to_f32(&smooth(n, 30.5)))),
    ]]);
    cab.set_placement(0, 1.0);
    cab.prepare(RATE as f32);
    let y = impulse(&mut cab, n);
    let want = smooth(n, 21.25);
    let comb: Vec<f64> = smooth(n, 12.0).iter().zip(smooth(n, 30.5)).map(|(a, b)| 0.5 * (a + b)).collect();
    let (shape, naive) = (residual(&y, &want), residual(&comb, &want));
    eprintln!("halfway: {shape:.1} dB from the shape at the halfway arrival (a plain blend: {naive:.1} dB)");
    assert!(shape < -60.0, "{shape}");
    assert!((cab.arrival(0) - (cab.arrivals(0)[0] + cab.arrivals(0)[1]) / 2.0).abs() < 1e-12);
}

#[test]
fn aligned_microphones_null() {
    for (d0, d1, budget) in [(4.0, 19.0, -95.0), (4.0, 17.62, -70.0)] {
        let run = |timing| {
            let mut cab = Captured::new(vec![
                vec![Position::new(0.0, ImpulseResponse::mono(&to_f32(&smooth(8_192, d0))))],
                vec![Position::new(0.0, ImpulseResponse::mono(&to_f32(&smooth(8_192, d1))))],
            ]);
            cab.set_timing(timing);
            cab.set_gain(1, 1.0, true);
            cab.prepare(RATE as f32);
            impulse(&mut cab, 8_192)
        };
        let one = smooth(8_192, d0);
        // The pair's sum against one microphone alone, dB.
        let level = |y: &[f64]| 10.0 * (y.iter().map(|v| v * v).sum::<f64>() / one.iter().map(|v| v * v).sum::<f64>()).log10();
        let null = level(&run(Timing::Aligned));
        // As captured, the inverted pair is a comb, not a null.
        let comb = level(&run(Timing::AsCaptured));
        eprintln!("mics {d0} and {d1} samples late, one inverted: aligned {null:.1} dB, as captured {comb:.1} dB");
        assert!(null < budget, "{null}");
        assert!(comb > -6.0);
    }
}

#[test]
fn minimum_phase_shapes_keep_the_magnitude() {
    let h = smooth(8_192, 15.0);
    let mut cab = Captured::new(vec![vec![Position::new(0.0, ImpulseResponse::mono(&to_f32(&h)))]])
        .with_shape(Shape::MinimumPhase);
    cab.prepare(RATE as f32);
    let y = impulse(&mut cab, 8_192);
    let mag = |v: &[f64]| {
        let fft = crate::primitives::spectral::Fft::<f64>::new(16_384);
        let mut buf = vec![0.0; 16_384];
        buf[..v.len()].copy_from_slice(v);
        let mut s = vec![crate::primitives::spectral::Complex::default(); 8_193];
        fft.forward_real(&buf, &mut s);
        s.iter().map(|c| c.norm()).collect::<Vec<_>>()
    };
    let (a, b) = (mag(&y), mag(&h));
    let peak = b.iter().fold(0.0f64, |m, &v| m.max(v));
    let worst = a.iter().zip(&b).take(7_400).filter(|(_, &w)| w > 1e-2 * peak).map(|(x, w)| db(x / w).abs()).fold(0.0, f64::max);
    eprintln!("minimum-phase shape: magnitude within {worst:.4} dB to 0.45 fs");
    assert!(worst < 0.05, "{worst}");
}

#[test]
fn matched_levels_have_equal_energy() {
    let quiet: Vec<f32> = all_pole(4_096, 3, 1.0).iter().map(|v| 0.1 * v).collect();
    let loud = all_pole(4_096, 3, 1.3);
    let mut cab = Captured::new(vec![vec![Position::new(0.0, ImpulseResponse::mono(&quiet))], vec![Position::new(0.0, ImpulseResponse::mono(&loud))]])
        .with_level(Level::Matched);
    let energy = |cab: &mut Captured, mic: usize| {
        cab.set_gain(mic, 1.0, false);
        cab.set_gain(1 - mic, 0.0, false);
        cab.prepare(RATE as f32);
        impulse(cab, 4_096).iter().map(|v| v * v).sum::<f64>()
    };
    let (e0, e1) = (energy(&mut cab, 0), energy(&mut cab, 1));
    eprintln!("matched energies {e0:.6}, {e1:.6}");
    assert!((e0 - 1.0).abs() < 1e-4 && (e1 - 1.0).abs() < 1e-4);
}

#[test]
fn placement_moves_are_click_free() {
    // A steady tone while the microphone sweeps between two captures:
    // the output's largest second difference stays near the steady
    // tone's (the crossfades are smooth), and its spray off the
    // carrier stays far below a hard cut's.
    let positions = vec![
        Position::new(0.0, ImpulseResponse::mono(&to_f32(&smooth(2_048, 6.0)))),
        Position::new(1.0, ImpulseResponse::mono(&to_f32(&smooth(2_048, 26.0).iter().map(|v| 0.7 * v).collect::<Vec<_>>()))),
    ];
    let mut cab = Captured::new(vec![positions]);
    cab.prepare(RATE as f32);
    let hz = 375.0; // 128 samples a cycle
    let n = 48_000;
    let mut y = Vec::with_capacity(n);
    for k in 0..n {
        if k >= 12_000 && k % 512 == 0 && k < 36_000 {
            cab.set_placement(0, (k - 12_000) as f32 / 24_000.0);
        }
        y.push(f64::from(cab.tick((TAU * hz * k as f64 / RATE).sin() as f32)));
    }
    let d2 = |v: &[f64]| v.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0, f64::max);
    let (steady, moving) = (d2(&y[6_000..12_000]), d2(&y[12_000..40_000]));
    eprintln!("largest second difference: steady {steady:.4}, moving {moving:.4}");
    assert!(moving < 1.5 * steady, "{moving} vs {steady}");
}

/// `2·J₁(x)/x` by its series.
fn piston(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        return 1.0;
    }
    let (mut sum, mut term) = (0.0, x / 2.0);
    for k in 0..40 {
        sum += term;
        term *= -(x * x / 4.0) / ((k + 1) as f64 * (k + 2) as f64);
    }
    2.0 * sum / x
}

#[test]
fn off_axis_lowpass_meets_the_piston() {
    assert!((piston(PISTON_MINUS_3_DB) - core::f64::consts::FRAC_1_SQRT_2).abs() < 1e-7);
    let ts = ThieleSmall::JENSEN_C12N_LR2;
    let placement = Placement { distance: 0.05, offset: 0.06, pattern: Pattern::OMNI };
    let mut on = CloseMic::new(Driver::new(ts, LargeSignal::LINEAR), Placement { offset: 0.0, ..placement }, Breakup::NONE);
    let mut off = CloseMic::new(Driver::new(ts, LargeSignal::LINEAR), placement, Breakup::NONE);
    on.prepare(RATE as f32);
    off.prepare(RATE as f32);
    let (a, r) = (ts.radius(), placement.distance);
    let sin = placement.offset / (placement.offset.powi(2) + r * r).sqrt();
    let corner = PISTON_MINUS_3_DB * SPEED_OF_SOUND / (TAU * a * sin);
    let gain = |m: &mut CloseMic, hz: f64| {
        m.reset();
        let (mut c, mut s) = (0.0, 0.0);
        let window = ((0.1 * hz).ceil() * RATE / hz).round() as usize;
        for k in 0..12_000 + window {
            let w = TAU * hz * k as f64 / RATE;
            let y = f64::from(m.tick(w.sin() as f32));
            if k >= 12_000 {
                c += y * w.cos();
                s += y * w.sin();
            }
        }
        2.0 * (c * c + s * s).sqrt() / window as f64
    };
    let at_corner = db(gain(&mut off, corner) / gain(&mut on, corner));
    eprintln!("offset 6 cm at 5 cm: corner {corner:.0} Hz, {at_corner:.3} dB there");
    assert!((at_corner + 3.0103).abs() < 0.02, "{at_corner}");
    // Against the piston's directivity below its first null (x < 3.83).
    let mut worst = 0.0f64;
    for x in [0.5, 1.0, 2.0, 3.0] {
        let hz = x * SPEED_OF_SOUND / (TAU * a * sin);
        let got = db(gain(&mut off, hz) / gain(&mut on, hz));
        worst = worst.max((got - db(piston(x).abs())).abs());
    }
    eprintln!("lowpass against 2·J1(x)/x up to x = 3: within {worst:.2} dB");
    assert!(worst < 6.0);
}

/// The close microphone's expected response at `hz`, from the driver's
/// analytic velocity (at the bilinear frequency, as the driver is
/// exact there) and the taps read through the interpolator's exact
/// response.
fn close_mic_expected(m: &CloseMic, hz: f64) -> Complex<f64> {
    let ts = ThieleSmall::JENSEN_C12N_LR2;
    let warped = (core::f64::consts::PI * hz / RATE).tan() * RATE / core::f64::consts::PI;
    let v = ts.velocity(warped, 0.0);
    let omega = TAU * hz / RATE;
    // A prepared interpolator (its tables are built by the line).
    let mut line: crate::primitives::time::delay::DelayLine<Sinc<8>> = Default::default();
    line.allocate(16);
    let read = |d: f32| {
        let whole = d.floor();
        let (re, im) = line.interpolator().response(d - whole, omega);
        let p = -omega * f64::from(whole);
        Complex::new(p.cos(), p.sin()) * Complex::new(re, im)
    };
    let (near, far, weight) = m.taps();
    (read(near) - read(far).scale(f64::from(weight))) * v.scale(m.scale())
}

#[test]
fn close_mic_is_the_piston_on_axis() {
    let mut worst = 0.0f64;
    for pattern in [Pattern::OMNI, Pattern::CARDIOID, Pattern::FIGURE_EIGHT] {
        for distance in [0.025, 0.1, 0.5] {
            let placement = Placement { distance, offset: 0.0, pattern };
            let mut m = CloseMic::new(Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR), placement, Breakup::NONE);
            m.prepare(RATE as f32);
            for hz in [60.0f64, 150.0, 700.0, 2_500.0, 9_000.0] {
                m.reset();
                let window = ((0.1 * hz).ceil() * RATE / hz).round() as usize;
                let (mut c, mut s) = (0.0, 0.0);
                for k in 0..24_000 + window {
                    let w = TAU * hz * k as f64 / RATE;
                    let y = f64::from(m.tick(w.cos() as f32));
                    if k >= 24_000 {
                        c += y * w.cos();
                        s += y * w.sin();
                    }
                }
                let got = Complex::new(2.0 * c / window as f64, -2.0 * s / window as f64);
                let want = close_mic_expected(&m, hz);
                worst = worst.max((got - want).norm() / want.norm());
            }
        }
    }
    eprintln!("close mic against the analytic piston: {worst:.2e} relative");
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn proximity_effect_matches_the_formula() {
    // The piston's on-axis gradient-to-pressure ratio, exactly, and its
    // far-field limit, the point source's 1 + c/(jωr).
    let a = ThieleSmall::JENSEN_C12N_LR2.radius();
    let ratio = |r: f64, hz: f64| {
        let r2 = (r * r + a * a).sqrt();
        let w = TAU * hz;
        let tau = (r2 - r) / SPEED_OF_SOUND;
        let e = Complex::new((w * tau).cos(), -(w * tau).sin());
        let p = Complex::new(1.0, 0.0) - e;
        let u = Complex::new(1.0, 0.0) - e.scale(r / r2);
        crate::physical::speaker::div(u, p)
    };
    let mut worst = 0.0f64;
    for hz in [40.0, 100.0, 250.0, 1_000.0] {
        let r = 2.0;
        let point = (1.0 + (SPEED_OF_SOUND / (TAU * hz * r)).powi(2)).sqrt();
        worst = worst.max((db(ratio(r, hz).norm()) - db(point)).abs());
    }
    eprintln!("piston at 2 m against 1 + c/(jωr): within {worst:.4} dB");
    assert!(worst < 0.05, "{worst}");
    // The microphones' figure-eight over omni (their taps through the
    // interpolator, as `close_mic_is_the_piston_on_axis` holds the
    // running stage to) is the piston's ratio.
    let mut worst = 0.0f64;
    for r in [0.025, 0.3] {
        let make = |pattern| {
            let mut m = CloseMic::new(
                Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR),
                Placement { distance: r, offset: 0.0, pattern },
                Breakup::NONE,
            );
            m.prepare(RATE as f32);
            m
        };
        let (omni, eight) = (make(Pattern::OMNI), make(Pattern::FIGURE_EIGHT));
        for hz in [50.0, 120.0, 400.0, 1_000.0] {
            let got = close_mic_expected(&eight, hz).norm() / close_mic_expected(&omni, hz).norm();
            worst = worst.max((db(got) - db(ratio(r, hz).norm())).abs());
        }
        eprintln!(
            "figure-eight over omni at {r} m: {:+.2} dB at 50 Hz, {:+.2} dB at 400 Hz",
            db(ratio(r, 50.0).norm()),
            db(ratio(r, 400.0).norm())
        );
    }
    eprintln!("taps against the continuous piston: within {worst:.4} dB");
    assert!(worst < 0.02, "{worst}");
}
