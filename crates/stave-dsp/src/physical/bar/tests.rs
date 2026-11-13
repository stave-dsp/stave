use super::*;
use crate::physical::probe::{cents, frequency, hann};
use crate::units::{hz, seconds};

const FS: f32 = 48_000.0;

/// The free–free and clamped–free roots of beam theory (Rayleigh;
/// Fletcher & Rossing table 2.2, to the digits they print, here to 9).
const FREE: [f64; 5] = [4.730_040_745, 7.853_204_624, 10.995_607_838, 14.137_165_491, 17.278_759_657];
const CLAMPED: [f64; 4] = [1.875_104_069, 4.694_091_133, 7.854_757_438, 10.995_540_735];

fn ring(t60: f64) -> Loss {
    Loss::two_point(hz(0.0), seconds(t60), hz(1.0), seconds(t60)).unwrap()
}

fn bar(boundary: Boundary, f1: f32) -> Bar {
    let beam = Beam::tuned(Material::ALUMINIUM, 0.006, boundary, hz(f1));
    let mut b = Bar::new(beam, boundary, ring(8.0));
    b.set_bandwidth(hz(20_000.0));
    b.prepare(FS);
    b
}

fn impulse(b: &mut Bar, len: usize) -> Vec<f64> {
    b.reset();
    (0..len).map(|i| f64::from(b.tick(if i == 0 { 1.0 } else { 0.0 }))).collect()
}

#[test]
fn roots_match_beam_theory() {
    let free = roots(Boundary::Free, 40);
    let clamped = roots(Boundary::Clamped, 40);
    for (got, want) in free.iter().zip(FREE).chain(clamped.iter().zip(CLAMPED)) {
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
    }
    // Beyond, (n + ½)π and (n − ½)π to within sech λ.
    for (i, (f, c)) in free.iter().zip(&clamped).enumerate().skip(5) {
        let n = i as f64 + 1.0;
        assert!((f - (n + 0.5) * core::f64::consts::PI).abs() < 1e-6);
        assert!((c - (n - 0.5) * core::f64::consts::PI).abs() < 1e-6);
    }
}

#[test]
fn shapes_are_orthonormal() {
    for boundary in [Boundary::Free, Boundary::Clamped] {
        let r = roots(boundary, 30);
        let n = 20_000;
        let mut worst = 0.0f64;
        for (i, &a) in r.iter().enumerate().step_by(3) {
            for &b in &r[i..] {
                // Simpson's rule on ∫₀¹ φ_a φ_b dξ.
                let f = |k: usize| {
                    let x = k as f64 / n as f64;
                    shape(boundary, a, x) * shape(boundary, b, x)
                };
                let mut sum = f(0) + f(n);
                for k in 1..n {
                    sum += f(k) * if k % 2 == 1 { 4.0 } else { 2.0 };
                }
                let integral = sum / (3.0 * n as f64);
                let want = if a == b { 1.0 } else { 0.0 };
                worst = worst.max((integral - want).abs());
            }
        }
        assert!(worst < 1e-6, "{boundary:?}: {worst:e}");
        // The ends: a free end moves twice as far as the mean, a clamp
        // not at all.
        match boundary {
            Boundary::Free => assert!((shape(boundary, r[0], 0.0).abs() - 2.0).abs() < 1e-9),
            Boundary::Clamped => assert!(shape(boundary, r[0], 0.0).abs() < 1e-9),
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn mode_ratios_ring_as_beam_theory_says() {
    let fs = f64::from(FS);
    for (boundary, f1, table) in [(Boundary::Free, 440.0, &FREE[..]), (Boundary::Clamped, 220.0, &CLAMPED[..])] {
        let mut b = bar(boundary, f1);
        let first = b.modes()[0].hz();
        assert!((first / f64::from(f1) - 1.0).abs() < 1e-12, "tuned to {first}");
        let y = impulse(&mut b, 2 * FS as usize);
        let mut worst = 0.0f64;
        for (mode, lambda) in b.modes().iter().zip(table) {
            let ratio = (lambda / table[0]).powi(2);
            assert!((mode.hz() / first / ratio - 1.0).abs() < 1e-8, "{mode:?}");
            if mode.hz() > 0.4 * fs {
                break;
            }
            let damped = (mode.natural * mode.natural - mode.sigma * mode.sigma).sqrt() / core::f64::consts::TAU;
            let got = frequency(&y, fs, damped, 2_400, 2_400 + 24_000, 24_000);
            worst = worst.max(cents(got, damped).abs());
        }
        eprintln!("Bar ({boundary:?}): modes ring within {worst:.5} cents of (λ_n/λ_1)²");
        assert!(worst < 0.01, "{boundary:?}: {worst} cents");
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_strike_on_a_node_leaves_its_mode_silent() {
    // Mode 2 of a free bar (λ = 7.853) has its central node at ξ = ½;
    // strike there and it is silent while modes 1 and 3 ring.
    let mut b = bar(Boundary::Free, 440.0);
    b.set_drive(0.5);
    let fs = f64::from(FS);
    let y = impulse(&mut b, FS as usize);
    let at = |i: usize| hann(&y, fs, b.modes()[i].hz(), 0, FS as usize).0;
    let (one, two, three) = (at(0), at(1), at(2));
    eprintln!("Bar struck at its centre: modes 1–3 at {:.1}, {:.1}, {:.1} dB", 20.0 * one.log10(), 20.0 * two.log10(), 20.0 * three.log10());
    assert!(two < 1e-5 * one.min(three), "mode 2 {two:e} against {one:e}, {three:e}");
}

#[test]
fn block_and_tick_agree_bit_for_bit() {
    let mut a = bar(Boundary::Free, 880.0);
    let mut b = a.clone();
    let input: Vec<f32> = (0..1_000).map(|i| (((i * 7919) % 211) as f32 / 105.0 - 1.0) * 0.5).collect();
    let ticked: Vec<f32> = input.iter().map(|&x| a.tick(x)).collect();
    let mut blocked = input.clone();
    for chunk in blocked.chunks_mut(29) {
        b.process(chunk);
    }
    assert_eq!(ticked, blocked);
}
