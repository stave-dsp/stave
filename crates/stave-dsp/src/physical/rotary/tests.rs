use super::*;
use crate::kernel::node::Stage;
use core::f64::consts::TAU;

/// The geometry the model claims, solved independently in f64: a
/// source on `rotor`'s circle at angle `theta(t)`, heard at `mic`.
struct Geometry {
    radius: f64,
    mic: (f64, f64),
    /// Angle at time `t` (s).
    theta: Box<dyn Fn(f64) -> f64>,
}

impl Geometry {
    fn source(&self, t: f64) -> ((f64, f64), (f64, f64)) {
        let (s, c) = (self.theta)(t).sin_cos();
        ((self.radius * c, self.radius * s), (c, s))
    }

    /// Distance and `cos φ` from the source at emission time `te`.
    fn heard_from(&self, te: f64) -> (f64, f64) {
        let ((px, py), (ax, ay)) = self.source(te);
        let (vx, vy) = (self.mic.0 - px, self.mic.1 - py);
        let d = (vx * vx + vy * vy).sqrt();
        (d, (ax * vx + ay * vy) / d)
    }

    /// The emission time of what arrives at `t`: `t − te = |mic − s(te)|/c`.
    fn emission(&self, t: f64) -> f64 {
        let mut te = t;
        for _ in 0..30 {
            te = t - self.heard_from(te).0 / SPEED_OF_SOUND;
        }
        te
    }
}

/// A horn spinning steadily at `speed`, heard at `mic`, prepared at
/// `rate`; and the geometry it should realise.
fn steady(rotor: Rotor, directivity: Directivity, speed: Speed, mic: Mic, rate: f64) -> (Radiator, Geometry) {
    let mut r = Radiator::new(rotor, directivity);
    r.set_speed(speed);
    r.settle();
    r.prepare(rate as f32);
    assert!(r.set_mic(0, mic));
    r.reset();
    let sign = if rotor.clockwise { -1.0 } else { 1.0 };
    let w = sign * TAU * r.rate();
    // The rotor turns before each sample is written: sample n leaves at
    // the angle of (n + 1) periods.
    let g = Geometry { radius: rotor.radius, mic: mic.position(), theta: Box::new(move |t| w * (t + 1.0 / rate)) };
    (r, g)
}

/// Linear-interpolated rising and falling zero crossings, seconds.
fn crossings(y: &[f32], rate: f64, from: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for n in from..y.len() - 1 {
        let (a, b) = (f64::from(y[n]), f64::from(y[n + 1]));
        if (a < 0.0 && b >= 0.0) || (a > 0.0 && b <= 0.0) {
            out.push((n as f64 + a / (a - b)) / rate);
        }
    }
    out
}

#[test]
fn doppler_is_the_moving_source_shift() {
    let f = 4_000.0;
    let mic = Mic::at(60.0, 1.0);
    for rate in [48_000.0, 96_000.0, 192_000.0] {
        let (mut r, g) = steady(Rotor::HORN_122H, Directivity::OMNI, Speed::Tremolo, mic, rate);
        let n = (0.3 * rate) as usize;
        let y: Vec<f32> = (0..n).map(|i| r.tick((TAU * f * i as f64 / rate).sin() as f32)).collect();
        let measured = crossings(&y, rate, (0.01 * rate) as usize);
        // Each input zero crossing k/(2f) arrives after its path.
        let mut worst: f64 = 0.0;
        for &t in &measured {
            // The emission crossing nearest this arrival.
            let k = (g.emission(t) * 2.0 * f).round();
            let te_k = k / (2.0 * f);
            let arrive = te_k + g.heard_from(te_k).0 / SPEED_OF_SOUND;
            worst = worst.max((arrive - t).abs());
        }
        assert!(worst < 0.5e-6, "{rate} Hz: a crossing {:.3} µs off", worst * 1e6);
        // The shift's swing: the received frequency over ten periods
        // (crossing jitter averages out) at its peak, against the
        // near-field geometry over the same span, and Smith et al.'s
        // far-field rω/c.
        let span = 10.0 / f;
        let peak_measured = measured.windows(21).map(|w| 10.0 / (w[20] - w[0]) / f - 1.0).fold(0.0, f64::max);
        let peak_geometry = (0..20_000)
            .map(|i| {
                let t = 0.05 + i as f64 * 1e-5;
                (g.emission(t + span) - g.emission(t)) / span - 1.0
            })
            .fold(0.0, f64::max);
        let far = Rotor::HORN_122H.radius * TAU * f64::from(Rotor::HORN_122H.fast.get()) / SPEED_OF_SOUND;
        assert!((peak_measured / peak_geometry - 1.0).abs() < 0.01, "{peak_measured} vs {peak_geometry}");
        eprintln!(
            "{rate} Hz: crossings within {:.3} µs; peak shift {:.4} % (geometry {:.4} %, far field rω/c {:.4} %)",
            worst * 1e6,
            100.0 * peak_measured,
            100.0 * peak_geometry,
            100.0 * far
        );
    }
}

/// Least-squares amplitude of `y` around sample `at` (± `half`) against
/// the received phase `2πf·te(t)`.
fn amplitude(y: &[f32], g: &Geometry, f: f64, rate: f64, at: usize, half: usize) -> f64 {
    let (mut cc, mut ss, mut cs, mut yc, mut ys) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for n in at - half..at + half {
        let phase = TAU * f * g.emission(n as f64 / rate);
        let (s, c) = phase.sin_cos();
        let v = f64::from(y[n]);
        cc += c * c;
        ss += s * s;
        cs += c * s;
        yc += v * c;
        ys += v * s;
    }
    let det = cc * ss - cs * cs;
    let (p, q) = ((yc * ss - ys * cs) / det, (ys * cc - yc * cs) / det);
    (p * p + q * q).sqrt()
}

#[test]
fn amplitude_follows_the_directivity() {
    let rate = 48_000.0;
    let mic = Mic::at(0.0, 1.0);
    for (rotor, directivity, tones) in [
        (Rotor::HORN_122H, Directivity::HORN_122H, &[500.0, 2_000.0, 4_000.0, 8_000.0][..]),
        (Rotor::DRUM_122H, Directivity::DRUM_122H, &[100.0, 200.0, 400.0][..]),
    ] {
        for &f in tones {
            // Lagrange3's droop at a moving fraction is 0.23 dB at
            // 8 kHz and 48 kHz (the interpolator's, not the model's):
            // the top tone runs at 96 kHz.
            let rate = if f > 5_000.0 { 2.0 * rate } else { rate };
            let (mut r, g) = steady(rotor, directivity, Speed::Chorale, mic, rate);
            // One and a half turns at chorale.
            let n = (1.5 / f64::from(rotor.slow.get()) * rate) as usize;
            let y: Vec<f32> = (0..n).map(|i| r.tick((TAU * f * i as f64 / rate).sin() as f32)).collect();
            let half = (rate / f) as usize;
            let (mut worst, mut lo, mut hi, mut plo, mut phi) = (0.0f64, f64::MAX, 0.0f64, f64::MAX, 0.0f64);
            for at in ((0.05 * rate) as usize..n - half).step_by(half) {
                let got = amplitude(&y, &g, f, rate, at, half);
                let te = g.emission(at as f64 / rate);
                let (dist, cos_phi) = g.heard_from(te);
                let want = directivity.response(cos_phi, f, rate) * mic.distance / dist;
                worst = worst.max((20.0 * (got / want).log10()).abs());
                (lo, hi, plo, phi) = (lo.min(got), hi.max(got), plo.min(want), phi.max(want));
            }
            assert!(worst < 0.05, "{f} Hz: envelope {worst:.3} dB from the model");
            eprintln!(
                "{f} Hz: AM depth {:.2} dB (model {:.2} dB), worst {worst:.4} dB",
                20.0 * (hi / lo).log10(),
                20.0 * (phi / plo).log10()
            );
        }
    }
}

#[test]
fn walls_add_their_image() {
    let rate = 48_000.0f64;
    let mut r: Radiator = Radiator::new(Rotor::HORN_122H, Directivity::OMNI);
    r.set_speed(Speed::Stop);
    r.settle();
    r.set_walls(&[Wall::BACK_122H]);
    r.prepare(rate as f32);
    r.set_mic(0, Mic::at(0.0, 1.0));
    r.reset();
    let mut y = vec![0.0f32; 1_000];
    y[0] = 1.0;
    for v in &mut y {
        *v = r.tick(*v);
    }
    // At angle 0 the horn is at (0.15, 0); its image in the back wall
    // (x = −0.27) at (−0.69, 0).
    let (near, far) = (1.0 - 0.15, 1.0 + 0.69);
    for (dist, level) in [(near, 1.0 / near), (far, 0.5 / far)] {
        let at = dist / SPEED_OF_SOUND * rate;
        let w = &y[at as usize - 1..at as usize + 3];
        let sum: f64 = w.iter().map(|&v| f64::from(v)).sum();
        let centroid: f64 = w.iter().enumerate().map(|(k, &v)| (at.floor() - 1.0 + k as f64) * f64::from(v)).sum::<f64>() / sum;
        assert!((sum - level).abs() < 1e-5, "level {sum} vs {level}");
        assert!((centroid - at).abs() < 0.02, "arrival {centroid} vs {at}");
    }
}

#[test]
fn cabinet_bands_reach_their_rotors() {
    // Both rotors still, omnidirectional, the same radius, the mic a whole
    // number of samples away: the cabinet is its crossover's allpass sum,
    // delayed and attenuated by the path.
    let rate = 48_000.0f64;
    let rotor = Rotor { radius: 0.15, ..Rotor::HORN_122H };
    let still = |r: Rotor| {
        let mut x = Radiator::new(r, Directivity::OMNI);
        x.set_speed(Speed::Stop);
        x.settle();
        x
    };
    let mut cab: Cabinet = Cabinet::new(still(rotor), still(Rotor { clockwise: true, ..rotor }), crate::units::hz(800.0));
    cab.prepare(rate as f32);
    let k = 120.0;
    let mic = Mic::at(0.0, 0.15 + k * SPEED_OF_SOUND / rate);
    assert!(cab.set_mic(0, mic));
    cab.reset();
    let mut reference = crate::primitives::filters::crossover::Crossover::<2>::linkwitz_riley(
        crate::primitives::filters::crossover::Order::Lr4,
        &[crate::units::hz(800.0)],
    );
    reference.prepare(rate as f32);
    let input: Vec<f32> = (0..4_000).map(|i| (((i * 7919) % 211) as f32 / 105.0 - 1.0) * 0.5).collect();
    let want: Vec<f32> = input.iter().map(|&x| reference.tick(x)).collect();
    let got: Vec<f32> = input.iter().map(|&x| cab.tick(x)).collect();
    let level = mic.distance / (k * SPEED_OF_SOUND / rate);
    let worst = (k as usize..got.len())
        .map(|n| (f64::from(got[n]) - level * f64::from(want[n - k as usize])).abs())
        .fold(0.0, f64::max);
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn speed_switch_brakes_and_settles() {
    let mut r: Radiator = Radiator::new(Rotor::HORN_122H, Directivity::OMNI);
    r.prepare(48_000.0);
    r.set_speed(Speed::Tremolo);
    for _ in 0..(10.0 * 48_000.0) as usize {
        r.tick(0.0);
    }
    assert!((r.rate() - f64::from(Rotor::HORN_122H.fast.get())).abs() < 1e-6);
    r.set_speed(Speed::Stop);
    for _ in 0..(16.0 * 48_000.0) as usize {
        r.tick(0.0);
    }
    assert!(r.rate().abs() < 1e-6, "{}", r.rate());
    // A mic inside the rotor's circle or past the room is refused.
    assert!(!r.set_mic(0, Mic::at(0.0, 0.1)));
    assert!(!r.set_mic(1, Mic::at(0.0, 40.0)));
    assert_eq!(r.refused(), 2);
}
