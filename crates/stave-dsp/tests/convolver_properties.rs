//! The [`Convolver`] against direct-form convolution (DSP.md F2, R10):
//! random responses and partitions, ragged blocks (blocks of one
//! included), responses shorter and longer than a partition, mono,
//! stereo and true stereo, with and without the direct head, uniform
//! (F2) and non-uniform (F3) schedules.
//!
//! The reference is `y[t] = Σ h[j]·x[t − j]` summed in `f64`. The
//! convolver runs an `f32` FFT, so the bound is relative: the error at
//! every sample is at most `TOLERANCE · ‖h‖₂ · max|x|` (measured worst
//! 2e-6, bounded at 2e-5).

use stave_dsp::primitives::spectral::Head;
use stave_dsp::{
    Biquad, Convolver, FilterSpec, ImpulseResponse, Oversampled2x, Serial, Stage, Tail, Xorshift32,
};

const TOLERANCE: f64 = 2e-5;

fn noise(n: usize, rng: &mut Xorshift32) -> Vec<f32> {
    (0..n).map(|_| rng.next_bipolar()).collect()
}

/// A decaying random response, like a room's.
fn response(n: usize, rng: &mut Xorshift32) -> Vec<f32> {
    (0..n)
        .map(|i| rng.next_bipolar() * (-(i as f32) / (n as f32 * 0.3 + 1.0)).exp())
        .collect()
}

fn direct(x: &[f32], h: &[f32]) -> Vec<f64> {
    (0..x.len())
        .map(|t| {
            h.iter()
                .enumerate()
                .take(t + 1)
                .map(|(j, &h)| f64::from(h) * f64::from(x[t - j]))
                .sum()
        })
        .collect()
}

fn l2(h: &[f32]) -> f64 {
    h.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt()
}

/// Ragged block sizes covering `total`, from 1 to `max`, ones included.
fn ragged(total: usize, max: usize, rng: &mut Xorshift32) -> Vec<usize> {
    let mut out = vec![];
    let mut left = total;
    while left > 0 {
        let b = if rng.next_u32() % 4 == 0 {
            1
        } else {
            1 + rng.next_u32() as usize % max
        };
        let b = b.min(left);
        out.push(b);
        left -= b;
    }
    out
}

/// Runs `x` through `conv` in ragged blocks; returns the output.
fn run_ragged(conv: &mut Convolver, x: &[f32], rng: &mut Xorshift32) -> Vec<f32> {
    let mut y = x.to_vec();
    let mut at = 0;
    for b in ragged(x.len(), 300, rng) {
        conv.process(&mut y[at..at + b]);
        at += b;
    }
    y
}

/// `got` (delayed by `latency`) against `want`, within the bound;
/// returns the worst error as a fraction of the bound.
fn assert_close(got: &[f32], want: &[f64], latency: usize, bound: f64, what: &str) -> f64 {
    let mut worst = 0.0f64;
    for (t, w) in want.iter().enumerate() {
        let g = got.get(t + latency).copied().unwrap_or(0.0);
        let err = (f64::from(g) - w).abs();
        assert!(
            err <= bound,
            "{what}: sample {t} is {g}, want {w} (error {err:e} > {bound:e})"
        );
        worst = worst.max(err / bound);
    }
    for (t, &g) in got.iter().take(latency).enumerate() {
        assert_eq!(g, 0.0, "{what}: sample {t} sounds before the latency");
    }
    worst
}

#[test]
#[cfg_attr(miri, ignore)]
fn mono_matches_direct_convolution_for_random_responses_and_blocks() {
    let mut rng = Xorshift32::new(0xC0DE);
    let (mut cases, mut worst) = (0, 0.0f64);
    for &block in &[1usize, 2, 4, 16, 64, 256] {
        for &len in &[
            0usize,
            1,
            3,
            block.saturating_sub(1),
            block,
            block + 1,
            5 * block + 3,
            700,
        ] {
            for head in [Head::Partitioned, Head::Direct] {
                let h = response(len, &mut rng);
                let ir = ImpulseResponse::mono(&h);
                let mut conv = Convolver::uniform(&ir, block, head);
                conv.prepare(48_000.0);
                let latency = conv.latency() as usize;
                assert_eq!(latency, if head == Head::Direct { 0 } else { block });
                let x = noise(len + 2 * block + 64, &mut rng);
                let y = run_ragged(&mut conv, &x, &mut rng);
                let want = direct(&x[..x.len() - latency], &h);
                let what = format!("B = {block}, len = {len}, {head:?}");
                let bound = TOLERANCE * l2(&h) + 1e-30;
                worst = worst.max(assert_close(&y, &want, latency, bound, &what));
                cases += 1;
            }
        }
    }
    assert!(cases > 90);
    println!("worst error: {:.2e} · ‖h‖₂ · max|x|", worst * TOLERANCE);
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_long_response_stays_within_the_bound() {
    // Many partitions: the frequency-domain delay line sums 94 products.
    let mut rng = Xorshift32::new(0x10A6);
    let h = response(6_000, &mut rng);
    let mut conv = Convolver::uniform(&ImpulseResponse::mono(&h), 64, Head::Direct);
    conv.prepare(48_000.0);
    assert_eq!(conv.partitions(), 93);
    let x = noise(7_000, &mut rng);
    let y = run_ragged(&mut conv, &x, &mut rng);
    let worst = assert_close(
        &y,
        &direct(&x, &h),
        0,
        TOLERANCE * l2(&h),
        "6000 taps, B = 64",
    );
    println!("worst error: {:.2e} · ‖h‖₂ · max|x|", worst * TOLERANCE);
}

#[test]
#[cfg_attr(miri, ignore)]
fn stereo_lanes_convolve_independently() {
    let mut rng = Xorshift32::new(0x57E);
    for head in [Head::Partitioned, Head::Direct] {
        let (hl, hr) = (response(300, &mut rng), response(77, &mut rng));
        let mut conv = Convolver::uniform(&ImpulseResponse::stereo(&hl, &hr), 32, head);
        conv.prepare(44_100.0);
        assert_eq!(conv.lanes(), 2);
        let latency = conv.latency() as usize;
        let (xl, xr) = (noise(600, &mut rng), noise(600, &mut rng));
        let (mut yl, mut yr) = (xl.clone(), xr.clone());
        let mut at = 0;
        for b in ragged(600, 100, &mut rng) {
            conv.process_stereo(&mut yl[at..at + b], &mut yr[at..at + b]);
            at += b;
        }
        let n = 600 - latency;
        assert_close(
            &yl,
            &direct(&xl[..n], &hl),
            latency,
            TOLERANCE * l2(&hl),
            "left",
        );
        assert_close(
            &yr,
            &direct(&xr[..n], &hr),
            latency,
            TOLERANCE * l2(&hr),
            "right",
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn true_stereo_sums_both_inputs_into_each_output() {
    let mut rng = Xorshift32::new(0x7255);
    let paths: Vec<Vec<f32>> = [200, 9, 130, 64].map(|n| response(n, &mut rng)).into();
    let ir = ImpulseResponse::true_stereo(&paths[0], &paths[1], &paths[2], &paths[3]);
    for head in [Head::Partitioned, Head::Direct] {
        let mut conv = Convolver::uniform(&ir, 16, head);
        conv.prepare(96_000.0);
        let latency = conv.latency() as usize;
        let (xl, xr) = (noise(500, &mut rng), noise(500, &mut rng));
        let mut out = (Vec::new(), Vec::new());
        for (&l, &r) in xl.iter().zip(&xr) {
            let (a, b) = conv.tick_stereo(l, r);
            out.0.push(a);
            out.1.push(b);
        }
        let n = 500 - latency;
        let sum = |a: Vec<f64>, b: Vec<f64>| -> Vec<f64> {
            a.iter().zip(&b).map(|(a, b)| a + b).collect()
        };
        let want_l = sum(direct(&xl[..n], &paths[0]), direct(&xr[..n], &paths[2]));
        let want_r = sum(direct(&xl[..n], &paths[1]), direct(&xr[..n], &paths[3]));
        let bound = TOLERANCE * paths.iter().map(|p| l2(p)).sum::<f64>();
        assert_close(&out.0, &want_l, latency, bound, "true-stereo left");
        assert_close(&out.1, &want_r, latency, bound, "true-stereo right");
    }
}

/// F3: the non-uniform schedule against direct convolution at the same
/// budget, over blocks, largest partitions, heads and response lengths
/// that end inside every run of the schedule.
#[test]
#[cfg_attr(miri, ignore)]
fn non_uniform_matches_direct_convolution() {
    let mut rng = Xorshift32::new(0x6A2D);
    let (mut cases, mut worst) = (0, 0.0f64);
    for &(block, largest) in &[(1usize, 8usize), (4, 4), (4, 64), (16, 128), (32, 1024)] {
        for &len in &[0usize, 1, block, 3 * block + 1, 7 * block, 23 * block + 5, 2_500] {
            for head in [Head::Partitioned, Head::Direct] {
                let h = response(len, &mut rng);
                let ir = ImpulseResponse::mono(&h);
                let mut conv = Convolver::non_uniform(&ir, block, largest, head);
                conv.prepare(48_000.0);
                let latency = conv.latency() as usize;
                let x = noise(len + 2 * largest + 64, &mut rng);
                let y = run_ragged(&mut conv, &x, &mut rng);
                let want = direct(&x[..x.len() - latency], &h);
                let what = format!("B = {block}, largest = {largest}, len = {len}, {head:?}");
                let bound = TOLERANCE * l2(&h) + 1e-30;
                worst = worst.max(assert_close(&y, &want, latency, bound, &what));
                cases += 1;
            }
        }
    }
    assert!(cases >= 70);
    println!("worst error: {:.2e} · ‖h‖₂ · max|x|", worst * TOLERANCE);
}

#[test]
#[cfg_attr(miri, ignore)]
fn non_uniform_true_stereo_matches_direct_convolution() {
    let mut rng = Xorshift32::new(0x7E57);
    let paths: Vec<Vec<f32>> = [1_500, 40, 900, 1_200].map(|n| response(n, &mut rng)).into();
    let ir = ImpulseResponse::true_stereo(&paths[0], &paths[1], &paths[2], &paths[3]);
    for head in [Head::Partitioned, Head::Direct] {
        let mut conv = Convolver::non_uniform(&ir, 16, 256, head);
        conv.prepare(48_000.0);
        assert!(conv.schedule().count() >= 2, "{:?}", conv.schedule().collect::<Vec<_>>());
        let latency = conv.latency() as usize;
        let (xl, xr) = (noise(2_000, &mut rng), noise(2_000, &mut rng));
        let (mut yl, mut yr) = (xl.clone(), xr.clone());
        let mut at = 0;
        for b in ragged(2_000, 300, &mut rng) {
            conv.process_stereo(&mut yl[at..at + b], &mut yr[at..at + b]);
            at += b;
        }
        let n = 2_000 - latency;
        let sum = |a: Vec<f64>, b: Vec<f64>| -> Vec<f64> {
            a.iter().zip(&b).map(|(a, b)| a + b).collect()
        };
        let want_l = sum(direct(&xl[..n], &paths[0]), direct(&xr[..n], &paths[2]));
        let want_r = sum(direct(&xl[..n], &paths[1]), direct(&xr[..n], &paths[3]));
        let bound = TOLERANCE * paths.iter().map(|p| l2(p)).sum::<f64>();
        assert_close(&yl, &want_l, latency, bound, "non-uniform left");
        assert_close(&yr, &want_r, latency, bound, "non-uniform right");
    }
}

#[test]
fn non_uniform_with_one_size_is_uniform_bit_for_bit() {
    let mut rng = Xorshift32::new(0x5A3E);
    let ir = ImpulseResponse::mono(&response(300, &mut rng));
    let x = noise(800, &mut rng);
    for head in [Head::Partitioned, Head::Direct] {
        let mut a = Convolver::uniform(&ir, 32, head);
        let mut b = Convolver::non_uniform(&ir, 32, 32, head);
        a.prepare(48_000.0);
        b.prepare(48_000.0);
        assert_eq!(a.schedule().collect::<Vec<_>>(), b.schedule().collect::<Vec<_>>());
        let ya: Vec<f32> = x.iter().map(|&s| a.tick(s)).collect();
        let yb: Vec<f32> = x.iter().map(|&s| b.tick(s)).collect();
        assert_eq!(ya, yb);
    }
}

#[test]
#[should_panic(expected = "largest partition")]
fn a_largest_partition_below_the_block_is_refused() {
    let _ = Convolver::non_uniform(&ImpulseResponse::mono(&[1.0]), 64, 32, Head::Direct);
}

#[test]
fn the_block_path_is_bit_identical_to_the_tick_loop() {
    let mut rng = Xorshift32::new(0xB10C);
    let h = response(150, &mut rng);
    let ir = ImpulseResponse::mono(&h);
    let x = noise(1_000, &mut rng);
    let mut a = Convolver::uniform(&ir, 32, Head::Direct);
    let mut b = a.clone();
    a.prepare(48_000.0);
    b.prepare(48_000.0);
    let ticked: Vec<f32> = x.iter().map(|&s| a.tick(s)).collect();
    let blocked = run_ragged(&mut b, &x, &mut rng);
    assert_eq!(ticked, blocked);
}

#[test]
fn reset_returns_to_the_fresh_state() {
    let mut rng = Xorshift32::new(0x2E5E7);
    let h = response(100, &mut rng);
    let mut conv = Convolver::uniform(&ImpulseResponse::mono(&h), 16, Head::Partitioned);
    conv.prepare(48_000.0);
    let x = noise(300, &mut rng);
    let first: Vec<f32> = x.iter().map(|&s| conv.tick(s)).collect();
    conv.reset();
    let again: Vec<f32> = x.iter().map(|&s| conv.tick(s)).collect();
    assert_eq!(first, again);
}

#[test]
fn latency_and_tail_are_reported_and_compose() {
    let h = vec![0.5f32; 1_000];
    let ir = ImpulseResponse::mono(&h);
    let conv = Convolver::uniform(&ir, 128, Head::Partitioned);
    assert_eq!(conv.latency(), 128);
    assert_eq!(conv.tail(), Tail::Samples(128 + 999));
    let direct_head = Convolver::uniform(&ir, 128, Head::Direct);
    assert_eq!(direct_head.latency(), 0);
    assert_eq!(direct_head.tail(), Tail::Samples(999));
    assert_eq!(
        Convolver::uniform(&ImpulseResponse::mono(&[]), 8, Head::Direct).tail(),
        Tail::None
    );

    // It is a Stage like any other: serial latencies sum, a 2× region
    // divides the interior's back to the outer rate.
    let mut chain = Serial((
        conv.clone(),
        Biquad::new(FilterSpec::Lowpass {
            hz: 5_000.0,
            q: 0.7,
        }),
    ));
    chain.prepare(48_000.0);
    assert_eq!(chain.latency(), 128);
    let mut os = Oversampled2x::front(conv);
    os.prepare(48_000.0);
    assert_eq!(os.latency(), Oversampled2x::front(()).latency() + 64);
    let y = os.tick(1.0);
    assert!(y.is_finite());
}

#[test]
fn an_empty_response_is_silence() {
    for head in [Head::Partitioned, Head::Direct] {
        let mut conv = Convolver::uniform(&ImpulseResponse::mono(&[]), 4, head);
        conv.prepare(48_000.0);
        assert!((0..40).all(|i| conv.tick(i as f32) == 0.0));
    }
}

#[test]
#[should_panic(expected = "power of two")]
fn a_partition_that_is_not_a_power_of_two_is_refused() {
    let _ = Convolver::uniform(&ImpulseResponse::mono(&[1.0]), 48, Head::Partitioned);
}
