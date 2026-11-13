use super::*;
use crate::kernel::node::Stage;
use core::f64::consts::{PI, TAU};

const RATE: f64 = 48_000.0;

/// A 12-inch guitar-like driver: 75 Hz, Qms 5, Qes 0.6, 100 L, 6.4 Ω.
fn twelve(le: f64) -> ThieleSmall {
    ThieleSmall::from_small_signal(75.0, 5.0, 0.6, 0.1, 6.4, le, 0.05)
}

/// The frequency a trapezoidal (bilinear) discretisation at `rate`
/// shows the analog response of `hz` at: `tan(π f/rate)·rate/π`.
fn warped(hz: f64, rate: f64) -> f64 {
    (PI * hz / rate).tan() * rate / PI
}

/// Amplitude and phase of the `hz` component of `y` by a least-squares
/// fit of `a·cos + b·sin + c` (IEEE 1057's three-parameter fit), exact
/// for a steady sinusoid at any frequency.
fn sine_fit(y: &[f64], hz: f64, rate: f64, start: usize) -> Complex<f64> {
    let mut ata = [[0.0f64; 3]; 3];
    let mut aty = [0.0f64; 3];
    for (n, &v) in y.iter().enumerate() {
        let w = TAU * hz * (start + n) as f64 / rate;
        let row = [w.cos(), w.sin(), 1.0];
        for r in 0..3 {
            aty[r] += row[r] * v;
            for c in 0..3 {
                ata[r][c] += row[r] * row[c];
            }
        }
    }
    // 3×3 solve by Cramer's rule.
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(ata);
    let mut sol = [0.0; 2];
    for (k, s) in sol.iter_mut().enumerate() {
        let mut m = ata;
        for r in 0..3 {
            m[r][k] = aty[r];
        }
        *s = det(m) / d;
    }
    // a·cos + b·sin = Re{(a − jb)·e^{jωt}}.
    Complex::new(sol[0], -sol[1])
}

/// Steady-state complex response of the coil current and the cone's
/// acceleration to a 1 V sine at `hz`, by simulation.
fn measure(driver: &mut Driver, hz: f64) -> (Complex<f64>, Complex<f64>, Complex<f64>) {
    driver.prepare(RATE as f32);
    let settle = (1.0 * RATE) as usize;
    let keep = (0.25 * RATE) as usize;
    let (mut u, mut i, mut a) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..settle + keep {
        let v = (TAU * hz * n as f64 / RATE).cos();
        let acc = driver.drive(v);
        if n >= settle {
            u.push(v);
            i.push(driver.current());
            a.push(acc);
        }
    }
    (sine_fit(&u, hz, RATE, settle), sine_fit(&i, hz, RATE, settle), sine_fit(&a, hz, RATE, settle))
}

fn rel(a: Complex<f64>, b: Complex<f64>) -> f64 {
    (a - b).norm() / b.norm()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

#[test]
fn small_signal_is_the_bilinear_transform_of_the_analytic_model() {
    let ts = twelve(0.6e-3).with_lr2(0.9e-3, 8.0);
    let source = 2.0;
    let mut d = Driver::new(ts, LargeSignal::LINEAR);
    d.set_source_ohms(source);
    let (mut worst_exact, mut worst_band) = (0.0f64, 0.0f64);
    for &hz in &[20.0, 40.0, 75.0, 110.0, 300.0, 1_000.0, 1_900.0, 5_000.0, 12_000.0] {
        let (u, i, a) = measure(&mut d, hz);
        // The source's EMF over the current is the driver plus the
        // amplifier's output resistance.
        let z = div(u, i) - Complex::new(source, 0.0);
        let acc = div(a, u);
        let fw = warped(hz, RATE);
        let (ez, ea) = (rel(z, ts.impedance(fw)), rel(acc, ts.acceleration(fw, source)));
        worst_exact = worst_exact.max(ez).max(ea);
        if hz <= 2_000.0 {
            let dz = db(z.norm() / ts.impedance(hz).norm()).abs();
            let da = db(acc.norm() / ts.acceleration(hz, source).norm()).abs();
            worst_band = worst_band.max(dz).max(da);
        }
    }
    eprintln!("bilinear model {worst_exact:.2e} relative; analog below 2 kHz {worst_band:.4} dB");
    assert!(worst_exact < 1e-9, "{worst_exact}");
    assert!(worst_band < 0.025, "{worst_band}");
}

#[test]
fn the_impedance_peaks_at_fs() {
    let ts = twelve(0.0);
    let mut d = Driver::new(ts, LargeSignal::LINEAR);
    // The frequency the discrete model puts the analog fs at.
    let fs = ts.resonance();
    let fd = RATE / PI * (PI * fs / RATE).atan();
    let z = |d: &mut Driver, hz| {
        let (u, i, _) = measure(d, hz);
        div(u, i)
    };
    let peak = z(&mut d, fd);
    let want = ts.re * (1.0 + ts.qms() / ts.qes());
    eprintln!(
        "fs {fs:.4} Hz (discrete {fd:.4}): |Z| {:.6} Ω vs Re(1 + Qms/Qes) {want:.6}, phase {:.2e} rad",
        peak.norm(),
        peak.arg()
    );
    assert!((peak.norm() / want - 1.0).abs() < 1e-9);
    assert!(peak.arg().abs() < 1e-8);
    for k in [0.999, 1.001] {
        assert!(z(&mut d, fd * k).norm() < peak.norm(), "{k}");
    }
    // Small's relations close on themselves.
    assert!((ts.resonance() / 75.0 - 1.0).abs() < 1e-12);
    assert!((ts.qms() / 5.0 - 1.0).abs() < 1e-12 && (ts.qes() / 0.6 - 1.0).abs() < 1e-12);
    assert!((ts.vas() / 0.1 - 1.0).abs() < 1e-12);
}

#[test]
fn the_response_is_a_qts_highpass() {
    let ts = twelve(0.0);
    let qts = ts.qts();
    let mut d = Driver::new(ts, LargeSignal::LINEAR);
    let ws = TAU * ts.resonance();
    let level = |d: &mut Driver, hz: f64| {
        let (u, _, a) = measure(d, hz);
        // Normalised as the stage's output is: unity in the mass band.
        div(a, u).norm() * ts.mms * ts.re / ts.bl
    };
    let analytic = |hz: f64| {
        let w = TAU * warped(hz, RATE);
        w * w / ((ws * ws - w * w).powi(2) + (w * ws / qts).powi(2)).sqrt()
    };
    let fs = ts.resonance();
    let at_fs = level(&mut d, fs);
    let slope = db(level(&mut d, fs / 16.0) / level(&mut d, fs / 32.0));
    let want_slope = db(analytic(fs / 16.0) / analytic(fs / 32.0));
    eprintln!(
        "Qts {qts:.4}: level at fs {:.4} dB (20·log Qts {:.4}); slope fs/32→fs/16 {slope:.4} dB/oct (analytic {want_slope:.4})",
        db(at_fs),
        db(qts)
    );
    assert!((db(at_fs) - db(analytic(fs))).abs() < 1e-6);
    assert!((db(analytic(fs)) - db(qts)).abs() < 1e-3);
    assert!((slope - want_slope).abs() < 1e-6 && (slope - 12.04).abs() < 0.05);
    // The passband sits at unity through the stage's own normalisation.
    let mut s = Driver::new(ts, LargeSignal::LINEAR);
    s.prepare(RATE as f32);
    let hz = 600.0;
    let y: Vec<f64> = (0..48_000).map(|n| f64::from(s.tick((TAU * hz * n as f64 / RATE).cos() as f32))).collect();
    let g = sine_fit(&y[24_000..], hz, RATE, 24_000).norm();
    assert!((g / analytic(hz) - 1.0).abs() < 1e-5, "{g}");
}

/// Large-signal curves with every term: symmetric droop and stiffening,
/// and the tilts that make a real motor asymmetric.
fn grinding() -> LargeSignal {
    LargeSignal {
        bl: Polynomial::new(&[1.0, -40.0, -0.18 / (1.5e-3f64).powi(2)]),
        kms: Polynomial::new(&[1.0, 120.0, (1.0 / 0.75 - 1.0) / (1.2e-3f64).powi(2)]),
        le: Polynomial::new(&[1.0, -150.0, -2.0e4]),
        range: 6e-3,
    }
}

/// The module's equations written out again here, independently of the
/// driver's own Jacobian-carrying form: `ds/dt` for `(i, i₂, v, x)`.
fn equations(ts: &ThieleSmall, large: &LargeSignal, s: &[f64; 4], u: f64) -> [f64; 4] {
    let [i, i2, v, x] = *s;
    let p = |c: &[f64], x: f64| c.iter().rev().fold(0.0, |acc, &k| acc * x + k);
    let dp = |c: &[f64], x: f64| c.iter().enumerate().skip(1).map(|(k, &ck)| k as f64 * ck * x.powi(k as i32 - 1)).sum::<f64>();
    let bl = ts.bl * p(large.bl.coefficients(), x);
    let kms = p(large.kms.coefficients(), x) / ts.cms;
    let le = ts.le * p(large.le.coefficients(), x);
    let dle = ts.le * dp(large.le.coefficients(), x);
    let u2 = ts.r2 * (i - i2);
    // u = Re·i + d(Le·i)/dt + u₂ + Bl·v, d(Le·i)/dt = Le·di/dt + i·Le′·v.
    let di = (u - ts.re * i - u2 - bl * v - i * dle * v) / le;
    let di2 = u2 / ts.l2;
    let dv = (bl * i - ts.rms * v - kms * x + 0.5 * i * i * dle) / ts.mms;
    [di, di2, dv, v]
}

/// The same equations by classical Runge–Kutta at `over`× the rate,
/// the input the exact sine: excursion and acceleration at each output
/// sample (the driver's first sample is the input at t = 0).
fn reference(ts: ThieleSmall, large: LargeSignal, volts: f64, hz: f64, samples: usize, over: usize) -> (Vec<f64>, Vec<f64>) {
    let dt = 1.0 / (RATE * over as f64);
    let u = |t: f64| volts * (TAU * hz * t).sin();
    let f = |s: &[f64; 4], t: f64| equations(&ts, &large, s, u(t));
    let (mut x, mut a) = (Vec::with_capacity(samples), Vec::with_capacity(samples));
    let mut s = [0.0f64; 4];
    let add = |s: &[f64; 4], k: &[f64; 4], c: f64| [s[0] + c * k[0], s[1] + c * k[1], s[2] + c * k[2], s[3] + c * k[3]];
    x.push(0.0);
    a.push(f(&s, 0.0)[2]);
    for n in 1..samples {
        for m in 0..over {
            let t = ((n - 1) * over + m) as f64 * dt;
            let k1 = f(&s, t);
            let k2 = f(&add(&s, &k1, dt / 2.0), t + dt / 2.0);
            let k3 = f(&add(&s, &k2, dt / 2.0), t + dt / 2.0);
            let k4 = f(&add(&s, &k3, dt), t + dt);
            for j in 0..4 {
                s[j] += dt / 6.0 * (k1[j] + 2.0 * k2[j] + 2.0 * k3[j] + k4[j]);
            }
        }
        x.push(s[3]);
        a.push(f(&s, n as f64 / RATE)[2]);
    }
    (x, a)
}

/// Harmonic `k`'s magnitude in `y` (a whole number of periods).
fn harmonic(y: &[f64], hz: f64, k: usize) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &v) in y.iter().enumerate() {
        let w = TAU * hz * k as f64 * n as f64 / RATE;
        re += v * w.cos();
        im += v * w.sin();
    }
    2.0 * (re * re + im * im).sqrt() / y.len() as f64
}

#[test]
fn large_signal_matches_an_oversampled_reference() {
    let ts = twelve(0.6e-3).with_lr2(0.9e-3, 8.0);
    let large = grinding();
    let (hz, volts) = (50.0, 20.0);
    let samples = (0.5 * RATE) as usize;
    let mut d = Driver::new(ts, large);
    d.prepare(RATE as f32);
    let (mut x, mut a) = (Vec::new(), Vec::new());
    for n in 0..samples {
        let u = volts * (TAU * hz * n as f64 / RATE).sin();
        a.push(d.drive(u));
        x.push(d.excursion());
    }
    let (rx, ra) = reference(ts, large, volts, hz, samples, 32);
    let tail = samples - (0.2 * RATE) as usize;
    let peak = x[tail..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    eprintln!(
        "peak excursion {:.3} mm; Newton peak {} iterations, {} cap hits",
        peak * 1e3,
        d.peak_iterations(),
        d.cap_hits()
    );
    assert_eq!(d.cap_hits(), 0);
    assert!(peak > 1.5e-3, "the drive reaches the curves: {peak}");
    for (name, y, r) in [("excursion", &x, &rx), ("acceleration", &a, &ra)] {
        let (y, r) = (&y[tail..], &r[tail..]);
        let h1 = harmonic(r, hz, 1);
        let mut worst = 0.0f64;
        for k in 2..=5 {
            let (got, want) = (harmonic(y, hz, k) / h1, harmonic(r, hz, k) / h1);
            eprintln!("  {name} H{k}: {:.2} dBc (reference {:.2})", db(got), db(want));
            if want > 1e-4 {
                worst = worst.max(db(got / want).abs());
            }
        }
        let err = y.iter().zip(r).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt()
            / r.iter().map(|b| b * b).sum::<f64>().sqrt();
        eprintln!("  {name}: harmonics within {worst:.4} dB, waveform {err:.2e} relative");
        assert!(worst < 0.05, "{name}: {worst} dB");
        assert!(err < 1e-3, "{name}: {err}");
    }
}

#[test]
fn klippel_symptoms() {
    let ts = twelve(0.6e-3);
    let hz = 40.0;
    let run = |large: LargeSignal, volts: f64| {
        let mut d = Driver::new(ts, large);
        d.prepare(RATE as f32);
        let n = (1.0 * RATE) as usize;
        let tail = n - (0.5 * RATE) as usize;
        let mut x = Vec::new();
        let mut a = Vec::new();
        for k in 0..n {
            let acc = d.drive(volts * (TAU * hz * k as f64 / RATE).sin());
            if k >= tail {
                x.push(d.excursion());
                a.push(acc);
            }
        }
        let mean = x.iter().sum::<f64>() / x.len() as f64;
        (x, a, mean)
    };
    // Symmetric curves: odd harmonics only, no DC.
    let symmetric = LargeSignal::from_limits(1.5e-3, 1.2e-3, 6e-3);
    let (x, _, mean) = run(symmetric, 20.0);
    let h1 = harmonic(&x, hz, 1);
    let (h2, h3) = (harmonic(&x, hz, 2) / h1, harmonic(&x, hz, 3) / h1);
    eprintln!("symmetric: H2 {:.1} dBc, H3 {:.1} dBc, DC {:.2e} m", db(h2), db(h3), mean);
    assert!(h2 < 1e-9 && mean.abs() < 1e-12 && h3 > 1e-3);
    // A suspension stiffer outward pushes the coil inward: a DC
    // displacement toward the softer side (Klippel 2006, §3.2).
    let mut stiff_out = LargeSignal::LINEAR;
    stiff_out.kms = Polynomial::new(&[1.0, 150.0, 1.0e5]);
    stiff_out.range = 6e-3;
    let (x, _, mean) = run(stiff_out, 20.0);
    let h2 = harmonic(&x, hz, 2) / harmonic(&x, hz, 1);
    eprintln!("stiffer outward: DC {:.3} mm, H2 {:.1} dBc", mean * 1e3, db(h2));
    assert!(mean < -1e-6 && h2 > 1e-3);
    // Compression: the fundamental falls behind the drive.
    let (_, small, _) = run(symmetric, 1.0);
    let (_, big, _) = run(symmetric, 20.0);
    let compression = db(harmonic(&big, hz, 1) / (20.0 * harmonic(&small, hz, 1)));
    eprintln!("20 V at {hz} Hz compresses {compression:.2} dB");
    assert!(compression < -0.5);
}

#[test]
fn polynomial_derivatives_are_exact() {
    let p = Polynomial::new(&[1.0, -2.0, 3.0, 0.5]);
    for x in [-0.3, 0.0, 0.7] {
        let (v, d1, d2) = p.eval(x);
        assert!((v - (1.0 - 2.0 * x + 3.0 * x * x + 0.5 * x * x * x)).abs() < 1e-14);
        assert!((d1 - (-2.0 + 6.0 * x + 1.5 * x * x)).abs() < 1e-14);
        assert!((d2 - (6.0 + 3.0 * x)).abs() < 1e-14);
    }
    let r = Polynomial::relative(&[5.0, 10.0]);
    assert_eq!(r.coefficients(), &[1.0, 2.0]);
}

#[test]
fn newton_matches_the_linear_step_without_curves() {
    // A curve that is 1 everywhere but not `ONE` sends the step through
    // Newton; it must land where the fixed linear update does.
    let ts = twelve(0.6e-3).with_lr2(0.9e-3, 8.0);
    let mut flat = LargeSignal::LINEAR;
    flat.bl = Polynomial::new(&[1.0, 0.0]);
    let (mut a, mut b) = (Driver::new(ts, LargeSignal::LINEAR), Driver::new(ts, flat));
    a.prepare(RATE as f32);
    b.prepare(RATE as f32);
    let mut worst = 0.0f64;
    for n in 0..4_800 {
        let u = 3.0 * (TAU * 97.0 * n as f64 / RATE).sin() + (TAU * 2_300.0 * n as f64 / RATE).sin();
        let (ya, yb) = (a.drive(u), b.drive(u));
        worst = worst.max((ya - yb).abs() / (1.0 + ya.abs()));
    }
    assert!(worst < 1e-9, "{worst}");
    assert!(b.peak_iterations() <= 3);
}
