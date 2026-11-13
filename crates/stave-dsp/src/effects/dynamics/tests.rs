use super::*;
use crate::kernel::node::Stage;
use stave_measure::loudness::{LevelMeasure, TruePeak};

const RATE: f32 = 48_000.0;

/// Eq. (4) of Giannoulis et al. 2012, written out independently.
fn eq4(x: f32, t: f32, r: f32, w: f32) -> f32 {
    if 2.0 * (x - t) < -w {
        x
    } else if w > 0.0 && 2.0 * (x - t).abs() <= w {
        x + (1.0 / r - 1.0) * (x - t + w / 2.0).powi(2) / (2.0 * w)
    } else {
        t + (x - t) / r
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.abs().log10()
}

/// The settled output level (dB) of a compressor fed a constant at
/// `level_db`.
fn settled(c: &mut Compressor, level_db: f32) -> f32 {
    c.prepare(RATE);
    let x = 10f32.powf(level_db / 20.0);
    let mut y = 0.0;
    for _ in 0..RATE as usize / 2 {
        y = c.tick(x);
    }
    db(y)
}

#[test]
#[cfg_attr(miri, ignore)]
fn static_curves_match_the_formulas() {
    let curves = [
        GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 0.0 },
        GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 10.0 },
        GainComputer { threshold_db: -10.0, ratio: f32::INFINITY, knee_db: 6.0 },
    ];
    for g in curves {
        for k in 0..=288 {
            let x = -60.0 + 0.25 * k as f32;
            let want = eq4(x, g.threshold_db, g.ratio, g.knee_db);
            assert!((g.output_db(x) - want).abs() < 1e-4, "{g:?} at {x}");
        }
    }
    let mut worst = [0.0f32; 3];
    for level in [-40.0f32, -25.0, -22.0, -20.0, -17.0, -10.0, 0.0, 6.0] {
        // Feed-forward, both knees.
        for (i, g) in curves[..2].iter().enumerate() {
            let mut c = Compressor::new(*g, Topology::FeedForward);
            c.set_attack_ms(1.0);
            c.set_release_ms(1.0);
            let err = (settled(&mut c, level) - g.output_db(level)).abs();
            worst[0] = worst[0].max(err);
            assert!(err < 0.01, "feed-forward {i} at {level}: {err}");
        }
        // Feedback, hard knee: the same static curve.
        let mut c = Compressor::new(curves[0], Topology::Feedback);
        c.set_attack_ms(1.0);
        c.set_release_ms(1.0);
        let err = (settled(&mut c, level) - curves[0].output_db(level)).abs();
        worst[1] = worst[1].max(err);
        assert!(err < 0.01, "feedback hard at {level}: {err}");
        // Feedback, soft knee: the implicit solution y = x + c(y).
        let g = curves[1];
        let (mut lo, mut hi) = (level - 60.0, level);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if mid - level - g.feedback_gain_db(mid) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let mut c = Compressor::new(g, Topology::Feedback);
        c.set_attack_ms(1.0);
        c.set_release_ms(1.0);
        let err = (settled(&mut c, level) - lo).abs();
        worst[2] = worst[2].max(err);
        assert!(err < 0.05, "feedback soft at {level}: {err}");
    }
    println!("static curve worst error: ff {:.5} dB, fb hard {:.5} dB, fb soft {:.5} dB", worst[0], worst[1], worst[2]);
}

/// Samples until `values` first crosses `target` (rising or falling).
fn crossing(values: &[f32], target: f32) -> usize {
    let rising = values.last().unwrap() > values.first().unwrap();
    values
        .iter()
        .position(|&v| if rising { v >= target } else { v <= target })
        .unwrap()
}

#[test]
fn detector_times_match_the_settings() {
    let mut p = Peak::new(5.0, 50.0);
    p.prepare(RATE);
    let rise: Vec<f32> = (0..4_800).map(|_| p.tick(1.0)).collect();
    let fall: Vec<f32> = (0..24_000).map(|_| p.tick(0.0)).collect();
    let attack = crossing(&rise, 1.0 - (-1.0f32).exp()) as f32 + 1.0;
    let release = crossing(&fall, rise[4_799] * (-1.0f32).exp()) as f32 + 1.0;
    assert!((attack - 240.0).abs() <= 1.0, "attack {attack} samples");
    assert!((release - 2_400.0).abs() <= 1.0, "release {release} samples");
}

/// The compressor's gain, measured from its audio on a DC step, moves
/// with the attack and release time constants.
#[test]
#[cfg_attr(miri, ignore)]
fn compressor_times_match_the_settings() {
    for topology in [Topology::FeedForward, Topology::Feedback] {
        let ratio = if topology == Topology::Feedback { 20.0 } else { f32::INFINITY };
        let mut c = Compressor::new(GainComputer { threshold_db: -20.0, ratio, knee_db: 0.0 }, topology);
        c.set_attack_ms(5.0);
        c.set_release_ms(80.0);
        c.prepare(RATE);
        let (low, high) = (0.01f32, 1.0f32);
        for _ in 0..4_800 {
            c.tick(low);
        }
        let attack_gain: Vec<f32> = (0..24_000).map(|_| db(c.tick(high) / high)).collect();
        let release_gain: Vec<f32> = (0..96_000).map(|_| db(c.tick(low) / low)).collect();
        let settled = attack_gain[23_999];
        let a = crossing(&attack_gain, settled * (1.0 - (-1.0f32).exp())) as f32 + 1.0;
        let r = crossing(&release_gain, release_gain[0] * (-1.0f32).exp()) as f32;
        println!("{topology:?}: attack {:.3} ms (set 5), release {:.3} ms (set 80)", a / 48.0, r / 48.0);
        let budget = if topology == Topology::Feedback { 0.05 } else { 0.01 };
        assert!((a / 240.0 - 1.0).abs() < budget, "{topology:?} attack {a} samples");
        assert!((r / 3_840.0 - 1.0).abs() < budget, "{topology:?} release {r} samples");
    }
}

#[test]
fn rms_reads_a_sine_at_its_rms() {
    let mut rms = Rms::new(10.0);
    rms.prepare(RATE);
    assert_eq!(rms.window_samples(), 480);
    let mut y = 0.0;
    for n in 0..960 {
        y = rms.tick(0.8 * (core::f32::consts::TAU * n as f32 / 48.0).sin());
    }
    assert!((y - 0.8 / 2f32.sqrt()).abs() < 1e-4, "{y}");
    for n in 0..480 {
        let y = rms.tick(0.0);
        if n < 479 {
            assert!(y > 0.0);
        } else {
            assert!(y < 1e-4, "{y}");
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn program_dependent_release_follows_history() {
    let fall_time = |burst: usize| {
        let mut d = ProgramDependent::new(1.0, 50.0, 1_000.0, 500.0);
        d.prepare(RATE);
        let mut top = 0.0;
        for _ in 0..burst {
            top = d.tick(1.0);
        }
        let fall: Vec<f32> = (0..240_000).map(|_| d.tick(0.0)).collect();
        crossing(&fall, top * (-1.0f32).exp()) as f32 / 48.0
    };
    let long = fall_time(3 * 48_000);
    let short = fall_time(240);
    println!("program-dependent release: {short:.1} ms after 5 ms, {long:.1} ms after 3 s");
    assert!((long / 1_000.0 - 1.0).abs() < 0.05, "{long} ms");
    assert!(short < 250.0, "{short} ms");
}

#[test]
fn stereo_link_modes() {
    assert_eq!(Link::Independent.apply(0.2, 0.6), (0.2, 0.6));
    assert_eq!(Link::Max.apply(0.2, 0.6), (0.6, 0.6));
    assert_eq!(Link::Mean.apply(0.2, 0.6), (0.4, 0.4));
}

#[test]
#[cfg_attr(miri, ignore)]
fn sidechain_band_limits_the_key() {
    let gain_db = |hz: f32| {
        let mut s = Sidechain::new(Some(100.0), Some(5_000.0));
        s.prepare(RATE);
        let (mut i, mut o) = (0.0f64, 0.0f64);
        for n in 0..48_000 {
            let x = (core::f32::consts::TAU * hz * n as f32 / RATE).sin();
            let y = s.tick(x);
            if n > 24_000 {
                i += f64::from(x * x);
                o += f64::from(y * y);
            }
        }
        10.0 * (o / i).log10()
    };
    for (hz, want) in [(100.0, -3.0), (5_000.0, -3.0), (1_000.0, 0.0)] {
        let g = gain_db(hz);
        assert!((g - want).abs() < 0.2, "{hz} Hz: {g} dB");
    }
}

/// Hostile signals for the limiter, each two seconds or less.
fn hostile() -> Vec<(&'static str, Vec<f32>)> {
    let n = 48_000;
    let mut lcg = 5u32;
    let mut noise = move || {
        lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    let tau = core::f32::consts::TAU;
    vec![
        // Samples at ±0.707·A, true peak A: inter-sample overs.
        ("fs/4 sine at 45°, +12 dB", (0..n).map(|i| 4.0 * (tau * i as f32 / 4.0 + tau / 8.0).sin()).collect()),
        ("noise, +20 dB", (0..n).map(|_| 10.0 * noise()).collect()),
        (
            "bursts: +12 dB noise / quiet sine",
            (0..n)
                .map(|i| if (i / 2_400) % 2 == 0 { 4.0 * noise() } else { 0.1 * (tau * 440.0 * i as f32 / RATE).sin() })
                .collect(),
        ),
        (
            "sweep 15–23.9 kHz, +6 dB",
            (0..n)
                .map(|i| {
                    let t = i as f32 / RATE;
                    2.0 * (tau * (15_000.0 * t + 0.5 * 8_900.0 * t * t)).sin()
                })
                .collect(),
        ),
        ("clipped square, 1 kHz, +6 dB", (0..n).map(|i| if (i / 24) % 2 == 0 { 2.0 } else { -2.0 }).collect()),
        ("spikes", (0..n).map(|i| if i % 97 == 0 { 10.0 } else if i % 97 == 1 { -10.0 } else { 0.0 }).collect()),
        ("DC step", (0..n).map(|i| if i < n / 2 { 0.0 } else { 3.0 }).collect()),
    ]
}

#[test]
#[cfg_attr(miri, ignore)]
fn the_limiter_never_exceeds_its_ceiling() {
    let ceiling = -1.0f32;
    let mut worst_true = f64::NEG_INFINITY;
    let mut over = Vec::new();
    for (name, x) in hostile() {
        for lookahead in [0.1f32, 0.5, 1.0, 1.5, 5.0] {
            let mut l = Limiter::new(Peaks::True, ceiling);
            l.set_lookahead_ms(lookahead);
            l.set_release_ms(20.0);
            l.prepare(RATE);
            // Flushed: the limiter itself sees the signal stop.
            let flush = l.latency() as usize + 64;
            let y: Vec<f32> = x.iter().chain(&vec![0.0; flush]).map(|&v| l.tick(v)).collect();
            let tp = TruePeak::default().measure(&[&y], f64::from(RATE)).unwrap();
            worst_true = worst_true.max(tp - f64::from(ceiling));
            println!("{name}, {lookahead} ms: {:+.6} dB over the ceiling", tp - f64::from(ceiling));
            if tp > f64::from(ceiling) {
                over.push(format!("{name}, {lookahead} ms: {tp:.6} dBTP"));
            }

            let mut s = Limiter::new(Peaks::Sample, ceiling);
            s.set_lookahead_ms(lookahead);
            s.prepare(RATE);
            let peak = x.iter().map(|&v| s.tick(v).abs()).fold(0.0f32, f32::max);
            assert!(db(peak) <= ceiling, "{name}, {lookahead} ms sample: {} dB", db(peak));
        }
    }
    println!("limiter: worst true peak {worst_true:+.6} dB relative to the ceiling");
    assert!(over.is_empty(), "over the ceiling:\n{}", over.join("\n"));
}

#[test]
fn limiter_latency_is_exact() {
    for (peaks, rate, lookahead) in [
        (Peaks::True, 48_000.0, 1.5),
        (Peaks::True, 96_000.0, 3.0),
        (Peaks::Sample, 44_100.0, 1.0),
        (Peaks::Sample, 48_000.0, 0.0),
    ] {
        let mut l = Limiter::new(peaks, 0.0);
        l.set_lookahead_ms(lookahead);
        l.prepare(rate);
        let y: Vec<f32> = (0..1_000).map(|n| l.tick(if n == 0 { 0.25 } else { 0.0 })).collect();
        let arrival = y.iter().position(|&v| v != 0.0).unwrap();
        assert_eq!(arrival as u32, l.latency(), "{peaks:?} at {rate}, {lookahead} ms");
        assert!((y[arrival] - 0.25).abs() < 1e-6);
    }
}

#[test]
fn limiter_release_matches_the_setting() {
    let mut l = Limiter::new(Peaks::Sample, 0.0);
    l.set_lookahead_ms(1.0);
    l.set_release_ms(50.0);
    l.prepare(RATE);
    let mut gains = Vec::new();
    for n in 0..20_000 {
        l.tick(if n == 100 { 4.0 } else { 0.5 });
        gains.push(1.0 - l.gain());
    }
    // Well into the recovery: one time constant later the gap is 1/e.
    let (a, b) = (gains[1_000], gains[1_000 + 2_400]);
    let ratio = b / a;
    assert!((ratio / (-1.0f32).exp() - 1.0).abs() < 0.02, "{ratio}");
}
