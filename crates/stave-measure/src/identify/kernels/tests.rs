//! Exact recovery on systems whose kernels are known in closed form: a
//! generalised Hammerstein system (filters around powers), a memoryless
//! polynomial (flat kernels) and a Wiener system (a filter before a
//! polynomial: Volterra diagonals `c·L(f/m)ᵐ`, Hammerstein kernels that
//! move with level). Each is run 4× oversampled on the band-limited
//! sweep and brought back, as a measurement through an anti-aliased
//! interface would see it.

use super::*;
use crate::identify::hammerstein::stimulus;
use crate::identify::periodic::resample;

const RATE: f64 = 48_000.0;
const UP: f64 = 4.0 * RATE;

/// A bilinear one-pole at `hz` (low- or high-pass) at the 4× rate.
#[derive(Copy, Clone)]
struct OnePole {
    b0: f64,
    b1: f64,
    a1: f64,
    gain: f64,
}

impl OnePole {
    fn new(high: bool, hz: f64, gain: f64) -> Self {
        let k = (PI * hz / UP).tan();
        let a1 = (k - 1.0) / (k + 1.0);
        let (b0, b1) = if high { (1.0 / (1.0 + k), -1.0 / (1.0 + k)) } else { (k / (1.0 + k), k / (1.0 + k)) };
        Self { b0, b1, a1, gain }
    }

    fn response(&self, hz: f64) -> Complex<f64> {
        let w = TAU * hz / UP;
        let z1 = Complex::new(w.cos(), -w.sin());
        let num = Complex::new(self.b0, 0.0) + z1.scale(self.b1);
        let den = Complex::new(1.0, 0.0) + z1.scale(self.a1);
        divide(num, den).scale(self.gain)
    }

    /// Filter `x` (direct form I, from rest).
    fn run(&self, x: &[f64]) -> Vec<f64> {
        let (mut x1, mut y1) = (0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = self.b0 * v + self.b1 * x1 - self.a1 * y1;
                x1 = v;
                y1 = y;
                self.gain * y
            })
            .collect()
    }
}

/// `system` run at 4× on the sweep's stimulus, brought back to `RATE`.
fn measure(sweep: &Sweep, order: usize, system: &dyn Fn(&[f64]) -> Vec<f64>) -> Kernels {
    let x = stimulus(sweep, RATE, 0.01, 0.2);
    let up = resample(&x, 4 * x.len());
    let y = resample(&system(&up), x.len());
    Kernels::measure(sweep, RATE, &y, order).expect("a synchronized sweep")
}

fn sweep(amplitude: f64) -> Sweep {
    Sweep::synchronized(LOW, 0.47 * RATE, 2.0, amplitude)
}

/// The sweep's start, and the band kernels are held to their truth over:
/// from 20 times it (the taper passes a quarter at `2·order·f₁`).
const LOW: f64 = 5.0;
const BAND: (f64, f64) = (100.0, 18_000.0);
/// Above 1 kHz, where the low edge's leakage is gone.
const HIGH: (f64, f64) = (1_000.0, 18_000.0);

/// The worst `|kernel − truth| / peak(truth)` over `band`, and where.
fn worst(kernel: &Kernel, truth: impl Fn(f64) -> Complex<f64>, band: (f64, f64)) -> (f64, f64) {
    let inside: Vec<(f64, Complex<f64>)> = kernel.points.iter().copied().filter(|p| (band.0..band.1).contains(&p.0)).collect();
    let peak = inside.iter().fold(0.0f64, |p, (f, _)| p.max(truth(*f).norm()));
    inside
        .iter()
        .map(|&(f, v)| ((v - truth(f)).norm() / peak, f))
        .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
}

/// The worst error of kernel `m` as its contribution to harmonic `m` at
/// amplitude `a`, `|ΔGₘ|·(a/2)^{m−1}`, re `linear` (the linear
/// response's peak) over [`BAND`]: what a report reads in dBc.
fn contribution(kernel: &Kernel, truth: impl Fn(f64) -> Complex<f64>, a: f64, linear: f64) -> f64 {
    let scale = (0.5 * a).powi(kernel.order as i32 - 1) / linear;
    kernel
        .points
        .iter()
        .filter(|p| (BAND.0..BAND.1).contains(&p.0))
        .map(|&(f, v)| (v - truth(f)).norm() * scale)
        .fold(0.0, f64::max)
}

#[test]
fn a_generalised_hammerstein_system_comes_back() {
    let g = [OnePole::new(false, 6_000.0, 1.0), OnePole::new(true, 250.0, 0.3), OnePole::new(false, 1_500.0, -0.2)];
    let system = |x: &[f64]| {
        let mut y = vec![0.0; x.len()];
        for (m, f) in g.iter().enumerate() {
            let p: Vec<f64> = x.iter().map(|v| v.powi(m as i32 + 1)).collect();
            for (o, b) in y.iter_mut().zip(f.run(&p)) {
                *o += b;
            }
        }
        y
    };
    let levels: Vec<Kernels> = [0.2, 0.4, 0.8].iter().map(|&a| measure(&sweep(a), 3, &system)).collect();
    for k in &levels {
        for (m, f) in g.iter().enumerate() {
            let (e, at) = worst(&k.hammerstein[m], |hz| f.response(hz), BAND);
            println!("a = {}: G{} off by {e:.2e} of its peak (at {at:.0} Hz)", k.amplitude(), m + 1);
            assert!(e < 5e-3, "G{} at {}", m + 1, k.amplitude());
        }
    }
    for m in 1..=3 {
        let d = level_dependence(&levels, m, BAND).unwrap();
        println!("G{m} level dependence {d:.2e}");
        assert!(d < 3e-3);
    }
    // A Hammerstein system's Volterra diagonals are its branch filters.
    let v = Volterra::from_levels(&levels).unwrap();
    assert_eq!((v.terms.clone(), v.complete.clone()), (vec![2, 1, 1], vec![true, true, true]));
    for (m, f) in g.iter().enumerate() {
        let (e, at) = worst(&v.kernels[m], |hz| f.response(hz), BAND);
        println!("V{} off by {e:.2e} of its peak (at {at:.0} Hz)", m + 1);
        assert!(e < 1.5e-3);
    }
    // The prediction at another level is the measurement there.
    let (a, f0) = (0.4, 1_000.0);
    let p = levels[0].predict(f0, a);
    for n in 1..=3 {
        let measured = levels[1].harmonics[n - 1].at(n as f64 * f0).unwrap();
        let e = (p[n - 1].unwrap() - measured).norm() / measured.norm();
        println!("H{n} at 1 kHz predicted from a = 0.2 at a = 0.4: off by {e:.2e}");
        assert!(e < 1e-6);
    }
}

#[test]
fn a_memoryless_polynomial_has_flat_kernels() {
    let c = [0.0, 1.0, 0.25, -0.15, 0.05, 0.02];
    let curve = |x: f64| (1..c.len()).map(|m| c[m] * x.powi(m as i32)).sum::<f64>();
    let system = |x: &[f64]| x.iter().map(|&v| curve(v)).collect::<Vec<_>>();
    let levels: Vec<Kernels> = [0.25, 0.5, 1.0].iter().map(|&a| measure(&sweep(a), 5, &system)).collect();
    let flat = |m: usize| move |_: f64| Complex::new(c[m], 0.0);
    for k in &levels {
        for m in 1..=5 {
            let (e, at) = worst(&k.hammerstein[m - 1], flat(m), HIGH);
            let d = contribution(&k.hammerstein[m - 1], flat(m), k.amplitude(), 1.0);
            println!(
                "a = {}: G{m} off flat {} by {e:.2e} above 1 kHz (at {at:.0} Hz), {d:.1e} re the linear response from 100 Hz",
                k.amplitude(),
                c[m]
            );
            assert!(e < 2e-3 && d < 2e-4, "G{m}");
        }
        // Harmonic levels against the closed form of a sine through the curve.
        let truth = |n: usize| {
            let a = k.amplitude();
            let h: Vec<f64> = (1..=5)
                .map(|q| (q..=5).step_by(2).fold(Complex::new(0.0, 0.0), |s, m| s + coefficient(q, m, a).scale(c[m])).norm())
                .collect();
            20.0 * (h[n - 1] / h[0]).log10()
        };
        for n in 2..=5 {
            let dbc = k.harmonic_dbc(n, 1_000.0).unwrap();
            println!("  H{n} at 1 kHz {dbc:.4} dBc, closed form {:.4}", truth(n));
            assert!((dbc - truth(n)).abs() < 0.01, "H{n}: {dbc} vs {}", truth(n));
        }
    }
    let v = Volterra::from_levels(&levels).unwrap();
    assert_eq!(v.terms, vec![3, 2, 2, 1, 1]);
    for m in 1..=5 {
        let (e, _) = worst(&v.kernels[m - 1], flat(m), HIGH);
        let d = contribution(&v.kernels[m - 1], flat(m), 1.0, 1.0);
        println!("V{m} off flat {} by {e:.2e} above 1 kHz, {d:.1e} re the linear response at a = 1 from 100 Hz", c[m]);
        assert!(e < 3e-5 && d < 2e-4, "V{m}");
    }
}

/// A Wiener system is not a generalised Hammerstein system: its kernels
/// `Gₘ` from one level move with level (the cubic's fundamental is
/// `L(f)|L(f)|²`, not `G₃`'s), while the Volterra diagonals from three
/// levels are exact.
#[test]
fn a_wiener_system_has_volterra_diagonals_and_level_dependent_hammerstein_kernels() {
    let l = OnePole::new(false, 2_000.0, 1.0);
    let c = [0.0, 1.0, 0.3, -0.2];
    let system = |x: &[f64]| l.run(x).into_iter().map(|v| v + c[2] * v * v + c[3] * v * v * v).collect::<Vec<_>>();
    let levels: Vec<Kernels> = [0.2, 0.4, 0.8].iter().map(|&a| measure(&sweep(a), 3, &system)).collect();
    let v = Volterra::from_levels(&levels).unwrap();
    for m in 1..=3 {
        let truth = |hz: f64| {
            let r = l.response(hz / m as f64);
            (1..m).fold(r, |p, _| p * r).scale(c[m])
        };
        let (e, at) = worst(&v.kernels[m - 1], truth, BAND);
        println!("V{m} = {}·L(f/{m})^{m}: off by {e:.2e} of its peak (at {at:.0} Hz)", c[m]);
        assert!(e < 3e-3, "V{m}");
    }
    // H2 has no other order (the cubic is the highest): G2 = V2 at every level.
    let d2 = level_dependence(&levels, 2, BAND).unwrap();
    let d1 = level_dependence(&levels, 1, BAND).unwrap();
    println!("G1 level dependence {d1:.2e}, G2 {d2:.2e}");
    assert!(d2 < 3e-3);
    assert!(d1 > 2e-2, "the Wiener system's G1 should move with level: {d1:.2e}");
    // The diagonal's weak-signal harmonic level matches the measurement low down.
    let low = &levels[0];
    for n in 2..=3 {
        let (m, w) = (low.harmonic_dbc(n, 500.0).unwrap(), v.harmonic_dbc(n, 500.0, low.amplitude()).unwrap());
        println!("H{n} at 500 Hz, a = 0.2: measured {m:.3} dBc, from the diagonal {w:.3} dBc");
        assert!((m - w).abs() < 0.1);
    }
}

#[test]
fn unsynchronized_or_mismatched_sweeps_are_refused() {
    let s = Sweep::exponential(10.0, 20_000.0, 1.03, 0.5);
    let x = stimulus(&s, RATE, 0.01, 0.1);
    assert!(Kernels::measure(&s, RATE, &x, 3).is_none());
    let a = measure(&sweep(0.5), 3, &|x| x.to_vec());
    let b = measure(&Sweep::synchronized(10.0, 0.47 * RATE, 1.0, 0.25), 3, &|x| x.to_vec());
    assert!(Volterra::from_levels(&[a.clone(), b]).is_none());
    // Two equal levels cannot part the orders.
    assert!(Volterra::from_levels(&[a.clone(), a.clone()]).is_none());
    // One level: Farina's single-level diagonal, flagged incomplete.
    let one = Volterra::from_levels(&[a]).unwrap();
    assert_eq!(one.complete, vec![false, true, true]);
}
