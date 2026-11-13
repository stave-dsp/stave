use super::*;
use crate::physical::probe::{cents, frequency, hann, peak};
use crate::units::hz;
use core::f64::consts::PI;

const FS: f32 = 48_000.0;

fn clarinet() -> Bore {
    Bore { length: 0.6, radius: 0.0075, shape: Shape::Cylinder }
}

/// The passive bore (no valve flow) heard at the mouthpiece after a
/// flow impulse.
fn input_response(bore: Bore, len: usize) -> Vec<f64> {
    let mut t: Waveguide = Waveguide::new(bore, Valve::Reed);
    t.set_embouchure(0.0);
    t.set_output(Output::Mouthpiece);
    t.prepare(FS);
    (0..len).map(|i| f64::from(t.tick(if i == 0 { 1.0 } else { 0.0 }))).collect()
}

/// The resonances an impulse response rings at, near each theory value.
fn resonances(y: &[f64], theory: &[f64]) -> Vec<f64> {
    let fs = f64::from(FS);
    theory
        .iter()
        .map(|&f| {
            let near = peak(y, fs, f, 0.05 * f, 0, 4_800);
            frequency(y, fs, near, 240, 240 + 1_200, 4_800)
        })
        .collect()
}

#[test]
#[cfg_attr(miri, ignore)]
fn cylinder_resonates_at_odd_harmonics() {
    let bore = clarinet();
    let l = bore.length + 0.6133 * bore.radius;
    let theory: Vec<f64> = (1..=6).map(|n| (2.0 * f64::from(n) - 1.0) * SPEED_OF_SOUND / (4.0 * l)).collect();
    let y = input_response(bore, 12_000);
    let got = resonances(&y, &theory);
    let worst = got.iter().zip(&theory).map(|(g, t)| cents(*g, *t).abs()).fold(0.0, f64::max);
    eprintln!("Cylinder L = 0.6 m, a = 7.5 mm: resonances 1–6 {:?} Hz, within {worst:.2} cents of (2n − 1)c/4(L + 0.6133a)", got.iter().map(|f| f.round()).collect::<Vec<_>>());
    assert!(worst < 10.0, "{worst} cents");
    // The even harmonics are anti-resonances: far weaker.
    let fs = f64::from(FS);
    let odd = hann(&y, fs, theory[0], 0, 4_800).0;
    let even = hann(&y, fs, 2.0 * theory[0], 0, 4_800).0;
    assert!(even < 0.2 * odd, "even {even} against odd {odd}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn cone_resonates_where_its_truncation_says() {
    let mut report = Vec::new();
    for apex in [0.02, 0.08] {
        let bore = Bore { length: 0.6, radius: 0.02, shape: Shape::Cone { apex } };
        let l = bore.length + 0.6133 * bore.radius;
        // Roots of tan kL′ = −k x₀, written independently: bisect
        // sin kL′ + k x₀ cos kL′ on ((n − ½)π, nπ)/L′.
        let theory: Vec<f64> = (1..=6)
            .map(|n| {
                let g = |k: f64| (k * l).sin() + k * apex * (k * l).cos();
                let (mut lo, mut hi) = ((f64::from(n) - 0.5) * PI / l, f64::from(n) * PI / l);
                for _ in 0..100 {
                    let mid = 0.5 * (lo + hi);
                    if g(mid) * g(lo) > 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi) * SPEED_OF_SOUND / (2.0 * PI)
            })
            .collect();
        for (n, t) in theory.iter().enumerate() {
            assert!((bore.resonance(n as u32 + 1) / (2.0 * PI) / t - 1.0).abs() < 1e-9);
        }
        let y = input_response(bore, 12_000);
        let got = resonances(&y, &theory);
        let worst = got.iter().zip(&theory).map(|(g, t)| cents(*g, *t).abs()).fold(0.0, f64::max);
        report.push((apex, got, theory, worst));
    }
    for (apex, got, theory, worst) in &report {
        eprintln!(
            "Cone x₀ = {apex} m: resonances {:?} Hz against tan kL′ = −kx₀ {:?}, within {worst:.2} cents",
            got.iter().map(|f| f.round()).collect::<Vec<_>>(),
            theory.iter().map(|f| f.round()).collect::<Vec<_>>()
        );
    }
    for (apex, _, _, worst) in report {
        assert!(worst < 10.0, "x₀ = {apex}: {worst} cents");
    }
}

/// Blow `tube` for a second; the last half's level and pitch (the
/// strongest peak near `near` Hz).
fn blown(tube: &mut Waveguide, near: f64) -> (f64, f64) {
    tube.set_output(Output::Mouthpiece);
    tube.prepare(FS);
    let y: Vec<f64> = (0..FS as usize).map(|_| f64::from(tube.tick(0.0))).collect();
    let tail = &y[FS as usize / 2..];
    let mean = tail.iter().sum::<f64>() / tail.len() as f64;
    let rms = (tail.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / tail.len() as f64).sqrt();
    let fs = f64::from(FS);
    let f = peak(tail, fs, near, 0.1 * near, 0, tail.len());
    (rms, f)
}

#[test]
#[cfg_attr(miri, ignore)]
fn reed_blows_at_the_first_resonance() {
    let bore = clarinet();
    let f1 = bore.resonance(1) / (2.0 * PI);
    let mut quiet: Waveguide = Waveguide::new(bore, Valve::Reed);
    quiet.set_embouchure(0.3);
    quiet.set_blow(0.25);
    let (below, _) = blown(&mut quiet, f1);
    let mut loud: Waveguide = Waveguide::new(bore, Valve::Reed);
    loud.set_embouchure(0.3);
    loud.set_blow(0.5);
    let (above, pitch) = blown(&mut loud, f1);
    eprintln!("Reed: γ = 0.25 rms {below:.2e}, γ = 0.5 rms {above:.3} at {pitch:.2} Hz ({:+.1} cents from the first resonance)", cents(pitch, f1));
    assert!(below < 1e-4, "it sounds below threshold: {below}");
    assert!(above > 0.1, "silent above threshold: {above}");
    assert!(cents(pitch, f1).abs() < 20.0);
    assert!(loud.iterations() <= 40);
}

#[test]
#[cfg_attr(miri, ignore)]
fn lips_lock_to_the_nearest_resonance() {
    // Outward-striking lips sound above their own resonance: tuned a
    // little below a bore resonance, they lock to it.
    let bore = Bore { length: 1.4, radius: 0.006, shape: Shape::Cylinder };
    let mut report = Vec::new();
    for n in [2u32, 3, 4] {
        let res = bore.resonance(n) / (2.0 * PI);
        let mut brass: Waveguide = Waveguide::new(bore, Valve::Lip { frequency: hz((0.9 * res) as f32), q: 20.0 });
        brass.set_embouchure(0.1);
        brass.set_blow(0.6);
        let (level, pitch) = blown(&mut brass, res);
        report.push((n, res, level, cents(pitch, res)));
    }
    for (n, res, level, off) in &report {
        eprintln!("Lips at 0.9 × resonance {n} ({res:.1} Hz): rms {level:.3}, sounding {off:+.1} cents from it");
    }
    for (n, _, level, off) in report {
        assert!(level > 0.05, "resonance {n}: silent ({level})");
        assert!(off.abs() < 25.0, "resonance {n}: {off} cents");
    }
}
