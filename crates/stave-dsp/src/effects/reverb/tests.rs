use super::*;
use crate::kernel::node::Stage;
use crate::units::{hz, seconds};
use stave_measure::tail::{t60_band_s, t60_s};
use crate::units::Seconds;
use crate::units::ms;

/// Every reverb under test, by name, at T60 `t60`.
fn reverbs(t60: f32) -> Vec<(&'static str, Box<dyn Stage>)> {
    let mut householder = Fdn::new(Mixing::Householder);
    householder.set_decay(Seconds::new(f64::from(t60)));
    let mut hadamard = Fdn::new(Mixing::Hadamard);
    hadamard.set_decay(Seconds::new(f64::from(t60)));
    let mut plate = Dattorro::new(ms(50.0));
    plate.set_t60(Seconds::new(f64::from(t60)));
    plate.set_damping(hz(1e6)); // open: the T60 is the broadband decay
    let mut schroeder = Schroeder::new();
    schroeder.set_t60(Seconds::new(f64::from(t60)));
    let mut moorer = Schroeder::moorer();
    moorer.set_t60(Seconds::new(f64::from(t60)));
    let mut velvet = VelvetNoise::new();
    velvet.set_decay(Seconds::new(f64::from(t60)));
    let mut graphic = Fdn::new(Mixing::Householder);
    graphic.set_t60(T60::uniform(seconds(f64::from(t60))));
    let mut kronecker = Fdn::new(Mixing::Householder);
    kronecker.set_kronecker(Kronecker::hadamard());
    kronecker.set_angle_modulation(0.3, hz(0.5));
    kronecker.set_decay(Seconds::new(f64::from(t60)));
    vec![
        ("Fdn (Householder)", Box::new(householder)),
        ("Fdn (Hadamard)", Box::new(hadamard)),
        ("Fdn (graphic EQ)", Box::new(graphic)),
        ("Fdn (Kronecker)", Box::new(kronecker)),
        ("Dattorro", Box::new(plate)),
        ("Schroeder", Box::new(schroeder)),
        ("Schroeder::moorer", Box::new(moorer)),
        ("VelvetNoise", Box::new(velvet)),
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
            println!(
                "{name:<18} T60 {t60} s: measured {measured:.3} s ({:+.2} %)",
                error * 100.0
            );
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

/// Prawda et al.'s Promenadi Hall (their Table 1): a real hall's T60 per
/// octave, 3.0 s at 31.5 Hz to 1.2 s at 16 kHz.
const PROMENADI: [f64; T60::BANDS] = [3.0, 2.8, 2.68, 2.55, 2.47, 2.5, 2.3, 1.89, 1.4, 1.2];
/// A damped studio room: 1.2 s lows to 0.3 s highs, neighbouring
/// octaves within 30 % of each other, as in measured rooms.
const STUDIO: [f64; T60::BANDS] = [1.2, 1.1, 1.0, 0.9, 0.85, 0.8, 0.7, 0.55, 0.4, 0.3];
/// A steep, made-up profile (neighbours up to 57 % apart), reported but
/// not held to the budget: an octave band's measured T30 is its slowest
/// part's (the late decay is whatever decays last), so where the profile
/// changes fast inside a band the band reads long even when the filter
/// meets the centre exactly — Prawda et al.'s "extreme case" misses the
/// JND for the same reason.
const STEEP: [f64; T60::BANDS] = [0.6, 0.9, 1.4, 2.0, 2.2, 2.0, 1.6, 1.1, 0.7, 0.5];

/// The T60 budget per octave band: 5 %, the reverberation time's
/// just-noticeable difference (ISO 3382-1; Seraphim), the budget Prawda
/// et al. design to (their hall meets it at every band, 4.4 % at
/// 16 kHz).
const BAND_BUDGET: f64 = 0.05;

/// Measured T60 per octave band of `stage`'s impulse response (the mid
/// channel, through `tick`), for the bands in `bands`.
fn band_t60s(
    stage: &mut dyn Stage,
    rate: f32,
    seconds: f32,
    bands: core::ops::RangeInclusive<usize>,
) -> Vec<(usize, f64)> {
    let h = impulse_response(stage, rate, seconds);
    bands
        .map(|k| {
            let fc = f64::from(T60::CENTRES_HZ[k]);
            (
                k,
                t60_band_s(&h, f64::from(rate), fc).expect("decays 35 dB in band"),
            )
        })
        .collect()
}

/// The graphic-EQ Fdn holds its profile per octave band: measured T30
/// in every band from 63 Hz to 8 kHz within 5 % of the setting, static
/// and modulated, at 48 and 96 kHz, for a real hall and a studio room
/// (and, reported only, a steep profile).
#[test]
#[cfg_attr(miri, ignore)]
fn t60_per_octave_band_matches_the_profile() {
    let mut over = Vec::new();
    let mut worst = 0.0f64;
    for (name, profile) in [
        ("Promenadi", PROMENADI),
        ("studio", STUDIO),
        ("steep", STEEP),
    ] {
        let held = name != "steep";
        for rate in [48_000.0f32, 96_000.0] {
            for modulation_ms in [0.0, 0.25] {
                let mut fdn = Fdn::new(Mixing::Hadamard);
                fdn.set_modulation(ms(f64::from(modulation_ms)));
                fdn.set_t60(T60::octaves(profile.map(seconds)));
                let longest = profile.iter().copied().fold(0.0, f64::max) as f32;
                let measured = band_t60s(&mut fdn, rate, 1.2 * longest + 0.2, 1..=8);
                let line: Vec<String> = measured
                    .iter()
                    .map(|&(k, t)| {
                        let error = t / profile[k] - 1.0;
                        if held {
                            worst = worst.max(error.abs());
                        }
                        if held && error.abs() >= BAND_BUDGET {
                            over.push(format!(
                                "{name} {rate} Hz mod {modulation_ms} ms, {} Hz: {t:.3} s for {}",
                                T60::CENTRES_HZ[k],
                                profile[k]
                            ));
                        }
                        format!("{:+.1}", error * 100.0)
                    })
                    .collect();
                println!(
                    "{name:<9} {rate:>6} Hz mod {modulation_ms:.2} ms: % by band 63 Hz–8 kHz {line:?}"
                );
            }
        }
    }
    println!("worst band error (hall and studio) {:.2} %", worst * 100.0);
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// `set_decay` still replaces a profile.
#[test]
fn set_decay_replaces_a_profile() {
    let mut fdn = Fdn::new(Mixing::Householder);
    fdn.set_t60(T60::uniform(seconds(4.0)));
    fdn.set_decay(seconds(0.5));
    fdn.prepare(48_000.0);
    let crate::Tail::Samples(n) = fdn.tail() else {
        panic!("finite")
    };
    assert!(n < 48_000 * 2, "tail {n} follows the 0.5 s setting");
}

/// Every Kronecker matrix is orthogonal: `AᵀA = I` over a grid of
/// angles, for every mix of rotations and reflections.
#[test]
fn kronecker_matrices_are_orthogonal_at_every_angle() {
    let kernels = [Kernel::Rotation, Kernel::Reflection];
    for a in 0..8 {
        for kind in 0..8 {
            let angles = [
                a as f32 * 0.41,
                a as f32 * -0.93 + 0.2,
                a as f32 * 1.7 - 3.0,
            ];
            let matrix = Kronecker::new(
                [
                    kernels[kind & 1],
                    kernels[(kind >> 1) & 1],
                    kernels[(kind >> 2) & 1],
                ],
                angles,
            );
            let columns: Vec<[f32; 8]> = (0..8)
                .map(|j| {
                    let mut e = [0.0f32; 8];
                    e[j] = 1.0;
                    matrix.mix(&mut e);
                    e
                })
                .collect();
            for i in 0..8 {
                for j in 0..8 {
                    let dot: f32 = (0..8).map(|k| columns[i][k] * columns[j][k]).sum();
                    let want = if i == j { 1.0 } else { 0.0 };
                    assert!((dot - want).abs() < 1e-6, "{angles:?} ({i},{j}): {dot}");
                }
            }
        }
    }
}

/// Every kernel a reflection at π/4 is the Hadamard matrix.
#[test]
fn kronecker_at_quarter_pi_is_hadamard() {
    for j in 0..8 {
        let mut a = [0.0f32; 8];
        a[j] = 1.0;
        let mut b = a;
        Kronecker::hadamard().mix(&mut a);
        Mixing::Hadamard.mix(&mut b);
        for k in 0..8 {
            assert!((a[k] - b[k]).abs() < 1e-6, "column {j}: {a:?} vs {b:?}");
        }
    }
}

/// Frozen, the network is lossless: the energy stored in its lines holds
/// over ten seconds at 48 kHz — with a fixed matrix, a static Kronecker
/// matrix and one whose angles sweep the whole time — and the output
/// keeps sounding; released, it decays again.
#[test]
#[cfg_attr(miri, ignore)]
fn freeze_preserves_energy() {
    let rate = 48_000.0;
    let mut worst = 0.0f64;
    for (name, kronecker, depth) in [
        ("Householder", false, 0.0),
        ("Kronecker", true, 0.0),
        ("Kronecker swept", true, 1.2),
    ] {
        let mut fdn = Fdn::new(Mixing::Householder);
        if kronecker {
            fdn.set_kronecker(Kronecker::new([Kernel::Rotation; 3], [0.5, 0.9, 0.2]));
            fdn.set_angle_modulation(depth, hz(1.3));
        }
        fdn.set_t60(T60::octaves(PROMENADI.map(seconds)));
        fdn.prepare(rate);
        let mut lcg = 5u32;
        for _ in 0..rate as usize {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            fdn.tick((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0);
        }
        fdn.set_freeze(true);
        assert_eq!(fdn.tail(), crate::Tail::Infinite);
        let start = fdn.stored_energy();
        let mut out = 0.0f64;
        let mut drift = 0.0f64;
        for second in 0..10 {
            for _ in 0..rate as usize {
                // The input is muted while frozen.
                let y = fdn.tick(1.0);
                if second == 9 {
                    out += f64::from(y).powi(2);
                }
            }
            drift = drift.max((fdn.stored_energy() / start - 1.0).abs());
        }
        worst = worst.max(drift);
        println!(
            "{name:<16} frozen 10 s: stored energy {start:.3}, worst drift {drift:.2e}, last-second output {out:.3}"
        );
        assert!(drift < 1e-4, "{name}: drift {drift:.2e}");
        assert!(out > 1e-3 * start, "{name}: the frozen tail keeps sounding");
        fdn.set_freeze(false);
        for _ in 0..(3.5 * rate) as usize {
            fdn.tick(0.0);
        }
        assert!(
            fdn.stored_energy() < 1e-3 * start,
            "{name}: decays after release"
        );
    }
    println!("worst frozen drift {worst:.2e}");
}

/// Sweeping the Kronecker angles moves the modes, not the decay: per
/// octave band, the T60 with the matrix modulated is within 3 % of the
/// static matrix's.
#[test]
#[cfg_attr(miri, ignore)]
fn modulating_the_matrix_keeps_the_decay() {
    let rate = 48_000.0;
    let run = |depth: f32| {
        let mut fdn = Fdn::new(Mixing::Householder);
        fdn.set_kronecker(Kronecker::new([Kernel::Rotation; 3], [0.7, 0.7, 0.7]));
        fdn.set_angle_modulation(depth, hz(0.8));
        fdn.set_t60(T60::octaves(PROMENADI.map(seconds)));
        band_t60s(&mut fdn, rate, 3.8, 1..=8)
    };
    let (still, moving) = (run(0.0), run(0.6));
    let mut worst = 0.0f64;
    for (&(k, a), &(_, b)) in still.iter().zip(&moving) {
        let change = b / a - 1.0;
        worst = worst.max(change.abs());
        println!(
            "{:>6} Hz: static {a:.3} s, modulated {b:.3} s ({:+.2} %)",
            T60::CENTRES_HZ[k],
            change * 100.0
        );
        assert!(
            change.abs() < 0.03,
            "{} Hz: {a:.3} → {b:.3}",
            T60::CENTRES_HZ[k]
        );
    }
    println!("worst change {:.2} %", worst * 100.0);
}

/// The velvet-noise reverb's impulse response holds exactly one non-zero
/// sample in every grid cell, for its whole decay: the echo density is
/// the design's `fs / round(fs/2205)` per second, and the interleaved
/// branches never collide.
#[test]
#[cfg_attr(miri, ignore)]
fn velvet_noise_density_is_the_design() {
    for rate in [44_100.0f32, 48_000.0, 96_000.0] {
        let mut velvet = VelvetNoise::new();
        velvet.set_decay(seconds(2.0));
        velvet.prepare(rate);
        let grid = velvet.grid();
        let n = (3.0 * rate) as usize / grid * grid;
        let (left, right): (Vec<f32>, Vec<f32>) = (0..n)
            .map(|i| velvet.tick_stereo(if i == 0 { 1.0 } else { 0.0 }))
            .unzip();
        for (channel, h) in [("left", &left), ("right", &right)] {
            let mut impulses = 0usize;
            for (cell, window) in h.chunks_exact(grid).enumerate() {
                let count = window.iter().filter(|y| **y != 0.0).count();
                // The right channel's first cells wait for the reversed
                // interleave.
                if channel == "right" && cell < VelvetNoise::BRANCHES - 1 {
                    continue;
                }
                assert_eq!(
                    count, 1,
                    "{rate} Hz {channel} cell {cell}: {count} impulses"
                );
                impulses += count;
            }
            if channel == "left" {
                let density = impulses as f64 / (n as f64 / f64::from(rate));
                let design = f64::from(rate) / grid as f64;
                println!(
                    "{rate} Hz: grid {grid}, {impulses} impulses in {:.2} s = {density:.1} /s (design {design:.1} /s)",
                    n as f64 / f64::from(rate)
                );
                assert!((density - design).abs() < 1e-9);
            }
        }
    }
}

/// The velvet-noise reverb decays as set: per octave band, with the
/// Promenadi profile, 125 Hz – 4 kHz within 5 %; its level sits within
/// 6 dB of the Fdn's.
#[test]
#[cfg_attr(miri, ignore)]
fn velvet_noise_t60_matches_the_setting() {
    let rate = 48_000.0;
    let mut velvet = VelvetNoise::new();
    velvet.set_t60(T60::octaves(PROMENADI.map(seconds)));
    let measured = band_t60s(&mut velvet, rate, 3.8, 2..=7);
    let mut over = Vec::new();
    for &(k, t) in &measured {
        let error = t / PROMENADI[k] - 1.0;
        println!(
            "VelvetNoise {:>6} Hz: {t:.3} s for {} ({:+.2} %)",
            T60::CENTRES_HZ[k],
            PROMENADI[k],
            error * 100.0
        );
        if error.abs() >= BAND_BUDGET {
            over.push(format!(
                "{} Hz: {t:.3} s for {}",
                T60::CENTRES_HZ[k],
                PROMENADI[k]
            ));
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
    let energy = |stage: &mut dyn Stage| {
        impulse_response(stage, rate, 2.0)
            .iter()
            .map(|&y| f64::from(y).powi(2))
            .sum::<f64>()
    };
    let mut fdn = Fdn::new(Mixing::Householder);
    fdn.set_decay(seconds(2.0));
    let mut velvet = VelvetNoise::new();
    velvet.set_decay(seconds(2.0));
    let ratio_db = 10.0 * (energy(&mut velvet) / energy(&mut fdn)).log10();
    println!("VelvetNoise level re Fdn: {ratio_db:+.2} dB");
    assert!(ratio_db.abs() < 6.0, "{ratio_db:+.2} dB");
}

/// The velvet-noise reverb's block path is its per-sample path, bit for
/// bit, for ragged blocks, broadband and per band.
#[test]
#[cfg_attr(miri, ignore)]
fn velvet_noise_process_is_tick() {
    for graphic in [false, true] {
        let make = || {
            let mut v = VelvetNoise::new();
            if graphic {
                v.set_t60(T60::octaves(PROMENADI.map(seconds)));
            } else {
                v.set_decay(seconds(1.5));
            }
            v.prepare(48_000.0);
            v
        };
        let (mut a, mut b) = (make(), make());
        let mut lcg = 3u32;
        let x: Vec<f32> = (0..30_000)
            .map(|_| {
                lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect();
        let ticked: Vec<f32> = x.iter().map(|&s| a.tick(s)).collect();
        let mut blocked = x.clone();
        let mut at = 0;
        for len in [1usize, 63, 64, 65, 200, 7, 4_096].iter().cycle() {
            if at >= blocked.len() {
                break;
            }
            let end = (at + len).min(blocked.len());
            b.process(&mut blocked[at..end]);
            at = end;
        }
        for (n, (p, q)) in ticked.iter().zip(&blocked).enumerate() {
            assert_eq!(p.to_bits(), q.to_bits(), "graphic {graphic}: sample {n}");
        }
    }
}
