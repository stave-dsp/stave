use super::*;
use crate::kernel::node::Stage;
use crate::primitives::filters::probe::phasor;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3, Lagrange5, Linear, Sinc, Thiran};
use crate::units::{hz, seconds};

const RATE: f32 = 48_000.0;

/// E1 (41.2 Hz) to E7 (2637 Hz), every fifth semitone.
fn pitches() -> impl Iterator<Item = f64> {
    (0..=72).step_by(5).map(|k| 41.203_444 * 2f64.powf(f64::from(k) / 12.0))
}

/// The impulse response of `stage`, `len` samples.
fn impulse(stage: &mut impl Stage, len: usize) -> Vec<f64> {
    stage.prepare(RATE);
    (0..len)
        .map(|i| f64::from(stage.tick(if i == 0 { 1.0 } else { 0.0 })))
        .collect()
}

/// `(amplitude, phase)` of the component near `f` in `y[start..start + len]`.
fn component(y: &[f64], start: usize, len: usize, f: f64) -> (f64, f64) {
    let (re, im) = phasor(&y[start..start + len], f64::from(RATE), f);
    ((re * re + im * im).sqrt(), im.atan2(re))
}

/// The frequency of the component near `f` in `y`, by its phase advance
/// over `span` whole nominal periods (so the nominal advance is a whole
/// number of turns), as cents from `f`.
fn cents_off(y: &[f64], f: f64) -> f64 {
    let period = f64::from(RATE) / f;
    let window = (8.0 * period).round() as usize;
    let t1 = (2.0 * period).round() as usize;
    let periods = 50.0;
    let dt = periods * period;
    let t2 = t1 + dt.round() as usize;
    let (_, p1) = component(y, t1, window, f);
    let (_, p2) = component(y, t2, window, f);
    // The two windows start `dt` rounded apart: correct the nominal turn.
    let nominal = core::f64::consts::TAU * f * (t2 - t1) as f64 / f64::from(RATE);
    let mut d = p2 - p1 - nominal;
    d -= core::f64::consts::TAU * (d / core::f64::consts::TAU).round();
    let measured = f + d / (core::f64::consts::TAU * (t2 - t1) as f64 / f64::from(RATE));
    1200.0 * (measured / f).log2()
}

/// The T60 (s) of the component at `f`, by a least-squares line through
/// its level (dB) in windows of four periods of `base` (the loop's
/// fundamental: a pulse train repeats only that often) from `from` to
/// `to` seconds.
fn t60(y: &[f64], f: f64, base: f64, from: f64, to: f64) -> f64 {
    let window = (4.0 * f64::from(RATE) / base).round() as usize;
    let (mut n, mut st, mut sl, mut stt, mut stl) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut start = (from * f64::from(RATE)) as usize;
    while (start + window) as f64 <= to * f64::from(RATE) && start + window <= y.len() {
        let (a, _) = component(y, start, window, f);
        let t = (start + window / 2) as f64 / f64::from(RATE);
        let l = 20.0 * a.log10();
        n += 1.0;
        st += t;
        sl += l;
        stt += t * t;
        stl += t * l;
        start += window;
    }
    let slope = (n * stl - st * sl) / (n * stt - st * st);
    -60.0 / slope
}

fn worst_cents<I: Interpolator>() -> f64 {
    let mut worst = 0.0f64;
    for f in pitches() {
        let mut s = KarplusStrong::<I>::new(hz(40.0), hz(f as f32));
        s.set_decay(seconds(10.0), seconds(2.0));
        let len = ((62.0 / f + 0.01) * f64::from(RATE)) as usize;
        let y = impulse(&mut s, len);
        worst = worst.max(cents_off(&y, f).abs());
    }
    worst
}

#[test]
#[cfg_attr(miri, ignore)]
fn karplus_strong_is_in_tune() {
    let cases = [
        ("Lagrange3", worst_cents::<Lagrange3>(), 0.1),
        ("Lagrange5", worst_cents::<Lagrange5>(), 0.1),
        ("Sinc<8>", worst_cents::<Sinc<8>>(), 0.1),
        ("Thiran", worst_cents::<Thiran>(), 0.1),
        ("Linear", worst_cents::<Linear>(), 0.1),
    ];
    for (name, worst, _) in cases {
        eprintln!("KarplusStrong<{name}>: worst {worst:.4} cents");
    }
    for (name, worst, budget) in cases {
        assert!(worst < budget, "{name}: {worst:.3} cents (budget {budget})");
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn karplus_strong_decays_as_set() {
    let mut worst = 0.0f64;
    for (f, low, high) in [(250.0, 2.0, 0.5), (110.0, 8.0, 1.0), (500.0, 0.6, 0.2), (250.0, 1.0, 1.0)] {
        let mut s = KarplusStrong::<Lagrange3>::new(hz(40.0), hz(f as f32));
        s.set_decay(seconds(low), seconds(high));
        let y = impulse(&mut s, (1.2 * low * f64::from(RATE)) as usize);
        let fundamental = t60(&y, f, f, 0.05, low);
        // The first 30 dB: later, the other partials' leakage floors it.
        let treble = t60(&y, 8.0 * f, f, 0.02, 0.5 * high.min(low));
        for (got, want) in [(fundamental, low), (treble, high)] {
            worst = worst.max((got / want - 1.0).abs());
        }
    }
    eprintln!("KarplusStrong decay: worst {:.3} %", 100.0 * worst);
    assert!(worst < 0.015, "decay {:.2} % off", 100.0 * worst);
}

#[test]
#[cfg_attr(miri, ignore)]
fn comb_is_in_tune_and_decays_as_set() {
    let mut worst = 0.0f64;
    for f in pitches() {
        let mut c = Comb::<Lagrange3>::new(hz(40.0), hz(f as f32), seconds(10.0));
        let len = ((62.0 / f + 0.01) * f64::from(RATE)) as usize;
        worst = worst.max(cents_off(&impulse(&mut c, len), f).abs());
    }
    eprintln!("Comb<Lagrange3>: worst {worst:.4} cents");
    assert!(worst < 0.1, "{worst:.3} cents");

    let mut decay = 0.0f64;
    for t in [0.2, 1.0, 8.0] {
        let mut c = Comb::<Lagrange3>::new(hz(40.0), hz(220.0), seconds(t));
        let y = impulse(&mut c, (1.1 * t * f64::from(RATE)) as usize);
        decay = decay.max((t60(&y, 220.0, 220.0, 0.02, t) / t - 1.0).abs());
    }
    eprintln!("Comb decay: worst {:.3} %", 100.0 * decay);
    assert!(decay < 0.01, "decay {:.2} % off", 100.0 * decay);

    // A resonance passes a sine at unity.
    let mut c = Comb::<Lagrange3>::new(hz(40.0), hz(240.0), seconds(0.2));
    c.prepare(RATE);
    let n = RATE as usize * 2;
    let y: Vec<f64> = (0..n)
        .map(|i| f64::from(c.tick((core::f64::consts::TAU * 240.0 * i as f64 / f64::from(RATE)).sin() as f32)))
        .collect();
    let (a, _) = component(&y, n / 2, n / 2, 240.0);
    assert!((20.0 * a.log10()).abs() < 0.01, "peak {:.4} dB", 20.0 * a.log10());
}

fn modes() -> Vec<Mode> {
    (0..24)
        .map(|k| {
            let t = f64::from(k) / 23.0;
            Mode {
                frequency: hz((40.0 * 400f64.powf(t)) as f32),
                decay: seconds(20.0 * (0.05f64 / 20.0).powf(t)),
                gain: 1.0,
            }
        })
        .collect()
}

#[test]
#[cfg_attr(miri, ignore)]
fn bank_modes_ring_as_set() {
    let (mut cents, mut decay, mut peak) = (0.0f64, 0.0f64, 0.0f64);
    for m in modes() {
        let f = f64::from(m.frequency.get());
        let t = m.decay.get();
        let span = t.min(2.0);
        let mut one = Bank::new(&[m]);
        let len = ((span + 0.02).max(62.0 / f + 0.01) * f64::from(RATE)) as usize;
        let y = impulse(&mut one, len);
        cents = cents.max(cents_off(&y, f).abs());
        decay = decay.max((t60(&y, f, f.max(200.0), 0.0, span) / t - 1.0).abs());
        if t <= 1.0 {
            // The peak gain: the impulse response's DFT at the mode.
            let y = impulse(&mut one, (8.0 * t * f64::from(RATE)) as usize);
            let (mut re, mut im) = (0.0, 0.0);
            for (n, v) in y.iter().enumerate() {
                let (s, c) = (core::f64::consts::TAU * f * n as f64 / f64::from(RATE)).sin_cos();
                re += v * c;
                im -= v * s;
            }
            peak = peak.max((20.0 * (re * re + im * im).sqrt().log10()).abs());
        }
    }
    eprintln!("Bank: {cents:.5} cents, decay {:.3} %, peak {peak:.4} dB", 100.0 * decay);
    assert!(cents < 0.01, "{cents} cents");
    assert!(decay < 0.005, "decay {:.3} % off", 100.0 * decay);
    assert!(peak < 0.01, "peak {peak:.4} dB");

    // The bank is the sum of its modes, and retuning carries on.
    let all = modes();
    let mut bank = Bank::new(&all);
    let sum = impulse(&mut bank, 4_800);
    let mut parts = vec![0.0; 4_800];
    for m in &all {
        for (p, v) in parts.iter_mut().zip(impulse(&mut Bank::new(&[*m]), 4_800)) {
            *p += v;
        }
    }
    let worst = sum.iter().zip(&parts).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    assert!(worst < 1e-4, "sum of modes off by {worst:e}");
    bank.set(3, Mode { frequency: hz(500.0), decay: seconds(1.0), gain: 0.5 });
    assert_eq!(bank.modes()[3].gain, 0.5);
    assert!(bank.tick(0.0).is_finite());
}

#[test]
fn tails_cover_the_ring() {
    let mut s = KarplusStrong::<Lagrange3>::new(hz(40.0), hz(440.0));
    s.set_decay(seconds(0.3), seconds(0.1));
    let y = impulse(&mut s, 48_000);
    let Some(n) = s.tail().to_samples() else { panic!() };
    let last = y.iter().rposition(|v| v.abs() > 1e-6).unwrap();
    assert!(last <= n as usize, "rang {last}, reported {n}");
    let mut b = Bank::new(&[Mode { frequency: hz(300.0), decay: seconds(0.2), gain: 1.0 }]);
    let y = impulse(&mut b, 48_000);
    let Some(n) = b.tail().to_samples() else { panic!() };
    let last = y.iter().rposition(|v| v.abs() > 1e-6).unwrap();
    assert!(last <= n as usize, "rang {last}, reported {n}");
}
