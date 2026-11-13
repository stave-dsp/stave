use super::*;
use core::f64::consts::TAU;

/// The rate every spec is stated at: the tightest one.
const FS: f64 = 44_100.0;
/// Outer samples per analysis window; tones are whole cycles of it.
const W: usize = 4096;
/// Outer samples played before the window: past every ring-out.
const WARM: usize = 2048;

struct Drive(f32);

impl Stage for Drive {
    fn tick(&mut self, x: f32) -> f32 {
        (x * self.0).tanh()
    }
}

/// A stage with a fixed one-sample delay, to give an interior latency.
#[derive(Default)]
struct OneSample(f32);

impl Stage for OneSample {
    fn tick(&mut self, x: f32) -> f32 {
        core::mem::replace(&mut self.0, x)
    }
    fn latency(&self) -> u32 {
        1
    }
}

/// `Σ x[i]·e^{−2πi·k·i/n}` over a whole window: the complex bin `k`.
fn bin(x: &[f64], k: f64) -> (f64, f64) {
    let n = x.len() as f64;
    x.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, &v)| {
        let ph = TAU * k * i as f64 / n;
        (re + v * ph.cos(), im - v * ph.sin())
    })
}

fn magnitude(x: &[f64], k: f64) -> f64 {
    let (re, im) = bin(x, k);
    2.0 * (re * re + im * im).sqrt() / x.len() as f64
}

fn configs() -> Vec<(&'static str, u32, Oversampled<()>)> {
    let mut out = Vec::new();
    for factor in [2, 3, 4, 6, 8] {
        out.push(("linear", factor, Oversampled::linear((), factor)));
        out.push(("minimum", factor, Oversampled::minimum((), factor)));
    }
    out
}

/// The outer window after the warm-up, for an input tone at `bins`
/// cycles per window (0 for silence) and an interior `body` that sees
/// the inner sample index.
fn window(
    os: &mut Oversampled<()>,
    bins: f64,
    mut body: impl FnMut(usize, f32) -> f32,
) -> Vec<f64> {
    os.prepare(FS as f32);
    let n = os.factor() as usize;
    let mut inner = 0usize;
    let mut out = Vec::with_capacity(W);
    for i in 0..WARM + W {
        let x = (TAU * bins * i as f64 / W as f64).sin() as f32 * 0.5;
        let y = os.tick_with(x, |(), s| {
            let v = body(inner, s);
            inner += 1;
            v
        });
        if i >= WARM {
            out.push(f64::from(y));
        }
    }
    debug_assert_eq!(inner, n * (WARM + W));
    out
}

/// At 44.1 kHz, for every factor and both phases: the passband is flat
/// to 20 kHz; a tone the interior makes anywhere that would fold below
/// 20 kHz is rejected; an input tone's images in the interior are
/// rejected; and the group delay at low frequencies is
/// [`Oversampled::latency_exact`].
///
/// Measured (this test prints them): ripple ≤ 6.7e-4 dB linear (8×),
/// 4e-8 dB minimum (4.3e-4 at 6×, its 3× stage is linear); worst
/// fold-in and image −95.4 dB linear (8×), −111.5 dB minimum (−98.9 at
/// 6×); delay within 4e-6 samples of `latency_exact`. Latencies: linear
/// 66, 66, 84, 73, 87 at 2, 3, 4, 6, 8×; minimum 3.94, 5.63, 10.94
/// (2, 4, 6×) and 6.42 (8×).
#[test]
#[cfg_attr(miri, ignore)]
fn every_factor_meets_its_spec() {
    const RIPPLE_DB: f64 = 0.001;
    const REJECT_DB: f64 = -90.0;
    let bin_hz = FS / W as f64;
    for (phase, factor, mut os) in configs() {
        let n = factor as usize;
        // Passband: steady tones from 30 Hz to 20 kHz through a wire.
        let mut ripple = 0.0f64;
        for hz in [
            30.0, 200.0, 1_000.0, 5_000.0, 10_000.0, 15_000.0, 18_000.0, 20_000.0,
        ] {
            let k = (hz / bin_hz).floor();
            let y = window(&mut os, k, |_, s| s);
            ripple = ripple.max((20.0 * (magnitude(&y, k) / 0.5).log10()).abs());
        }
        // Fold-in: the interior replaces the signal with a tone at
        // j·fs ± f, which decimation would fold onto f < 20 kHz.
        let mut fold = f64::NEG_INFINITY;
        for f in [1_000.0, 9_000.0, 19_900.0] {
            let k = (f / bin_hz).floor();
            for j in 1..=n / 2 {
                for sign in [-1.0, 1.0] {
                    // In cycles per inner window of n·W samples.
                    let cycles = j as f64 * W as f64 + sign * k;
                    if cycles >= (n * W) as f64 / 2.0 {
                        continue;
                    }
                    let y = window(&mut os, 0.0, |i, _| {
                        (TAU * cycles * i as f64 / (n * W) as f64).sin() as f32 * 0.5
                    });
                    fold = fold.max(20.0 * (magnitude(&y, k) / 0.5).log10());
                }
            }
        }
        // Images: an input tone's copies at j·fs ± f in the interior.
        let mut image = f64::NEG_INFINITY;
        for f in [1_000.0, 9_000.0, 19_900.0] {
            let k = (f / bin_hz).floor();
            let mut seen = Vec::new();
            let _ = window(&mut os, k, |i, s| {
                if i >= n * WARM {
                    seen.push(f64::from(s));
                }
                s
            });
            for j in 1..=n / 2 {
                for sign in [-1.0, 1.0] {
                    let cycles = j as f64 * W as f64 + sign * k;
                    if cycles < (n * W) as f64 / 2.0 {
                        image = image.max(20.0 * (magnitude(&seen, cycles) / 0.5).log10());
                    }
                }
            }
        }
        // Delay: the phase of a 32 Hz tone, input to output.
        let k = 3.0;
        let y = window(&mut os, k, |_, s| s);
        let x: Vec<f64> = (WARM..WARM + W)
            .map(|i| (TAU * k * i as f64 / W as f64).sin() * 0.5)
            .collect();
        let ((xr, xi), (yr, yi)) = (bin(&x, k), bin(&y, k));
        let lag = (xi.atan2(xr) - yi.atan2(yr)).rem_euclid(TAU);
        let samples = lag / (TAU * k / W as f64);
        let delay_error = (samples - os.latency_exact()).abs();
        println!(
            "{phase} {factor}x: latency {:.4} ({}), ripple {ripple:.2e} dB, fold-in {fold:.1} dB, image {image:.1} dB, delay error {delay_error:.1e}",
            os.latency_exact(),
            os.latency()
        );
        assert!(ripple < RIPPLE_DB, "{phase} {factor}x: ripple {ripple} dB");
        assert!(fold < REJECT_DB, "{phase} {factor}x: fold-in {fold:.1} dB");
        assert!(image < REJECT_DB, "{phase} {factor}x: image {image:.1} dB");
        assert!(
            delay_error < 0.01,
            "{phase} {factor}x: delay {samples} vs {}",
            os.latency_exact()
        );
    }
}

/// The linear designs' latencies, by structure: 66 for any front, the
/// inner stages' whole base samples on top.
#[test]
fn linear_latencies_are_whole_samples() {
    for (factor, latency) in [
        (1, 0),
        (2, 66),
        (3, 66),
        (4, 84),
        (6, 73),
        (8, 87),
        (9, 70),
        (12, 87),
    ] {
        let os = Oversampled::linear((), factor);
        assert_eq!(os.latency_exact(), latency as f64, "{factor}x");
        assert_eq!(os.latency(), latency, "{factor}x");
    }
}

/// An interior latency that does not divide by the factor is kept as a
/// fraction and rounded half up.
#[test]
fn a_fractional_latency_rounds_to_nearest() {
    let os = Oversampled::linear(OneSample::default(), 2);
    assert_eq!(os.latency_exact(), 66.5);
    assert_eq!(os.latency(), 67);
    let os = Oversampled::linear(OneSample::default(), 3);
    assert!((os.latency_exact() - 66.0 - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(os.latency(), 66);
}

/// `linear(s, 2)` and `linear(s, 4)` are `Oversampled2x`'s front and
/// front-of-nested regions, bit for bit, ticked and in blocks.
#[test]
fn linear_two_and_four_are_oversampled2x() {
    let input: Vec<f32> = (0..3_000).map(|i| (i as f32 * 0.031).sin() * 0.9).collect();
    let mut a2 = Oversampled2x::front(Drive(3.0));
    let mut b2 = Oversampled::linear(Drive(3.0), 2);
    let mut a4 = Oversampled2x::front(Oversampled2x::nested(Drive(3.0)));
    let mut b4 = Oversampled::linear(Drive(3.0), 4);
    for s in [&mut a2 as &mut dyn Stage, &mut b2, &mut a4, &mut b4] {
        s.prepare(48_000.0);
    }
    assert_eq!(a2.latency(), b2.latency());
    assert_eq!(a4.latency(), b4.latency());
    for &x in &input {
        assert_eq!(a2.tick(x).to_bits(), b2.tick(x).to_bits());
        assert_eq!(a4.tick(x).to_bits(), b4.tick(x).to_bits());
    }
    let (mut ba, mut bb) = (input.clone(), input.clone());
    a4.process(&mut ba);
    b4.process(&mut bb);
    assert_eq!(ba, bb);
}

/// The block form is the tick loop, for every structure.
#[test]
fn block_and_tick_agree() {
    let input: Vec<f32> = (0..700).map(|i| (i as f32 * 0.07).sin() * 0.8).collect();
    for factor in [1, 2, 3, 5, 6, 8] {
        for minimum in [false, true] {
            let make = || {
                if minimum {
                    Oversampled::minimum(Drive(4.0), factor)
                } else {
                    Oversampled::linear(Drive(4.0), factor)
                }
            };
            let (mut ticked, mut blocked) = (make(), make());
            ticked.prepare(48_000.0);
            blocked.prepare(48_000.0);
            let t: Vec<f32> = input.iter().map(|&x| ticked.tick(x)).collect();
            let mut b = input.clone();
            for part in b.chunks_mut(97) {
                blocked.process(part);
            }
            assert_eq!(t, b, "{factor}x minimum {minimum}");
        }
    }
}

/// The reported tail bounds the impulse response's ring-out (to
/// −120 dB of its peak) for every structure.
#[test]
fn the_tail_bounds_the_ring_out() {
    for (phase, factor, mut os) in configs() {
        os.prepare(FS as f32);
        let Tail::Samples(tail) = os.tail() else {
            panic!("{phase} {factor}x: tail {:?}", os.tail())
        };
        let ir: Vec<f32> = (0..tail as usize + 4096)
            .map(|i| os.tick(if i == 0 { 1.0 } else { 0.0 }))
            .collect();
        let peak = ir.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let last = ir.iter().rposition(|v| v.abs() > 1e-6 * peak).unwrap_or(0);
        assert!(
            last < tail as usize,
            "{phase} {factor}x: rings to {last}, tail {tail}"
        );
    }
}

/// A closure interior is the same region as the stage it inlines.
#[test]
fn a_closure_interior_matches_its_stage() {
    let input: Vec<f32> = (0..1_000).map(|i| (i as f32 * 0.031).sin() * 0.9).collect();
    let mut staged = Oversampled::minimum(Drive(3.0), 6);
    let mut closed = Oversampled::minimum((), 6);
    staged.prepare(48_000.0);
    closed.prepare(48_000.0);
    for &x in &input {
        let a = staged.tick(x);
        let b = closed.tick_with(x, |(), u| (u * 3.0).tanh());
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

/// A closure interior is the same region as the stage it inlines,
/// sample for sample, at both designs and nested.
#[test]
fn an_oversampled2x_closure_interior_matches_its_stage() {
    let input: Vec<f32> = (0..2_000).map(|i| (i as f32 * 0.031).sin() * 0.9).collect();
    let mut staged = Oversampled2x::front(Oversampled2x::nested(Drive(3.0)));
    let mut closed = Oversampled2x::front(Oversampled2x::nested(()));
    staged.prepare(48_000.0);
    closed.prepare(48_000.0);
    assert_eq!(staged.latency(), closed.latency());
    for &x in &input {
        let a = staged.tick(x);
        let b = closed.tick_with(x, |inner, s| inner.tick_with(s, |(), u| (u * 3.0).tanh()));
        assert_eq!(a.to_bits(), b.to_bits());
    }
}
