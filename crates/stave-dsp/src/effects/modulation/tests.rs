//! Image rejection: the wanted sideband at unity, the other far below,
//! measured on coherent tones (integer hertz over one-second windows).

use super::{FrequencyShifter, Sideband, SingleSideband};
use crate::kernel::node::Stage;
use crate::primitives::filters::probe::phasor;
use crate::units::hz;
use std::f64::consts::TAU;

fn level(y: &[f64], fs: f64, f: f64) -> f64 {
    let (re, im) = phasor(y, fs, f);
    10.0 * (re * re + im * im).max(1e-30).log10()
}

/// One second after one second of settling.
fn run(fs: f64, mut tick: impl FnMut(usize) -> f32) -> Vec<f64> {
    let n = fs as usize;
    (0..2 * n).map(|i| f64::from(tick(i))).skip(n).collect()
}

#[test]
fn shifter_rejects_the_image() {
    let mut worst_image = f64::MIN;
    let mut worst_gain = 0.0f64;
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        for f in [50.0, 200.0, 1_000.0, 5_000.0, 15_000.0] {
            for shift in [-1_000.0, -100.0, 100.0, 1_000.0] {
                let (wanted, image) = (f + shift, f - shift);
                let band = 20.0..fs / 2.0 - 20.0;
                if !band.contains(&wanted) || !band.contains(&image) {
                    continue;
                }
                let mut s = FrequencyShifter::new(hz(shift as f32));
                s.prepare(fs as f32);
                let y = run(fs, |i| s.tick((TAU * f * i as f64 / fs).sin() as f32));
                let (w, m) = (level(&y, fs, wanted), level(&y, fs, image));
                worst_gain = worst_gain.max(w.abs());
                worst_image = worst_image.max(m - w);
            }
        }
    }
    println!("shifter: wanted within {worst_gain:.2e} dB, image at worst {worst_image:.1} dB");
    assert!(worst_gain < 1e-3, "{worst_gain} dB");
    assert!(worst_image < -40.0, "image {worst_image} dB");
}

#[test]
fn shifter_gives_both_sidebands_at_once() {
    let fs = 48_000.0;
    let mut s = FrequencyShifter::new(hz(300.0));
    s.prepare(fs as f32);
    let (mut up, mut down) = (Vec::new(), Vec::new());
    for i in 0..96_000 {
        let (u, d) = s.sidebands((TAU * 2_000.0 * i as f64 / fs).sin() as f32);
        if i >= 48_000 {
            up.push(f64::from(u));
            down.push(f64::from(d));
        }
    }
    assert!(level(&up, fs, 2_300.0).abs() < 1e-3);
    assert!(level(&down, fs, 1_700.0).abs() < 1e-3);
    assert!(level(&down, fs, 2_300.0) < -40.0);
}

#[test]
fn single_sideband_keeps_one_sideband() {
    let fs = 48_000.0;
    let carrier: f64 = 1_000.0;
    let mut worst = f64::MIN;
    for f in [50.0, 300.0, 700.0, 3_000.0, 8_000.0] {
        for (sideband, kept, dropped) in [
            (Sideband::Upper, carrier + f, (carrier - f).abs()),
            (Sideband::Lower, (carrier - f).abs(), carrier + f),
        ] {
            let mut m = SingleSideband::new(sideband);
            m.prepare(fs as f32);
            let y = run(fs, |i| {
                let t = i as f64 / fs;
                m.modulate((TAU * f * t).sin() as f32, (TAU * carrier * t).sin() as f32)
            });
            let (k, d) = (level(&y, fs, kept), level(&y, fs, dropped));
            assert!(k.abs() < 1e-3, "{sideband:?} {f} Hz: kept at {k} dB");
            worst = worst.max(d - k);
        }
    }
    println!("single sideband: the other sideband at worst {worst:.1} dB");
    assert!(worst < -40.0, "{worst} dB");
}
