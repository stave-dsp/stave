use super::*;
use crate::kernel::node::Stage;
use crate::units::{hz, seconds};
use core::f64::consts::{PI, TAU};
use stave_measure::spectrum::fft;
use stave_measure::tail::t60_s;

fn impulse_response(stage: &mut impl Stage, len: usize) -> Vec<f32> {
    (0..len).map(|n| stage.tick(if n == 0 { 1.0 } else { 0.0 })).collect()
}

/// The response's DFT (length a power of two, zero-padded).
fn spectrum(h: &[f32], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut re: Vec<f64> = h.iter().map(|&x| f64::from(x)).chain(core::iter::repeat(0.0)).take(n).collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (re, im)
}

#[test]
fn chirp_group_delay_is_the_published_dispersion() {
    for (sections, stretch, a) in [(100, 6, 0.6f32), (50, 6, 0.6), (200, 1, -0.6), (100, 2, -0.6)] {
        let mut chirp = Chirp::new(sections, stretch, a);
        let n = 1 << 16;
        let h = impulse_response(&mut chirp, n);
        let (re, im) = spectrum(&h, n);
        let peak = chirp.mean_delay() * (1.0 + f64::from(a.abs())) / (1.0 - f64::from(a.abs()));
        let top = (0.95 * n as f64 / (2.0 * stretch as f64)) as usize;
        let mut worst: f64 = 0.0;
        for b in 1..top {
            // −dφ/dω between bins b and b+1, at their midpoint.
            let (p0, p1) = (im[b].atan2(re[b]), im[b + 1].atan2(re[b + 1]));
            let dphi = (p1 - p0 + PI).rem_euclid(TAU) - PI;
            let measured = -dphi / (TAU / n as f64);
            let design = chirp.group_delay((b as f64 + 0.5) / n as f64);
            worst = worst.max((measured - design).abs() / peak);
        }
        println!("M {sections}, k {stretch}, a {a}: worst group-delay error {:.4} % of the {peak:.0}-sample peak", 100.0 * worst);
        assert!(worst < 1e-5, "M {sections}, k {stretch}, a {a}: {worst}");
    }
}

#[test]
fn chirp_is_allpass() {
    let mut chirp = Chirp::new(100, 6, 0.6);
    let n = 1 << 16;
    let (re, im) = spectrum(&impulse_response(&mut chirp, n), n);
    let worst = (0..n / 2).map(|b| (10.0 * (re[b] * re[b] + im[b] * im[b]).log10()).abs()).fold(0.0, f64::max);
    assert!(worst < 1e-4, "{worst} dB");
}

/// The time at which `h`'s energy in a narrow band at `hz` peaks within
/// `range` (samples): a Gaussian-windowed demodulation, σ = `sigma`
/// samples, its envelope's maximum.
fn ridge(h: &[f32], hz: f64, rate: f64, sigma: f64, range: core::ops::Range<usize>) -> f64 {
    let half = (4.0 * sigma) as isize;
    let w = TAU * hz / rate;
    let envelope = |n: usize| {
        let (mut re, mut im) = (0.0, 0.0);
        for j in -half..=half {
            let m = n as isize + j;
            if m < 0 || m as usize >= h.len() {
                continue;
            }
            let g = (-0.5 * (j as f64 / sigma).powi(2)).exp() * f64::from(h[m as usize]);
            re += g * (w * m as f64).cos();
            im -= g * (w * m as f64).sin();
        }
        re * re + im * im
    };
    // Coarse scan, then refine around the best.
    let step = 8;
    let mut best = (range.start, 0.0);
    for n in range.clone().step_by(step) {
        let e = envelope(n);
        if e > best.1 {
            best = (n, e);
        }
    }
    let lo = best.0.saturating_sub(step).max(range.start);
    let hi = (best.0 + step).min(range.end);
    for n in lo..hi {
        let e = envelope(n);
        if e > best.1 {
            best = (n, e);
        }
    }
    best.0 as f64
}

/// Group delay of the spring's output lowpass at `hz`, in samples.
fn lowpass_delay(spring: &Spring, hz: f64, rate: f64) -> f64 {
    let phase = |f: f64| {
        spring.lowpass.iter().map(|b| {
            let (re, im) = b.response_at(Hz::new(f as f32));
            f64::from(im).atan2(f64::from(re))
        })
        .sum::<f64>()
    };
    let df = 1.0;
    let d = (phase(hz + df) - phase(hz - df) + PI).rem_euclid(TAU) - PI;
    -d / (TAU * 2.0 * df / rate)
}

/// A transient leaves as a train of chirps whose delay rises with
/// frequency toward `f_C`: the ridge measured from the impulse response
/// at the first and second arrivals is the design's `L + D/2` and
/// `3L + 3D/2` (one, then three transits).
#[test]
fn transient_boings_as_rising_chirps() {
    let rate = 48_000.0;
    let mut spring = Spring::new(seconds(0.035), hz(4_000.0));
    spring.prepare(rate as f32);
    spring.set_high_chirps(Db::new(-200.0));
    spring.set_t60(seconds(3.0));
    let h = impulse_response(&mut spring, (0.25 * rate) as usize);
    let l = spring.transit_samples() as f64;
    let half = spring.low_chirp();
    let mut worst: f64 = 0.0;
    let mut first_ridge = Vec::new();
    for f in [300.0, 600.0, 1_000.0, 1_500.0, 2_000.0, 2_500.0, 3_000.0] {
        let d = half.group_delay(f / rate);
        let lp = lowpass_delay(&spring, f, rate);
        let first = l + d + lp;
        let third = 3.0 * l + 3.0 * d + lp;
        let early = 3.0 * l + 3.0 * half.group_delay(0.0);
        let got1 = ridge(&h, f, rate, 96.0, 0..early as usize - 200);
        let got3 = ridge(&h, f, rate, 96.0, early as usize - 200..(5.0 * l) as usize);
        let e1 = (got1 - first) / rate * 1e3;
        let e3 = (got3 - third) / rate * 1e3;
        println!(
            "{f:>5} Hz: first arrival {:.2} ms (design {:.2}), third transit {:.2} ms (design {:.2})",
            got1 / rate * 1e3,
            first / rate * 1e3,
            got3 / rate * 1e3,
            third / rate * 1e3
        );
        worst = worst.max(e1.abs()).max(e3.abs());
        first_ridge.push(got1);
    }
    // The delay rises with frequency (the chirp's direction).
    assert!(first_ridge.windows(2).all(|w| w[1] > w[0]), "{first_ridge:?}");
    assert!(worst < 0.1, "worst ridge error {worst} ms");
}

/// The band about `cos(ωk) = −a`, where the round trip is its mean,
/// decays at the set T60.
#[test]
fn decay_is_the_t60() {
    let rate = 48_000.0;
    for t60 in [1.5, 3.0] {
        let mut spring = Spring::new(seconds(0.033), hz(4_000.0));
        spring.prepare(rate as f32);
        spring.set_high_chirps(Db::new(-200.0));
        spring.set_t60(seconds(t60));
        let k = spring.low_chirp().stretch();
        let f = (-f64::from(LOW_COEFFICIENT)).acos() / (TAU * k) * rate;
        let h = impulse_response(&mut spring, (1.6 * t60 * rate) as usize);
        // The band's envelope every 16 samples (σ = 4 ms).
        let sigma = 0.004 * rate;
        let half = (4.0 * sigma) as isize;
        let w = TAU * f / rate;
        let envelope: Vec<f32> = (0..h.len())
            .step_by(16)
            .map(|n| {
                let (mut re, mut im) = (0.0, 0.0);
                for j in (-half..=half).step_by(2) {
                    let m = n as isize + j;
                    if m < 0 || m as usize >= h.len() {
                        continue;
                    }
                    let g = (-0.5 * (j as f64 / sigma).powi(2)).exp() * f64::from(h[m as usize]);
                    re += g * (w * m as f64).cos();
                    im -= g * (w * m as f64).sin();
                }
                (re * re + im * im).sqrt() as f32
            })
            .collect();
        let measured = t60_s(&envelope, rate / 16.0).unwrap();
        let error = (measured - t60) / t60;
        println!("T60 {t60} s at {f:.0} Hz: measured {measured:.3} s ({:+.2} %)", 100.0 * error);
        assert!(error.abs() < 0.02, "{t60} s: {measured}");
    }
}

#[test]
fn lossless_spring_stays_bounded() {
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut spring = Spring::new(seconds(0.03), hz(4_000.0));
        spring.prepare(rate);
        spring.set_t60(seconds(f64::INFINITY));
        assert_eq!(spring.reflection(), 1.0);
        let mut rng = crate::primitives::random::Xorshift32::new(7);
        let mut peak: f32 = 0.0;
        let n = if rate > 50_000.0 { rate as usize * 2 } else { rate as usize * 4 };
        for i in 0..n {
            let x = if i < rate as usize { rng.next_bipolar() } else { 0.0 };
            let y = spring.tick(x);
            assert!(y.is_finite());
            peak = peak.max(y.abs());
        }
        // An allpass loop conserves energy: the output stays at the
        // input's scale.
        assert!(peak < 50.0, "{rate}: peak {peak}");
        assert_eq!(spring.tail(), Tail::Infinite);
    }
}

#[test]
fn every_spring_arrives_in_a_tank() {
    let rate = 48_000.0;
    let mut tank = Tank::default();
    tank.prepare(rate as f32);
    tank.set_high_chirps(Db::new(-200.0));
    let h = impulse_response(&mut tank, (0.12 * rate) as usize);
    let f = 1_000.0;
    let mut previous = 0usize;
    for spring in tank.springs() {
        let l = spring.transit_samples() as f64;
        let first = l + spring.low_chirp().group_delay(f / rate) + lowpass_delay(spring, f, rate);
        let lo = (first - 0.004 * rate) as usize;
        let hi = (first + 0.004 * rate) as usize;
        let got = ridge(&h, f, rate, 96.0, lo.max(previous)..hi);
        let error = (got - first) / rate * 1e3;
        println!("spring at {:.1} ms: arrival {:.2} ms, design {:.2} ms", l / rate * 1e3, got / rate * 1e3, first / rate * 1e3);
        assert!(error.abs() < 0.2, "{error} ms");
        previous = hi;
    }
}

#[test]
fn transducers_band_limit_the_tank() {
    let rate = 48_000.0;
    let mut tank = Tank::default();
    tank.prepare(rate as f32);
    let n = 1 << 17;
    let h = impulse_response(&mut tank, n);
    let (re, im) = spectrum(&h, n);
    // Mean energy in `width` hertz of bins about `f`.
    let band = |f: f64, width: f64| {
        let c = (f / rate * n as f64) as usize;
        let w = ((width / rate * n as f64) as usize).max(2);
        let e: f64 = (c - w / 2..c + w / 2).map(|b| re[b] * re[b] + im[b] * im[b]).sum();
        10.0 * (e / w as f64).log10()
    };
    let mid = (band(1_000.0, 200.0) + band(2_000.0, 200.0) + band(3_000.0, 200.0)) / 3.0;
    let (low, high) = (band(20.0, 10.0), band(12_000.0, 200.0));
    println!("tank: 1-3 kHz {mid:.1} dB, 20 Hz {low:.1} dB, 12 kHz {high:.1} dB");
    assert!(mid - low > 12.0 && mid - high > 30.0, "{mid} {low} {high}");
}

#[test]
fn the_transition_is_the_one_set_at_every_rate() {
    let mut spring = Spring::new(seconds(0.03), hz(4_300.0));
    for rate in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
        spring.prepare(rate);
        let k = f64::from(rate) / 8_600.0;
        assert!((spring.low_chirp().stretch() - k).abs() < 1e-9, "{rate}: k {}", spring.low_chirp().stretch());
        assert!((spring.transition().get() - 4_300.0).abs() < 0.01, "{rate}: {:?}", spring.transition());
    }
}

#[test]
fn fractional_stretch_is_allpass() {
    for (k, a) in [(5.51, 0.6f32), (1.088, -0.6), (1.5, 0.6), (2.177, -0.6)] {
        let mut chirp = Chirp::stretched(50, k, a);
        let n = 1 << 16;
        let (re, im) = spectrum(&impulse_response(&mut chirp, n), n);
        let worst = (0..n / 2).map(|b| (10.0 * (re[b] * re[b] + im[b] * im[b]).log10()).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-3, "k {k}, a {a}: {worst} dB");
    }
}

#[test]
fn fractional_stretch_follows_the_design_delay() {
    // Thiran's delay is exact at DC and falls short toward Nyquist, so a
    // fractional stretch follows Parker's D(ω) at that k closely below
    // the transition and less closely at its peak.
    for (k, a) in [(5.51, 0.6f32), (11.03, 0.6), (1.088, -0.6), (2.177, -0.6)] {
        let mut chirp = Chirp::stretched(50, k, a);
        let n = 1 << 17;
        let h = impulse_response(&mut chirp, n);
        let (re, im) = spectrum(&h, n);
        let peak = chirp.mean_delay() * (1.0 + f64::from(a.abs())) / (1.0 - f64::from(a.abs()));
        let top = (0.8 * n as f64 / (2.0 * k)) as usize;
        let mut worst: f64 = 0.0;
        for b in 1..top {
            let (p0, p1) = (im[b].atan2(re[b]), im[b + 1].atan2(re[b + 1]));
            let dphi = (p1 - p0 + PI).rem_euclid(TAU) - PI;
            let measured = -dphi / (TAU / n as f64);
            let design = chirp.group_delay((b as f64 + 0.5) / n as f64);
            worst = worst.max((measured - design).abs() / peak);
        }
        println!("k {k}, a {a}: worst group-delay error {:.3} % of the {peak:.0}-sample peak", 100.0 * worst);
        assert!(worst < 0.025, "k {k}, a {a}: {worst}");
    }
}
