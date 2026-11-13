use super::*;
use crate::units::{ms, seconds, semitones};
use crate::Stage;

const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];

#[test]
fn grains_follow_the_density() {
    for rate in RATES {
        for (density, size_ms) in [(5.0, 20.0), (20.0, 10.0), (50.0, 5.0), (100.0, 5.0), (200.0, 2.0)] {
            let mut g = Granulator::new(seconds(1.0), 16);
            g.set_density(density);
            g.set_size(ms(size_ms));
            g.set_position(ms(10.0));
            g.prepare(rate);
            // Fill the buffer, then count over one second of DC.
            for _ in 0..rate as usize / 10 {
                g.tick(1.0);
            }
            let n = rate as usize;
            let mut onsets = 0usize;
            let mut last = 0.0f32;
            for _ in 0..n {
                let y = g.tick(1.0);
                if last <= 1e-9 && y > 1e-9 {
                    onsets += 1;
                }
                last = y;
            }
            let want = density as usize;
            assert!(onsets.abs_diff(want) <= 1, "{rate} {density}/s: {onsets} grains");
            assert_eq!(g.dropped(), 0);
        }
    }
}

#[test]
fn grains_follow_the_pitch() {
    let mut worst = 0.0f64;
    for rate in RATES {
        let fs = f64::from(rate);
        for shift in [-12.0f32, -5.0, 0.0, 7.0, 12.0] {
            let mut g = Granulator::new(seconds(1.0), 8);
            g.set_density(10.0); // isolated grains
            g.set_size(ms(80.0));
            g.set_pitch(semitones(shift));
            g.set_position(ms(200.0));
            g.prepare(rate);
            let f = 440.0;
            let y: Vec<f32> = (0..(1.2 * fs) as usize)
                .map(|t| g.tick((core::f64::consts::TAU * f * t as f64 / fs).sin() as f32))
                .collect();
            // Each grain: zero crossings across its middle 60 %.
            let length = (0.08 * fs).round() as usize;
            let every = fs / 10.0;
            let want = f * f64::from(shift / 12.0).exp2();
            for k in 3..11 {
                let onset = (k as f64 * every).ceil() as usize;
                let seg = &y[onset + length / 5..onset + 4 * length / 5];
                let crossings: Vec<f64> = (1..seg.len())
                    .filter(|&i| seg[i - 1] < 0.0 && seg[i] >= 0.0)
                    .map(|i| i as f64 - 1.0 + f64::from(seg[i - 1] / (seg[i - 1] - seg[i])))
                    .collect();
                let (a, b) = (crossings[0], crossings[crossings.len() - 1]);
                let got = (crossings.len() - 1) as f64 * fs / (b - a);
                let c = 1200.0 * (got / want).log2();
                worst = worst.max(c.abs());
                assert!(c.abs() < 0.5, "{rate} {shift} st grain {k}: {c} cents");
            }
        }
    }
    println!("Granulator pitch: worst {worst:.4} cents");
}

#[test]
fn coherent_grains_are_unity() {
    for rate in RATES {
        let mut g = Granulator::new(seconds(1.0), 16);
        g.set_density(100.0);
        g.set_size(ms(50.0));
        g.set_position(ms(20.0));
        g.prepare(rate);
        let y: Vec<f32> = (0..rate as usize / 2).map(|_| g.tick(1.0)).collect();
        let settled = &y[y.len() / 2..];
        let off = settled.iter().map(|v| (v - 1.0).abs()).fold(0.0f32, f32::max);
        assert!(off < 1e-3, "{rate}: {off} from unity");
    }
}

#[test]
fn a_full_pool_drops_and_counts() {
    let mut g = Granulator::new(seconds(1.0), 2);
    g.set_density(100.0);
    g.set_size(ms(50.0));
    g.prepare(48_000.0);
    for _ in 0..48_000 {
        g.tick(0.5);
    }
    assert!(g.dropped() > 0);
    assert!(g.active() <= 2);
}
