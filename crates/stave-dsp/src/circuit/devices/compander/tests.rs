use super::*;
use stave_measure::spectrum::goertzel;

const RATE: f32 = 48_000.0;

/// The RMS of a steady 1 kHz sine of RMS `rms` through `stage`, measured
/// coherently over the last 0.25 s of 1 s.
fn level_through(stage: &mut impl FnMut(f64) -> f64, rms: f64) -> f64 {
    let n = RATE as usize;
    let amp = rms * core::f64::consts::SQRT_2;
    let out: Vec<f64> = (0..n)
        .map(|i| stage(amp * (core::f64::consts::TAU * 1_000.0 * i as f64 / f64::from(RATE)).sin()))
        .collect();
    let tail = &out[3 * n / 4..];
    goertzel(tail, 1_000.0 / f64::from(RATE)) / core::f64::consts::SQRT_2
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

fn half(mode: Mode) -> Compander {
    let mut c = Compander::new(Chip::NE570, mode);
    c.prepare(RATE);
    c
}

#[test]
fn compressor_halves_the_level_in_db() {
    let unity = Chip::NE570.unity.get();
    for level in [-60.0, -40.0, -20.0, -6.0, 0.0, 10.0] {
        let mut c = half(Mode::Compress);
        let out = level_through(&mut |x| c.tick_f64(x), unity * 10f64.powf(level / 20.0));
        let got = db(out / unity);
        assert!((got - level / 2.0).abs() < 0.05, "{level} dB in: {got} dB out, want {}", level / 2.0);
    }
}

#[test]
fn expander_doubles_the_level_in_db() {
    let unity = Chip::NE570.unity.get();
    for level in [-30.0, -20.0, -10.0, -3.0, 0.0, 5.0] {
        let mut c = half(Mode::Expand);
        let out = level_through(&mut |x| c.tick_f64(x), unity * 10f64.powf(level / 20.0));
        let got = db(out / unity);
        assert!((got - 2.0 * level).abs() < 0.05, "{level} dB in: {got} dB out, want {}", 2.0 * level);
    }
}

#[test]
fn compressor_then_expander_is_unity() {
    let unity = Chip::NE570.unity.get();
    for level in [-50.0, -30.0, -10.0, 0.0, 6.0] {
        let (mut c, mut e) = (half(Mode::Compress), half(Mode::Expand));
        let rms = unity * 10f64.powf(level / 20.0);
        let out = level_through(&mut |x| e.tick_f64(c.tick_f64(x)), rms);
        assert!(db(out / rms).abs() < 0.05, "{level} dB: {} dB through the pair", db(out / rms));
    }
}

#[test]
fn rectifier_settles_in_its_time_constant() {
    // The expander's gain is its rectifier: a DC step from silence
    // reaches 63 % of its final gain after one time constant.
    let mut e = half(Mode::Expand);
    let tau = (Chip::NE570.rectifier.get() * f64::from(RATE)).round() as usize;
    for _ in 0..tau {
        e.tick_f64(1.0);
    }
    let at_tau = e.gain();
    for _ in 0..20 * tau {
        e.tick_f64(1.0);
    }
    let fraction = at_tau / e.gain();
    assert!((fraction - (1.0 - (-1.0f64).exp())).abs() < 0.05 * 0.632, "{fraction}");
}

#[test]
fn silence_stays_silent_and_finite() {
    for mode in [Mode::Compress, Mode::Expand] {
        let mut c = half(mode);
        for _ in 0..1_000 {
            assert_eq!(c.tick(0.0), 0.0);
        }
        // Full scale after silence: the compressor's maximum gain is
        // bounded, and nothing blows up.
        for i in 0..48_000 {
            let y = c.tick(if i % 100 < 50 { 1.0 } else { -1.0 });
            assert!(y.is_finite());
        }
    }
}
