//! The Hilbert pair measured on tones: 90° apart and equal in level
//! across the band at every rate, as the design promises.

use super::Hilbert;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::probe::phasor;
use crate::units::hz;
use std::f64::consts::TAU;

/// The paths' phase difference (degrees, `re` ahead of `im`) and level
/// difference (dB) on a settled tone at `f`.
fn quadrature(h: &mut Hilbert, fs: f64, f: f64) -> (f64, f64) {
    h.prepare(fs as f32);
    let settle = fs as usize;
    let measure = ((0.05 * f).ceil().max(4.0) * fs / f).round() as usize;
    let (mut re, mut im) = (Vec::with_capacity(measure), Vec::with_capacity(measure));
    for i in 0..settle + measure {
        let (a, b) = h.analytic_f64((TAU * f * i as f64 / fs).sin());
        if i >= settle {
            re.push(a);
            im.push(b);
        }
    }
    let (p, q) = (phasor(&re, fs, f), phasor(&im, fs, f));
    let angle = (p.1.atan2(p.0) - q.1.atan2(q.0)).to_degrees();
    let level = 10.0 * ((p.0 * p.0 + p.1 * p.1) / (q.0 * q.0 + q.1 * q.1)).log10();
    ((angle + 540.0).rem_euclid(360.0) - 180.0, level)
}

#[test]
fn quadrature_holds_across_the_band() {
    for fs in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut h = Hilbert::default();
        h.prepare(fs as f32);
        let ripple = h.phase_error_degrees();
        let (mut worst, mut worst_level) = (0.0f64, 0.0f64);
        for k in 0..=30 {
            let f = 20.0 * 1000f64.powf(f64::from(k) / 30.0);
            let (angle, level) = quadrature(&mut h, fs, f);
            worst = worst.max((angle - 90.0).abs());
            worst_level = worst_level.max(level.abs());
        }
        println!(
            "Hilbert at {fs}: {} sections, design ripple {ripple:.4}°, measured {worst:.4}°, level {worst_level:.2e} dB",
            h.sections()
        );
        assert!(ripple <= 0.5, "{fs}: design {ripple}°");
        assert!(
            worst <= ripple + 0.02,
            "{fs}: {worst}° against the design's {ripple}°"
        );
        assert!(worst_level < 1e-6, "{fs}: paths differ by {worst_level} dB");
    }
}

#[test]
fn a_tighter_budget_buys_sections_and_meets_itself() {
    let mut h = Hilbert::new(hz(20.0), 0.05);
    h.prepare(48_000.0);
    assert!(h.sections() > Hilbert::default().sections());
    assert!(h.phase_error_degrees() <= 0.05);
    let worst = [20.0, 100.0, 1_000.0, 10_000.0, 23_000.0]
        .iter()
        .map(|&f| (quadrature(&mut h, 48_000.0, f).0 - 90.0).abs())
        .fold(0.0, f64::max);
    assert!(worst <= 0.05 + 0.005, "{worst}°");
}

#[test]
fn tail_covers_the_slowest_section() {
    let mut h = Hilbert::default();
    h.prepare(48_000.0);
    let Tail::Samples(n) = h.tail() else { panic!() };
    h.tick(1.0);
    let last = (1..2 * n)
        .filter(|_| h.analytic(0.0).0.abs() > 1e-6)
        .last()
        .unwrap_or(0);
    assert!(last <= n, "rang {last} past {n}");
}
