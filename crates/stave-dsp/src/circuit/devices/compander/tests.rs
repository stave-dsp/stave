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

#[test]
fn a_hot_onset_rides_the_rail_until_the_rectifier_catches_up() {
    // 2 V DC after silence: the compressor's gain is at its maximum, so
    // the output the loop needs passes the cell's saturation and the
    // op-amp slews to its rail; the rectifier, saturated, charges toward
    // its limit until the gain is cell/x, and the loop closes again.
    let chip = Chip::NE570;
    let (x, cell, limit, swing) = (2.0, chip.cell.get(), chip.rectifier_limit.get(), chip.swing.get());
    let avg_unity = chip.unity.get() * AVERAGE_OF_RMS;
    let floor = avg_unity * 10f64.powf(-f64::from(chip.range.get()) / 20.0);
    let closes = avg_unity * x / cell;
    let want = chip.rectifier.get() * ((limit - floor) / (limit - closes)).ln();
    for rate in [8_000.0f32, 44_100.0, 48_000.0, 96_000.0, 192_000.0, 768_000.0] {
        let mut c = Compander::new(chip, Mode::Compress);
        c.prepare(rate);
        let n = (0.05 * f64::from(rate)) as usize;
        let out: Vec<f64> = (0..n).map(|_| c.tick_f64(x)).collect();
        let peak = out.iter().fold(0.0f64, |m, y| m.max(y.abs()));
        assert!(peak <= swing, "{rate} Hz: {peak} V past the swing");
        let railed = out.iter().filter(|&&y| y >= swing - 1e-9).count() as f64 / f64::from(rate);
        // A sample's resolution, and the slew from 0 V to the rail.
        let slack = 1.0 / f64::from(rate) + swing / chip.slew_rate;
        assert!((railed - want).abs() <= slack, "{rate} Hz: {railed} s on the rail, want {want} s");
        // Then the 2:1 law takes over: the output falls toward its
        // steady level, never past the cell's ceiling once it has slewed
        // down from the rail.
        let down = ((swing - cell) / chip.slew_rate * f64::from(rate)).ceil() as usize;
        let last = out.iter().rposition(|&y| y >= swing - 1e-9).unwrap_or(0);
        let after = &out[last + down + 2..];
        assert!(after.iter().all(|y| y.abs() <= cell + 1e-9), "{rate} Hz: past the cell after closing");
    }
}

#[test]
fn the_output_stays_inside_its_swing() {
    let swing = Chip::NE570.swing.get();
    for mode in [Mode::Compress, Mode::Expand] {
        for rate in [8_000.0f32, 48_000.0, 768_000.0] {
            let mut c = Compander::new(Chip::NE570, mode);
            c.prepare(rate);
            let n = (0.2 * rate) as usize;
            for i in 0..n {
                // Silence, a 20 V square burst, silence, then 20 V DC.
                let x = match i * 8 / n {
                    0 | 3 => 0.0,
                    1 | 2 => {
                        if i % 37 < 18 {
                            20.0
                        } else {
                            -20.0
                        }
                    }
                    _ => 20.0,
                };
                let y = c.tick_f64(x);
                assert!(y.abs() <= swing, "{mode:?} at {rate} Hz: {y} V");
            }
        }
    }
}

#[test]
fn the_slew_rate_is_the_sheets() {
    // From rest at 768 kHz, a hot step: the output climbs at 0.5 V/µs.
    let mut c = Compander::new(Chip::NE570, Mode::Compress);
    c.prepare(768_000.0);
    let y: Vec<f64> = (0..4).map(|_| c.tick_f64(1.0)).collect();
    let slope = (y[3] - y[0]) / 3.0 * 768_000.0;
    assert!((slope / Chip::NE570.slew_rate - 1.0).abs() < 1e-9, "{slope} V/s");
}
