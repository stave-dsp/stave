use super::*;
use crate::kernel::node::Stage;
use stave_measure::tail::t60_s;

/// Every reverb under test, by name, at T60 `t60`.
fn reverbs(t60: f32) -> Vec<(&'static str, Box<dyn Stage>)> {
    let mut householder = Fdn::new(Mixing::Householder);
    householder.set_t60_s(t60);
    let mut hadamard = Fdn::new(Mixing::Hadamard);
    hadamard.set_t60_s(t60);
    let mut plate = Dattorro::new(50.0);
    plate.set_t60_s(t60);
    plate.set_damping_hz(1e6); // open: the T60 is the broadband decay
    let mut schroeder = Schroeder::new();
    schroeder.set_t60_s(t60);
    let mut moorer = Schroeder::moorer();
    moorer.set_t60_s(t60);
    vec![
        ("Fdn (Householder)", Box::new(householder)),
        ("Fdn (Hadamard)", Box::new(hadamard)),
        ("Dattorro", Box::new(plate)),
        ("Schroeder", Box::new(schroeder)),
        ("Schroeder::moorer", Box::new(moorer)),
    ]
}

/// The relative T60 budget per reverb.
fn budget(name: &str) -> f64 {
    match name {
        // Four combs: few modes near 1 kHz, so the decay reads noisier.
        "Schroeder" => 0.05,
        _ => 0.03,
    }
}

fn impulse_response(stage: &mut dyn Stage, sample_rate: f32, seconds: f32) -> Vec<f32> {
    stage.prepare(sample_rate);
    (0..(seconds * sample_rate) as usize)
        .map(|n| stage.tick(if n == 0 { 1.0 } else { 0.0 }))
        .collect()
}

/// The response to a 2 ms Hann-windowed 1 kHz burst (the same in
/// seconds at every rate): the T60 is read in the 1 kHz region, as ISO
/// 3382 reads it per octave, not over a broadband impulse whose top
/// octave a damped or modulated loop shortens by design.
fn burst_response(stage: &mut dyn Stage, rate: f32, seconds: f32) -> Vec<f32> {
    stage.prepare(rate);
    let burst = (0.002 * rate) as usize;
    (0..(seconds * rate) as usize)
        .map(|n| {
            let x = if n < burst {
                let w = 0.5 - 0.5 * (core::f32::consts::TAU * n as f32 / burst as f32).cos();
                w * (core::f32::consts::TAU * 1_000.0 * n as f32 / rate).sin()
            } else {
                0.0
            };
            stage.tick(x)
        })
        .collect()
}

/// The measured T60 (Schroeder T30, ISO 3382, around 1 kHz) is the
/// setting.
#[test]
#[cfg_attr(miri, ignore)]
fn measured_t60_matches_the_setting() {
    let mut over = Vec::new();
    for t60 in [1.0f32, 2.5] {
        for (name, mut reverb) in reverbs(t60) {
            let h = burst_response(reverb.as_mut(), 48_000.0, t60 * 1.2 + 0.1);
            let measured = t60_s(&h, 48_000.0).expect("decays 35 dB");
            let error = measured / f64::from(t60) - 1.0;
            println!("{name:<18} T60 {t60} s: measured {measured:.3} s ({:+.2} %)", error * 100.0);
            if error.abs() >= budget(name) {
                over.push(format!("{name}: {measured:.3} s for {t60} s"));
            }
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// Both mixing matrices are orthogonal: `AᵀA = I`.
#[test]
fn the_fdn_matrix_is_unitary() {
    for mixing in [Mixing::Householder, Mixing::Hadamard] {
        let columns: Vec<[f32; 8]> = (0..8)
            .map(|j| {
                let mut e = [0.0f32; 8];
                e[j] = 1.0;
                mixing.mix(&mut e);
                e
            })
            .collect();
        for a in 0..8 {
            for b in 0..8 {
                let dot: f32 = (0..8).map(|k| columns[a][k] * columns[b][k]).sum();
                let want = if a == b { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-6, "{mixing:?} ({a},{b}): {dot}");
            }
        }
    }
}

/// At the longest decay — infinite, a frozen loop — full-scale noise
/// fed in and then stopped neither grows nor goes non-finite.
#[test]
#[cfg_attr(miri, ignore)]
fn stable_at_maximum_decay() {
    let rate = 48_000.0;
    for (name, mut reverb) in reverbs(f32::INFINITY) {
        reverb.prepare(rate);
        assert_eq!(reverb.tail(), crate::Tail::Infinite, "{name}");
        let mut lcg = 99u32;
        let mut noise = move || {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        };
        for _ in 0..rate as usize / 2 {
            let y = reverb.tick(noise());
            assert!(y.is_finite(), "{name}");
        }
        // Energy per half second once the input stops.
        let mut energies = Vec::new();
        for _ in 0..8 {
            let e: f64 = (0..rate as usize / 2)
                .map(|_| f64::from(reverb.tick(0.0)).powi(2))
                .sum();
            assert!(e.is_finite(), "{name}");
            energies.push(e);
        }
        let (first, last) = (energies[1], energies[7]);
        println!("{name:<18} frozen: energy {first:.1} → {last:.1}");
        // Never growing: each half second holds no more than the last
        // (a hair of slack for the modulation's beating).
        for pair in energies.windows(2) {
            assert!(pair[1] < pair[0] * 1.05, "{name} grew: {energies:?}");
        }
    }
}

/// The same reverb in seconds at every rate: T60 and the level of a
/// band-limited burst's response hold from 44.1 to 192 kHz.
#[test]
#[cfg_attr(miri, ignore)]
fn sample_rate_invariant() {
    let t60 = 1.0f32;
    let mut over = Vec::new();
    for (index, (name, _)) in reverbs(t60).into_iter().enumerate() {
        let mut readings = Vec::new();
        for rate in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut reverb = reverbs(t60).swap_remove(index).1;
            let h = burst_response(reverb.as_mut(), rate, 1.3);
            let measured = t60_s(&h, f64::from(rate)).expect("decays");
            let energy_per_s: f64 =
                h.iter().map(|&y| f64::from(y).powi(2)).sum::<f64>() / f64::from(rate);
            readings.push((rate, measured, 10.0 * energy_per_s.log10()));
        }
        let (_, t_ref, db_ref) = readings[1];
        for &(rate, t, db) in &readings {
            println!("{name:<18} {rate:>7} Hz: T60 {t:.3} s, level {db:+.2} dB");
            if (t / f64::from(t60) - 1.0).abs() >= budget(name) + 0.02
                || (t / t_ref - 1.0).abs() >= budget(name)
                || (db - db_ref).abs() >= 0.5
            {
                over.push(format!("{name} at {rate}: T60 {t:.3} (48 kHz {t_ref:.3}), {db:.2} dB (48 kHz {db_ref:.2})"));
            }
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// The reported tail covers the measured ring-out.
#[test]
#[cfg_attr(miri, ignore)]
fn tails_cover_the_decay() {
    for (name, mut reverb) in reverbs(0.3) {
        let h = impulse_response(reverb.as_mut(), 48_000.0, 1.0);
        let crate::Tail::Samples(n) = reverb.tail() else {
            panic!("{name}: finite T60, finite tail")
        };
        let last_loud = h.iter().rposition(|y| y.abs() > 1e-6).unwrap_or(0) as u32;
        assert!(last_loud <= n, "{name}: rang {last_loud}, reported {n}");
    }
}
