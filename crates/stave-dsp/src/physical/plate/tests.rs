use super::*;
use crate::kernel::node::Stage;
use crate::units::{hz, seconds};
use core::f64::consts::{PI, TAU};

/// A small, stiff aluminium plate: a few hundred modes below 3 kHz,
/// the lowest well apart.
fn small(tension: f64) -> Geometry {
    Geometry { width: 0.41, height: 0.29, thickness: 2e-3, tension, material: Material::ALUMINIUM }
}

fn quick_loss() -> Loss {
    Loss::two_point(hz(0.0), seconds(0.8), hz(2_000.0), seconds(0.2)).unwrap()
}

fn plate(geometry: Geometry, loss: Loss, top: f32, rate: f32) -> Plate {
    let mut p = Plate::new(geometry, loss);
    p.set_bandwidth(hz(top));
    p.prepare(rate);
    p
}

#[test]
fn modes_are_leissas_without_tension() {
    // Leissa (1969), simply supported rectangle:
    // ω = π² √(D/ρh) (m²/a² + n²/b²).
    for g in [small(0.0), Geometry { tension: 0.0, ..Geometry::EMT140 }] {
        let p = plate(g, Loss::DAFX26, 4_000.0, 48_000.0);
        let k = (g.rigidity() / g.mass()).sqrt();
        let mut expected = 0usize;
        for m in 1..2_000u32 {
            for n in 1..2_000u32 {
                let w = PI * PI * k * (f64::from(m * m) / (g.width * g.width) + f64::from(n * n) / (g.height * g.height));
                if w <= TAU * 4_000.0 {
                    expected += 1;
                }
            }
        }
        assert_eq!(p.modes().len() + p.overdamped(), expected);
        for mode in p.modes() {
            let w = PI * PI * k * (f64::from(mode.m).powi(2) / g.width.powi(2) + f64::from(mode.n).powi(2) / g.height.powi(2));
            assert!((mode.natural / w - 1.0).abs() < 1e-12, "{mode:?} vs {w}");
        }
        assert!(p.modes().windows(2).all(|w| w[0].natural <= w[1].natural));
    }
}

#[test]
fn mode_count_follows_weyls_law() {
    let g = Geometry { tension: 0.0, ..Geometry::EMT140 };
    for top in [2_500.0f32, 5_000.0, 10_000.0] {
        let p = plate(g, Loss::DAFX26, top, 48_000.0);
        let weyl = g.mode_count(TAU * f64::from(top));
        let n = p.modes().len() as f64;
        assert!((n / weyl - 1.0).abs() < 5e-3, "{top} Hz: {n} modes, Weyl {weyl:.0}");
    }
    // The EMT 140 at 10 kHz: 1.3 modes a hertz.
    let p = plate(Geometry::EMT140, Loss::DAFX26, 10_000.0, 48_000.0);
    assert!((12_500..13_500).contains(&p.modes().len()), "{}", p.modes().len());
}

/// The continuous solution, sampled: each mode's displacement, velocity
/// or acceleration impulse response at `t = nT`, in SI, times `T`.
fn analytic(p: &Plate, output: Output, len: usize, rate: f64) -> Vec<f64> {
    let t = 1.0 / rate;
    let mass = p.geometry().mass();
    let mut y = vec![0.0f64; len];
    for m in p.modes() {
        let (w, s) = (m.damped(), m.sigma);
        let a = t * m.drive * m.pickups[0] / (mass * w);
        for (n, v) in y.iter_mut().enumerate() {
            let tn = n as f64 * t;
            let (sin, cos) = (w * tn).sin_cos();
            let e = a * (-s * tn).exp();
            *v += match output {
                Output::Displacement => e * sin,
                Output::Velocity => e * (w * cos - s * sin),
                Output::Acceleration => e * ((s * s - w * w) * sin - 2.0 * s * w * cos),
            };
        }
    }
    y
}

#[test]
fn impulse_response_is_the_continuous_solution() {
    let rate = 48_000.0;
    let mut worst: f64 = 0.0;
    for output in [Output::Displacement, Output::Velocity, Output::Acceleration] {
        let mut p = plate(small(150.0), quick_loss(), 3_000.0, rate as f32);
        p.set_output(output);
        let len = 4_800;
        let want = analytic(&p, output, len, rate);
        let scale = p.scale(0);
        let peak = want.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        let mut x = vec![0.0f32; len];
        x[0] = 1.0;
        p.process(&mut x);
        let err = x.iter().zip(&want).fold(0.0f64, |a, (&g, w)| a.max((f64::from(g) / scale - w).abs()));
        worst = worst.max(err / peak);
        assert!(err / peak < 1e-4, "{output:?}: {:.2e} of the peak", err / peak);
    }
    eprintln!("bank vs continuous solution: worst {worst:.2e} of the peak");
}

/// `|Σ y e^(−jωn)|` at `omega` rad/sample.
fn dtft(y: &[f64], omega: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    let (s1, c1) = omega.sin_cos();
    let (mut c, mut s) = (1.0f64, 0.0f64);
    for &v in y {
        re += v * c;
        im -= v * s;
        let next = c * c1 - s * s1;
        s = s * c1 + c * s1;
        c = next;
    }
    (re * re + im * im).sqrt()
}

#[test]
fn modes_ring_where_the_theory_puts_them() {
    // Displacement IR, 2 s; each mode at least 1.5 % from its neighbours
    // is found as the DTFT's peak (golden section, f64) near it.
    let rate = 48_000.0f64;
    let loss = Loss::two_point(hz(0.0), seconds(1.5), hz(2_000.0), seconds(1.0)).unwrap();
    let mut p = plate(small(150.0), loss, 1_500.0, rate as f32);
    p.set_output(Output::Displacement);
    p.set_drive(Point::new(0.31, 0.27));
    p.set_pickup(0, Point::new(0.83, 0.62));
    let mut x = vec![0.0f32; 96_000];
    x[0] = 1.0;
    p.process(&mut x);
    let y: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let modes: Vec<Mode> = p.modes().to_vec();
    let mut checked = 0;
    let mut worst: f64 = 0.0;
    for (i, m) in modes.iter().enumerate() {
        let w = m.damped();
        let apart = |o: Option<&Mode>| o.is_none_or(|o| (o.damped() / w - 1.0).abs() > 0.015);
        let heard = (m.drive * m.pickups[0]).abs() > 0.2 * 4.0 / (0.41 * 0.29);
        if !(apart(modes.get(i + 1)) && apart(i.checked_sub(1).and_then(|j| modes.get(j))) && heard) {
            continue;
        }
        let (mut a, mut b) = (0.99 * w / rate, 1.01 * w / rate);
        let g = (5f64.sqrt() - 1.0) / 2.0;
        for _ in 0..60 {
            let (c, d) = (b - g * (b - a), a + g * (b - a));
            if dtft(&y, c) > dtft(&y, d) {
                b = d;
            } else {
                a = c;
            }
        }
        let found = 0.5 * (a + b) * rate;
        let err = (found / w - 1.0).abs();
        worst = worst.max(err);
        assert!(err < 1e-4, "mode ({}, {}) at {:.3} Hz found at {:.3}", m.m, m.n, w / TAU, found / TAU);
        checked += 1;
    }
    assert!(checked >= 6, "only {checked} isolated modes");
    eprintln!("{checked} modes found within {worst:.1e} of the theory");
}

#[test]
fn block_and_tick_agree_bit_for_bit() {
    let mut a = plate(small(150.0), quick_loss(), 3_000.0, 44_100.0);
    let mut b = a.clone();
    let mut c = a.clone();
    let mut d = a.clone();
    let input: Vec<f32> = (0..1_000).map(|i| (((i * 7919) % 211) as f32 / 105.0 - 1.0) * 0.5).collect();
    let ticked: Vec<f32> = input.iter().map(|&x| a.tick(x)).collect();
    let mut blocked = input.clone();
    for chunk in blocked.chunks_mut(37) {
        b.process(chunk);
    }
    assert_eq!(ticked, blocked);
    let stereo: Vec<(f32, f32)> = input.iter().map(|&x| c.tick_stereo(x)).collect();
    let (mut l, mut r) = (vec![0.0; 1_000], vec![0.0; 1_000]);
    for (k, chunk) in input.chunks(53).enumerate() {
        let at = k * 53;
        d.process_stereo(chunk, &mut l[at..at + chunk.len()], &mut r[at..at + chunk.len()]);
    }
    assert!(stereo.iter().zip(l.iter().zip(&r)).all(|(s, (l, r))| s.0 == *l && s.1 == *r));
    assert!(stereo.iter().zip(&ticked).all(|(s, t)| s.0 == *t), "the left pickup is the mono output");
}

#[test]
fn pickups_are_calibrated_to_unit_energy() {
    for output in [Output::Displacement, Output::Velocity, Output::Acceleration] {
        let mut p = plate(small(150.0), quick_loss(), 3_000.0, 48_000.0);
        p.set_output(output);
        let n = 48_000;
        let mut x = vec![0.0f32; n];
        x[0] = 1.0;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        p.process_stereo(&x, &mut l, &mut r);
        for y in [&l, &r] {
            let e: f64 = y.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
            assert!((10.0 * e.log10()).abs() < 0.5, "{output:?}: {:.2} dB", 10.0 * e.log10());
        }
        // At twice the rate, the same continuous energy: half the sum,
        // so a tone is as loud as at 48 kHz.
        let mut fast = plate(small(150.0), quick_loss(), 3_000.0, 96_000.0);
        fast.set_output(output);
        let mut x2 = vec![0.0f32; 2 * n];
        x2[0] = 1.0;
        let (mut l2, mut r2) = (vec![0.0; 2 * n], vec![0.0; 2 * n]);
        fast.process_stereo(&x2, &mut l2, &mut r2);
        for y in [&l2, &r2] {
            let e: f64 = y.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
            assert!((10.0 * (2.0 * e).log10()).abs() < 0.5, "{output:?} at 96 kHz: {:.2} dB", 10.0 * e.log10());
        }
        // Two pickups hear different mixtures of the same modes (but a
        // displacement pickup is ruled by the lowest mode, which both hear).
        let cross: f64 = l.iter().zip(&r).map(|(&a, &b)| f64::from(a) * f64::from(b)).sum();
        if output != Output::Displacement {
            assert!(cross.abs() < 0.3, "{output:?}: pickups correlate {cross}");
        }
    }
}

#[test]
fn loss_law_holds_its_two_points_and_rejects_gain() {
    let loss = Loss::two_point(hz(100.0), seconds(4.0), hz(3_000.0), seconds(0.5)).unwrap();
    assert!((loss.t60(100.0).get() - 4.0).abs() < 1e-12);
    assert!((loss.t60(3_000.0).get() - 0.5).abs() < 1e-12);
    // Decay lengthening with frequency is not passive.
    assert!(Loss::two_point(hz(0.0), seconds(1.0), hz(500.0), seconds(2.0)).is_none());
    // The challenge's fixed loss: 6 s at DC, 2 s at 500 Hz.
    assert!((Loss::DAFX26.t60(0.0).get() - 6.0).abs() < 1e-12);
    assert!((Loss::DAFX26.t60(500.0).get() - 2.0).abs() < 1e-12);
    // The EMT 140's damper: its 500 Hz T60 as set, the treble's loss kept.
    for t in [0.8, 2.0, 5.0] {
        let l = Loss::emt140(seconds(t)).unwrap();
        assert!((l.t60(500.0).get() / t - 1.0).abs() < 1e-12);
        assert!(l.t60(5_000.0).get() < l.t60(500.0).get());
    }
    assert!((Loss::emt140(seconds(5.0)).unwrap().t60(5_000.0).get() - 1.0).abs() < 1e-12);
    // Past about 120 s the damper would have to add energy.
    assert!(Loss::emt140(seconds(100.0)).is_some());
    assert!(Loss::emt140(seconds(150.0)).is_none());
}

#[test]
fn moving_a_pickup_moves_its_nodes() {
    // On the plate's centre line every mode with an even `m` is silent.
    let mut p = plate(small(150.0), quick_loss(), 3_000.0, 48_000.0);
    p.set_pickup(1, Point::new(0.5, 0.37));
    for m in p.modes() {
        if m.m % 2 == 0 {
            assert!(m.pickups[1].abs() < 1e-12, "{m:?}");
        }
    }
    let silent = p.modes().iter().filter(|m| m.pickups[1].abs() < 1e-12).count();
    assert!(silent * 3 > p.modes().len(), "{silent}");
}

#[test]
fn rate_and_bandwidth_bound_the_modes() {
    let mut p = Plate::new(small(150.0), quick_loss());
    p.set_bandwidth(hz(40_000.0));
    p.prepare(44_100.0);
    assert!((p.bandwidth().get() - 0.45 * 44_100.0).abs() < 1e-3);
    assert!(p.modes().iter().all(|m| m.hz() <= 0.45 * 44_100.0));
    let n44 = p.modes().len();
    p.prepare(96_000.0);
    assert!(p.modes().len() > n44);
    // Silence stays silent and finite, and the tail covers the slowest mode.
    let mut y = [0.0f32; 512];
    p.process(&mut y);
    assert!(y.iter().all(|&v| v == 0.0));
    let tail = f64::from(p.tail().to_samples().unwrap());
    let slowest = p.modes().iter().map(|m| m.t60().get()).fold(0.0, f64::max);
    assert!((tail / (2.0 * slowest * 96_000.0) - 1.0).abs() < 0.01, "{tail} vs {slowest} s");
}
