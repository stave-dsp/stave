//! Click-free impulse-response swaps (DSP.md F3b, for H7's microphone
//! blending): [`Crossfade`] against the crossfade of two direct-form
//! convolutions, and a coherent tone swapped between two microphone-like
//! responses, measured for off-carrier spray (stave-measure's zipper
//! metric) and for sample-level edges (the largest second difference),
//! against a hard swap.

use stave_dsp::primitives::spectral::{Crossfade, Head};
use stave_dsp::{Convolver, ImpulseResponse, Stage};
use stave_measure::zipper::{zipper_report, ZipperConfig};

fn noise(seed: u32, n: usize) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// `Σ h[j]·x[t − j]` in `f64`.
fn direct(h: &[f32], x: &[f32]) -> Vec<f64> {
    (0..x.len())
        .map(|t| {
            h.iter()
                .enumerate()
                .take(t + 1)
                .map(|(j, &c)| f64::from(c) * f64::from(x[t - j]))
                .sum()
        })
        .collect()
}

/// The incoming gain at fade sample `k` of `fade`: a raised cosine.
fn gain(k: usize, fade: usize) -> f64 {
    if k + 1 >= fade {
        1.0
    } else {
        0.5 - 0.5 * (std::f64::consts::PI * (k + 1) as f64 / fade as f64).cos()
    }
}

/// During a swap the output is the raised-cosine crossfade of the two
/// full convolutions: silent warm-up for the incoming response's
/// length, then the fade, then the new response alone — within 1e-5,
/// both heads, swapping twice.
#[test]
fn a_swap_is_the_crossfade_of_the_two_convolutions() {
    let (a, b) = (noise(1, 300), noise(2, 700));
    let x = noise(3, 12_000);
    let (ya, yb) = (direct(&a, &x), direct(&b, &x));
    let fade = 1_000;
    for head in [Head::Direct, Head::Partitioned] {
        let mut xf = Crossfade::new(
            Convolver::uniform(&ImpulseResponse::mono(&a), 64, head),
            fade,
        );
        xf.prepare(48_000.0);
        let latency = xf.latency() as usize;
        let at = 2_000;
        let mut worst = 0.0f64;
        let mut swapped_at = None;
        for (t, &s) in x.iter().enumerate() {
            if t == at {
                let next = Box::new(Convolver::uniform(&ImpulseResponse::mono(&b), 64, head));
                assert!(xf.load(next).expect("steady").is_none());
                assert!(xf.swapping());
                swapped_at = Some(t);
            }
            let y = f64::from(xf.tick(s));
            // The incoming convolver warms for its tail: latency plus
            // length minus one.
            let warm = latency + b.len() - 1;
            let want_at = |t: usize| {
                let clean = |v: &[f64]| if t >= latency { v[t - latency] } else { 0.0 };
                match swapped_at {
                    Some(t0) if t >= t0 + warm => {
                        let g = gain(t - t0 - warm, fade);
                        (1.0 - g) * clean(&ya) + g * clean(&yb)
                    }
                    _ => clean(&ya),
                }
            };
            worst = worst.max((y - want_at(t)).abs());
        }
        assert!(!xf.swapping(), "the swap finished");
        println!("{head:?}: worst error against the direct crossfade {worst:.2e}");
        assert!(worst < 1e-5, "{head:?}: {worst:.2e}");
        // A second load while steady is accepted; one mid-swap refused.
        let back = Box::new(Convolver::uniform(&ImpulseResponse::mono(&a), 64, head));
        let displaced = xf.load(back).expect("steady again");
        assert!(
            displaced.is_some(),
            "the first convolver comes back to drop"
        );
        let again = Box::new(Convolver::uniform(&ImpulseResponse::mono(&a), 64, head));
        assert!(xf.load(again).is_err(), "refused mid-swap");
    }
}

/// A coherent 1 kHz tone swapped between two microphone-like responses
/// (0.3 ms and 0.7 ms arrivals, different levels and a reflection): the
/// crossfade sprays under −60 dB off the carrier
/// and adds no edge (its largest second difference stays within the
/// steady output's), where a hard swap sprays tens of dB more and steps.
#[test]
fn a_swap_has_no_click_where_a_hard_swap_does() {
    const RATE: f64 = 48_000.0;
    const N: usize = 16_384;
    const BIN: usize = 341; // 999 Hz, on a bin
    let mic = |delay: usize, gain: f32, reflection: (usize, f32)| {
        let mut h = vec![0.0f32; 64];
        h[delay] = gain;
        h[reflection.0] += reflection.1;
        ImpulseResponse::mono(&h)
    };
    let close = mic(14, 1.0, (40, 0.3));
    let far = mic(34, 0.7, (55, -0.25));
    let tone: Vec<f32> = (0..3 * N)
        .map(|n| (std::f64::consts::TAU * BIN as f64 * n as f64 / N as f64).sin() as f32 * 0.5)
        .collect();
    // The analysed window: N samples, the swap starting a quarter in.
    let (start, swap) = (N, N + N / 4);
    let run = |fade: Option<usize>| -> Vec<f64> {
        let mut xf = Crossfade::new(
            Convolver::uniform(&close, 16, Head::Direct),
            fade.unwrap_or(1),
        );
        xf.prepare(RATE as f32);
        tone.iter()
            .enumerate()
            .map(|(t, &s)| {
                if t == swap && fade.is_some() {
                    let next = Box::new(Convolver::uniform(&far, 16, Head::Direct));
                    assert!(xf.load(next).is_ok());
                }
                f64::from(xf.tick(s))
            })
            .collect()
    };
    let config = ZipperConfig {
        carrier_bin: BIN,
        sample_rate: RATE,
        guard_hz: 200.0,
        low_cut_hz: 20.0,
    };
    let steady = run(None);
    let faded = run(Some(960)); // 20 ms
    let hard = run(Some(1));
    let spray = |y: &[f64]| zipper_report(&y[start..start + N], &config).spray_db;
    let edge = |y: &[f64]| {
        y[start..start + N]
            .windows(3)
            .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
            .fold(0.0f64, f64::max)
    };
    // The far microphone's own steady edge (a different level).
    let far_steady: Vec<f64> = {
        let mut c = Convolver::uniform(&far, 16, Head::Direct);
        c.prepare(RATE as f32);
        tone.iter().map(|&s| f64::from(c.tick(s))).collect()
    };
    let steady_edge = edge(&steady).max(edge(&far_steady));
    println!(
        "spray off the carrier: steady {:.1} dB, crossfade (20 ms) {:.1} dB, hard swap {:.1} dB",
        spray(&steady),
        spray(&faded),
        spray(&hard)
    );
    println!(
        "largest second difference: steady {:.4}, crossfade {:.4}, hard swap {:.4}",
        steady_edge,
        edge(&faded),
        edge(&hard)
    );
    assert!(spray(&faded) < -60.0, "the crossfade sprays");
    assert!(
        spray(&hard) > spray(&faded) + 20.0,
        "a hard swap should click"
    );
    assert!(edge(&faded) <= steady_edge * 1.01, "the crossfade steps");
    assert!(edge(&hard) > 2.0 * steady_edge, "a hard swap should step");
}
