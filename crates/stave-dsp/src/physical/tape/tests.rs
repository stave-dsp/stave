use super::*;
use crate::kernel::node::Stage;

/// Harmonic `k` of a 1 kHz tone (48 samples a cycle), amplitude.
fn harmonic(y: &[f64], k: usize) -> f64 {
    let w = std::f64::consts::TAU * k as f64 / 48.0;
    let (re, im) = y.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, v)| {
        (re + v * (w * i as f64).cos(), im + v * (w * i as f64).sin())
    });
    2.0 * (re * re + im * im).sqrt() / y.len() as f64
}

/// The record side at 48 kHz (15× inside) under a 1 kHz tone at
/// `amplitude`: (fundamental gain, H2 dBc, H3 dBc).
fn record(machine: Machine, amplitude: f64) -> (f64, f64, f64) {
    let mut rec = Oversampled::linear(Recording::new(machine), 15);
    rec.prepare(48_000.0);
    let y: Vec<f64> = (0..48 * 300)
        .map(|n| f64::from(rec.tick((amplitude * (std::f64::consts::TAU * n as f64 / 48.0).sin()) as f32)))
        .skip(48 * 200)
        .collect();
    let h1 = harmonic(&y, 1);
    let db = |h: f64| 20.0 * (h / h1).log10();
    (h1 / amplitude, db(harmonic(&y, 2)), db(harmonic(&y, 3)))
}

fn with_bias(ratio: f64) -> Machine {
    let mut m = Machine::TC260;
    m.bias.ratio = ratio;
    m
}

#[test]
fn bias_linearises_the_dead_zone() {
    // Chowdhury: unbiased, the output has a dead zone at the zero
    // crossings; the TC-260's bias (5× the signal) linearises it.
    let quiet = 10f64.powf(-12.0 / 20.0);
    let (g_off, _, h3_off) = record(with_bias(0.0), quiet);
    let (g_on, h2, h3_on) = record(Machine::TC260, quiet);
    assert!(h3_off > h3_on + 30.0, "dead zone {h3_off} vs biased {h3_on} dBc");
    assert!(g_on > 3.0 * g_off, "bias raises the sensitivity: {g_on} vs {g_off}");
    assert!(h2 < -100.0, "a symmetric loop makes no even harmonics: {h2}");
    // The playback gain makes -20 dBFS unity.
    let (g, _, _) = record(Machine::TC260, 0.1);
    assert!((20.0 * g.log10()).abs() < 0.05, "{g}");
}

#[test]
fn sensitivity_peaks_at_low_bias_and_overbias_costs_it() {
    // The classic bias curve (Camras): output rises from the dead zone
    // to a peak and falls as the bias grows past it.
    let level = 10f64.powf(-12.0 / 20.0);
    let g: Vec<f64> = [0.0, 1.0, 5.0, 8.0]
        .iter()
        .map(|&b| record(with_bias(b), level).0)
        .collect();
    assert!(g[1] > g[0] && g[1] > g[2] && g[2] > g[3], "{g:?}");
}

#[test]
fn distortion_rises_with_level() {
    let h3: Vec<f64> = [0.0, 6.0, 12.0, 18.0]
        .iter()
        .map(|db: &f64| record(Machine::TC260, 10f64.powf(db / 20.0)).2)
        .collect();
    assert!(h3.windows(2).all(|w| w[1] > w[0]), "{h3:?}");
    // 3% (-30.5 dBc) between +12 and +18 dBFS: the calibration TC260 states.
    assert!(h3[2] < -30.5 && h3[3] > -30.5, "{h3:?}");
}

#[test]
fn playback_is_realtime_and_tracks_speed() {
    let head = Machine::TC260.play;
    let mut p = Playback::new(head, Formulation::FERRIC.thickness, Machine::ips(7.5), Phase::Linear);
    p.prepare(48_000.0);
    let n = p.taps().count();
    let slow = p.loss(10e3).abs();
    p.set_speed(Machine::ips(15.0));
    assert_eq!(p.taps().count(), n);
    assert!(p.loss(10e3).abs() > slow, "faster tape loses less at 10 kHz");
    assert_eq!(p.latency(), (n as u32 - 1) / 2);
}
