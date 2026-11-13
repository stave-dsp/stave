use super::*;
use crate::kernel::node::Stage;
use crate::physical::rotary::SPEED_OF_SOUND;

const RATE: f64 = 48_000.0;

/// The stage's impulse response, `len` samples.
fn impulse(stage: &mut impl Stage, len: usize) -> Vec<f64> {
    (0..len).map(|n| f64::from(stage.tick(if n == 0 { 1.0 } else { 0.0 }))).collect()
}

#[test]
fn room_reflections_follow_the_images() {
    let shoebox = Shoebox::LIVE_ROOM;
    let mut room = Room::new(shoebox);
    room.prepare(RATE as f32);
    room.set_late_gain(0.0);
    let y = impulse(&mut room, 4_800);
    let direct = shoebox.direct();
    let taps: Vec<(f64, f64)> = shoebox
        .images()
        .iter()
        .map(|&(path, beta)| ((path - direct) / SPEED_OF_SOUND * RATE, beta * direct / path))
        .collect();
    let mut checked = 0;
    for &(d, g) in &taps {
        // Images within the sinc's reach of another overlap: skip them.
        if taps.iter().filter(|(e, _)| (e - d).abs() < 24.0).count() > 1 {
            continue;
        }
        // The tap's kernel sums to one and is symmetric about its
        // delay: the window's sum is its gain, its centroid its arrival.
        let (lo, hi) = ((d - 12.0).floor() as usize, (d + 12.0).ceil() as usize);
        let sum: f64 = y[lo..=hi].iter().sum();
        let centroid = (lo..=hi).map(|k| k as f64 * y[k]).sum::<f64>() / sum;
        eprintln!("image {d:.3} samples after the direct sound: gain {sum:.6} (geometry {g:.6}), at {centroid:.4}");
        assert!((sum / g - 1.0).abs() < 1e-4, "image at {d:.2}: {sum} vs {g}");
        assert!((centroid - d).abs() < 5e-3, "image at {d:.3}: {centroid}");
        checked += 1;
    }
    assert!(checked >= 4, "{checked}");
    assert!((y[0] - 1.0).abs() < 1e-6, "the direct sound passes");
}

#[test]
fn room_tail_follows_sabine_and_kuttruff() {
    let shoebox = Shoebox::LIVE_ROOM;
    let mut room = Room::new(shoebox);
    room.prepare(RATE as f32);
    room.set_early(false);
    let t60 = shoebox.t60();
    let n = (1.2 * t60 * RATE) as usize;
    let y = impulse(&mut room, n);
    let late: Vec<f64> = y[1..].to_vec();
    // Schroeder backward integration; T30 from −5 to −35 dB.
    let mut edc: Vec<f64> = late.iter().rev().scan(0.0, |acc, v| { *acc += v * v; Some(*acc) }).collect();
    edc.reverse();
    let e0 = edc[0];
    let level = |db: f64| edc.iter().position(|&e| 10.0 * (e / e0).log10() <= db).expect("decays") as f64 / RATE;
    let t30 = 2.0 * (level(-35.0) - level(-5.0));
    let energy: f64 = late.iter().map(|v| v * v).sum();
    let want = (shoebox.direct() / shoebox.critical_distance()).powi(2);
    eprintln!("Sabine T60 {t60:.3} s, measured T30 {t30:.3} s; late energy {energy:.4} vs Kuttruff {want:.4}");
    assert!((t30 / t60 - 1.0).abs() < 0.05);
    assert!((energy / want - 1.0).abs() < 0.10);
}
