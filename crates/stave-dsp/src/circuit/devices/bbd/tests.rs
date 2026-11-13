use super::*;
use crate::circuit::topologies::bucket_brigade::{BucketBrigade, INPUT_FILTER, OUTPUT_FILTER};
use crate::kernel::node::Stage;
use crate::units::{hz, seconds};
use core::f64::consts::{PI, TAU};

fn sallen_key_line(chip: Chip, rate: f32, clock: f32) -> HoltersParker {
    let input = Filter::sallen_key(&INPUT_FILTER).unwrap();
    let output = Filter::sallen_key(&OUTPUT_FILTER).unwrap();
    let mut line = HoltersParker::new(chip, input, output);
    line.prepare(rate);
    line.set_noise(0.0);
    line.set_feedthrough(0.0);
    line.reset();
    line.set_clock(hz(clock));
    line
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// The complex amplitude of `hz` in `buf` at `rate` (exact for whole
/// cycles): `(magnitude, phase)` of `a·cos(ωn + φ)`.
fn tone(buf: &[f64], hz: f64, rate: f64) -> (f64, f64) {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &x) in buf.iter().enumerate() {
        let w = TAU * hz * n as f64 / rate;
        re += x * w.cos();
        im -= x * w.sin();
    }
    let k = 2.0 / buf.len() as f64;
    ((re * re + im * im).sqrt() * k, im.atan2(re))
}

/// `settle` then `len` samples of `line` driven by `cos(2π·hz·n/rate)`,
/// the window starting at a whole number of input cycles.
fn run(line: &mut HoltersParker, hz: f64, rate: f64, settle: usize, len: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(len);
    for n in 0..settle + len {
        let y = line.tick_f64((TAU * hz * n as f64 / rate).cos());
        if n >= settle {
            out.push(y);
        }
    }
    out
}

fn wrap(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

fn phase(p: crate::circuit::methods::dk::Phasor) -> f64 {
    p.im.atan2(p.re)
}

fn magnitude(p: crate::circuit::methods::dk::Phasor) -> f64 {
    p.re.hypot(p.im)
}

/// The paper's own check (Holters & Parker, Fig. 7): N = 256 stages at
/// 50 kHz, fs 44.1 kHz, a 1 kHz sine through the Juno-60 filters
/// against `a·sin(ωk + φ)`, eqs. 36–38.
#[test]
fn matches_the_papers_sine_through_the_juno_60_filters() {
    let chip = Chip { stages: 256, ..Chip::MN3007 };
    let mut line = HoltersParker::new(chip, Filter::JUNO_60_INPUT, Filter::JUNO_60_OUTPUT);
    let rate = 44_100.0;
    line.prepare(rate as f32);
    line.set_noise(0.0);
    line.set_feedthrough(0.0);
    line.reset();
    line.set_clock(hz(50_000.0));
    let (f0, fbbd, n) = (1_000.0, 50_000.0, 256.0);
    let hin = Filter::JUNO_60_INPUT.response(f0);
    let hout = Filter::JUNO_60_OUTPUT.response(f0);
    let a = sinc(f0 / fbbd) * magnitude(hin) * magnitude(hout);
    let phi = -PI * f0 * n / fbbd + phase(hin) + phase(hout);
    let mut worst: f64 = 0.0;
    for k in 0..(0.02 * rate) as usize {
        let y = line.tick_f64((TAU * f0 * k as f64 / rate).sin());
        if k as f64 > 0.005 * rate {
            let want = a * (TAU * f0 * k as f64 / rate + phi).sin();
            worst = worst.max((y - want).abs());
        }
    }
    println!("Fig. 7 setup: worst difference {worst:.2e} (the paper's plot: about 1e-3)");
    assert!(worst < 1e-3, "{worst}");
}

/// The delay is `stages / (2·clock)`: a 100 Hz tone's phase, less the
/// filters' phase, read as time, across the chip's clock range.
#[test]
fn delay_follows_the_clock() {
    let rate = 48_000.0;
    let f0 = 100.0;
    for clock in [10_000.0, 25_000.0, 50_000.0, 100_000.0] {
        let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
        let out = run(&mut line, f0, rate, 9_600, 48_000);
        let (_, measured) = tone(&out, f0, rate);
        // The window starts at sample 9 600, a whole number of cycles.
        let filters = phase(line.input_filter().response(f0)) + phase(line.output_filter().response(f0));
        let expected_s = Chip::MN3007.delay_s(hz(clock as f32));
        let delay_phase = wrap(measured - filters);
        // Unwrap against the expected delay.
        let turns = ((-TAU * f0 * expected_s - delay_phase) / TAU).round();
        let got_s = -(delay_phase + turns * TAU) / (TAU * f0);
        let error = (got_s - expected_s) / expected_s;
        println!("clock {clock} Hz: delay {:.4} ms, expected {:.4} ms ({:+.3} %)", got_s * 1e3, expected_s * 1e3, 100.0 * error);
        assert!(error.abs() < 1e-4, "{clock} Hz: {error}");
    }
}

/// A tone's gain is `|H_in|·|H_out|·sinc(f/clock)` (eq. 37), at 48 and
/// 96 kHz, at two clocks.
#[test]
fn response_is_the_filters_and_the_hold() {
    for (rate, clock) in [(48_000.0, 40_000.0), (48_000.0, 100_000.0), (96_000.0, 40_000.0)] {
        let mut worst: f64 = 0.0;
        for f0 in [100.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 6_000.0, 8_000.0] {
            let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
            let out = run(&mut line, f0, rate, (0.06 * rate) as usize, (0.1 * rate) as usize);
            let (got, _) = tone(&out, f0, rate);
            let want = magnitude(line.input_filter().response(f0))
                * magnitude(line.output_filter().response(f0))
                * sinc(f0 / clock);
            let error = 20.0 * (got / want).log10();
            worst = worst.max(error.abs());
        }
        println!("fs {rate}, clock {clock}: worst gain error {worst:.4} dB");
        assert!(worst < 0.01, "fs {rate}, clock {clock}: {worst} dB");
    }
}

/// Above half the clock a tone folds to `clock − f`; below, the
/// staircase images it there. Both at the level the filters and the
/// hold predict (fs 96 kHz, clock 40 kHz, so both land below Nyquist).
#[test]
fn aliases_and_images_follow_the_clock() {
    let (rate, clock) = (96_000.0, 40_000.0);
    let fin = Filter::sallen_key(&INPUT_FILTER).unwrap();
    let fout = Filter::sallen_key(&OUTPUT_FILTER).unwrap();
    let h = |f: &Filter, hz: f64| magnitude(f.response(hz));
    // A 25 kHz tone aliases to 15 kHz.
    let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
    let out = run(&mut line, 25_000.0, rate, 9_600, 96_000);
    let (alias, _) = tone(&out, 15_000.0, rate);
    let want = h(&fin, 25_000.0) * sinc(15_000.0 / clock) * h(&fout, 15_000.0);
    let alias_error = 20.0 * (alias / want).log10();
    // A 15 kHz tone images to 25 kHz.
    let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
    let out = run(&mut line, 15_000.0, rate, 9_600, 96_000);
    let (image, _) = tone(&out, 25_000.0, rate);
    let want_image = h(&fin, 15_000.0) * sinc(25_000.0 / clock).abs() * h(&fout, 25_000.0);
    let image_error = 20.0 * (image / want_image).log10();
    println!(
        "alias at 15 kHz: {:.1} dB (predicted {:.1}); image at 25 kHz: {:.1} dB (predicted {:.1})",
        20.0 * alias.log10(),
        20.0 * want.log10(),
        20.0 * image.log10(),
        20.0 * want_image.log10()
    );
    assert!(alias_error.abs() < 0.2, "alias {alias_error} dB");
    assert!(image_error.abs() < 0.2, "image {image_error} dB");
    // And the alias moves with the clock: at 45 kHz it lands at 20 kHz.
    let mut line = sallen_key_line(Chip::MN3007, rate as f32, 45_000.0);
    let out = run(&mut line, 25_000.0, rate, 9_600, 96_000);
    let (moved, _) = tone(&out, 20_000.0, rate);
    let (stale, _) = tone(&out, 15_000.0, rate);
    assert!(moved > 100.0 * stale, "{moved} at 20 kHz, {stale} at 15 kHz");
}

/// Idle noise: the chip's quoted S/N on the pin, held for a clock period
/// and shaped by the output filter — its power `σ²/clock · ∫|H_out|²·sinc²`.
#[test]
fn noise_floor_is_the_datasheet_s_n() {
    let (rate, clock) = (48_000.0, 40_000.0);
    let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
    line.set_noise(Chip::MN3007.noise_volts());
    let out: Vec<f64> = (0..(4.0 * rate) as usize).map(|_| line.tick_f64(0.0)).collect();
    let measured = (out[4_800..].iter().map(|x| x * x).sum::<f64>() / (out.len() - 4_800) as f64).sqrt();
    let fout = line.output_filter();
    let (mut integral, df) = (0.0, 5.0);
    let mut f = 0.5 * df;
    while f < 2e6 {
        integral += magnitude(fout.response(f)).powi(2) * sinc(f / clock).powi(2) * df;
        f += df;
    }
    let sigma = Chip::MN3007.noise_volts();
    let predicted = sigma * (2.0 * integral / clock).sqrt();
    let error = 20.0 * (measured / predicted).log10();
    println!(
        "noise: {:.2} µV RMS measured, {:.2} µV predicted ({:+.2} dB); S/N re the nominal level: {:.1} dB at the output",
        measured * 1e6,
        predicted * 1e6,
        error,
        20.0 * (Chip::MN3007.nominal.get() / measured).log10()
    );
    assert!(error.abs() < 0.2, "{error} dB");
}

/// The clock's residue: a square wave of the feedthrough's amplitude at
/// the clock, through the output filter (fs 192 kHz so it lies below
/// Nyquist).
#[test]
fn feedthrough_is_the_clock_through_the_output_filter() {
    let (rate, clock) = (192_000.0, 40_000.0);
    let mut line = sallen_key_line(Chip::MN3007, rate as f32, clock as f32);
    line.set_feedthrough(0.1);
    let out: Vec<f64> = (0..(0.5 * rate) as usize).map(|_| line.tick_f64(0.0)).collect();
    let (got, _) = tone(&out[19_200..], clock, rate);
    let want = 4.0 / PI * 0.1 * magnitude(line.output_filter().response(clock));
    let error = 20.0 * (got / want).log10();
    println!("feedthrough at the clock: {:.1} dBV, predicted {:.1} dBV", 20.0 * got.log10(), 20.0 * want.log10());
    assert!(error.abs() < 0.1, "{error} dB");
}

/// A clock step glides the pitch (Fig. 8): no discontinuity. While the
/// buckets filled at 50 kHz drain at 25 kHz (the glide), the output's
/// steepest step stays within 10 % of the steady states either side (5 %
/// measured) — a digital line's jump would be several times it. (Those
/// include the staircase's images, so neither is the input's slope.)
#[test]
fn a_clock_step_glides() {
    let rate = 44_100.0;
    let mut line = sallen_key_line(Chip { stages: 256, ..Chip::MN3007 }, rate as f32, 50_000.0);
    let f0 = 1_000.0;
    let step = (0.010 * rate) as usize;
    // The glide ends when the last 50 kHz bucket leaves: 256/(2·25k).
    let glide_end = step + (0.006 * rate) as usize;
    let mut last = 0.0;
    let (mut before, mut glide, mut after) = (0.0f64, 0.0f64, 0.0f64);
    for k in 0..(0.04 * rate) as usize {
        if k == step {
            line.set_clock(hz(25_000.0));
        }
        let y = line.tick_f64((TAU * f0 * k as f64 / rate).sin());
        let d = (y - last).abs();
        last = y;
        if k < (0.005 * rate) as usize {
            continue; // the onset's filter transient
        }
        match k {
            _ if k < step => before = before.max(d),
            _ if k < glide_end => glide = glide.max(d),
            _ => after = after.max(d),
        }
    }
    println!("steepest step: {before:.4} at 50 kHz, {glide:.4} gliding, {after:.4} at 25 kHz");
    assert!(glide <= 1.1 * before.max(after), "{glide}");
}

#[test]
fn stays_finite_and_rings_out() {
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for clock in [5_000.0, 10_000.0, 100_000.0, 1e6] {
            let mut line = sallen_key_line(Chip::MN3205, rate, clock);
            for n in 0..20_000 {
                let x = if n < 5_000 { if n % 7 < 3 { 2.0 } else { -2.0 } } else { 0.0 };
                assert!(line.tick(x).is_finite());
            }
            assert!(line.tail().to_samples().is_some());
        }
    }
    // The clock clamps to the chip's range.
    let mut line = sallen_key_line(Chip::MN3007, 48_000.0, 1e6);
    assert_eq!(line.clock(), Chip::MN3007.clock_max);
    line.set_delay(seconds(1.0));
    assert_eq!(line.clock(), Chip::MN3007.clock_min);
    line.set_delay(seconds(0.02));
    assert!((line.delay().get() - 0.02).abs() < 1e-6);
}

/// With the compander in, a steady tone leaves at the line's gain
/// squared (the expander doubles the line's loss in dB), from −40 to
/// 0 dB re the NE570's unity level.
#[test]
fn compander_tracks_through_the_line() {
    let rate = 48_000.0;
    let f0 = 1_000.0;
    let unity = crate::circuit::devices::compander::Chip::NE570.unity.get();
    for level in [-40.0, -20.0, 0.0] {
        let mut bbd = BucketBrigade::new(Chip::MN3007);
        bbd.prepare(rate as f32);
        bbd.line().set_noise(0.0);
        bbd.line().set_feedthrough(0.0);
        bbd.reset();
        bbd.set_clock(hz(40_000.0));
        let amp = unity * 10f64.powf(level / 20.0) * 2f64.sqrt();
        let n = (0.6 * rate) as usize;
        let out: Vec<f64> =
            (0..n).map(|k| bbd.tick_f64(amp * (TAU * f0 * k as f64 / rate).cos())).collect();
        let (got, _) = tone(&out[n - (0.2 * rate) as usize..], f0, rate);
        let line = Filter::sallen_key(&INPUT_FILTER).unwrap().response(f0);
        let line = magnitude(line) * magnitude(Filter::sallen_key(&OUTPUT_FILTER).unwrap().response(f0)) * sinc(f0 / 40_000.0);
        let error = 20.0 * (got / (amp * line * line)).log10();
        println!("compander at {level} dB: {error:+.3} dB re the line's gain squared");
        assert!(error.abs() < 0.05, "{level} dB: {error}");
    }
}

/// The compander buries the line's noise: idle output with and without.
#[test]
fn compander_lowers_the_line_noise() {
    let rate = 48_000.0;
    let idle = |compander: bool| {
        let mut bbd = BucketBrigade::new(Chip::MN3007);
        bbd.prepare(rate);
        bbd.set_compander(compander);
        bbd.line().set_feedthrough(0.0);
        let out: Vec<f64> = (0..96_000).map(|_| bbd.tick_f64(0.0)).collect();
        (out[48_000..].iter().map(|x| x * x).sum::<f64>() / 48_000.0).sqrt()
    };
    let (with, without) = (idle(true), idle(false));
    println!("idle noise: {:.1} dBV without the compander, {:.1} dBV with", 20.0 * without.log10(), 20.0 * with.log10());
    assert!(with < without * 1e-3, "{with} vs {without}");
}
