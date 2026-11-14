//! H8: early digital converters against sampling theory. A tone sampled
//! at the machine's rate `F` (inside a 48 kHz host) appears at `|f − kF|`,
//! held by the DAC with the `sinc(f/F)` droop of a zero-order hold and
//! its images at `kF ± f`. Every tone sits on a bin of a 60 000-sample
//! record at 48 kHz (0.8 Hz bins), and the SP-1200's 26.04 kHz clock puts
//! 32 550 of its samples in that record, so tones, aliases and images
//! are all coherent: one rectangular Goertzel per line, no leakage.

use stave_dsp::circuit::devices::converter::{Converter, Filter, Machine};
use stave_dsp::Stage;
use stave_measure::spectrum::goertzel;
use stave_dsp::units::Hz;

const FS: f32 = 48_000.0;
const N: usize = 60_000;
const BIN: f64 = 48_000.0 / N as f64;
const SETTLE: usize = 4_800;

/// `sin(πx)/(πx)`.
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    }
}

/// The converter's steady output for a tone of `amplitude` on bin `k`.
fn run(machine: Machine, k: usize, amplitude: f64) -> Vec<f64> {
    let mut c = Converter::new(machine);
    c.prepare(FS);
    let w = std::f64::consts::TAU * k as f64 / N as f64;
    (0..SETTLE + N)
        .map(|n| f64::from(c.tick((amplitude * (w * n as f64).sin()) as f32)))
        .skip(SETTLE)
        .collect()
}

/// The level of the line at `hz` (a bin centre), dB re `amplitude`.
fn level_db(y: &[f64], hz: f64, amplitude: f64) -> f64 {
    20.0 * (goertzel(y, hz / 48_000.0) / amplitude).log10()
}

#[test]
fn aliases_land_where_sampling_theory_puts_them() {
    let f = Machine::SP1200.rate_hz;
    let a = 0.5;
    let mut report = Vec::new();
    // In band: the hold's droop alone.
    for k in [2_500usize, 5_000, 10_000] {
        let hz = k as f64 * BIN;
        let y = run(Machine::SP1200, k, a);
        let expect = 20.0 * sinc(hz / f).abs().log10();
        let got = level_db(&y, hz, a);
        report.push((hz, got, expect));
        assert!((got - expect).abs() < 0.02, "{hz} Hz: {got:.3} dB, sinc {expect:.3}");
    }
    // Above the machine's Nyquist, unfiltered: folded to F − f.
    for k in [17_500usize, 20_000, 22_500] {
        let hz = k as f64 * BIN;
        let alias = f - hz;
        let y = run(Machine::SP1200, k, a);
        let expect = 20.0 * sinc(alias / f).abs().log10();
        let got = level_db(&y, alias, a);
        report.push((alias, got, expect));
        assert!((got - expect).abs() < 0.02, "{hz} Hz → {alias} Hz: {got:.3} dB, sinc {expect:.3}");
    }
    eprintln!("(Hz, measured dB, sinc dB): {report:.3?}");
}

#[test]
fn hold_images_follow_the_sinc() {
    let f = Machine::SP1200.rate_hz;
    let a = 0.5;
    let mut report = Vec::new();
    for k in [7_500usize, 10_000, 12_500] {
        let hz = k as f64 * BIN;
        let image = f - hz;
        let y = run(Machine::SP1200, k, a);
        let expect = 20.0 * sinc(image / f).abs().log10();
        let got = level_db(&y, image, a);
        report.push((image, got, expect));
        assert!((got - expect).abs() < 0.1, "image {image} Hz: {got:.3} dB, sinc {expect:.3}");
    }
    eprintln!("images (Hz, measured dB, sinc dB): {report:.3?}");
}

#[test]
fn the_word_holds_its_codes() {
    // A DC input settles to its truncated 12-bit code exactly.
    for x in [0.3f32, -0.71, 0.999] {
        let mut c = Converter::new(Machine::SP1200);
        c.prepare(FS);
        let mut y = 0.0;
        for _ in 0..2_000 {
            y = c.tick(x);
        }
        let code = (f64::from(x) * 2048.0).floor() / 2048.0;
        assert!((f64::from(y) - code).abs() < 1e-6, "{x}: {y} against {code}");
    }
    // Over full scale it saturates at the top code.
    let mut c = Converter::new(Machine::SP1200);
    c.prepare(FS);
    let y = (0..2_000).map(|_| c.tick(3.0)).last().unwrap();
    assert!((f64::from(y) - 2047.0 / 2048.0).abs() < 1e-6, "{y}");
}

#[test]
fn anti_alias_filters_remove_what_they_should() {
    // An 8th-order Butterworth at 11 kHz before the ADC: the 18 kHz tone's
    // alias at 8.04 kHz falls by the filter's magnitude at 18 kHz.
    let k = 22_500;
    let hz = k as f64 * BIN;
    let alias = Machine::SP1200.rate_hz - hz;
    // 24 bits, so a tone the filter leaves 70 dB down is still many
    // steps tall and the comparison linear.
    let wide = Machine::SP1200.with_bits(24);
    let filtered = wide.with_filters(Filter::Butterworth { order: 8, hz: 11_000.0 }, Filter::None);
    let bare = level_db(&run(wide, k, 0.5), alias, 0.5);
    let kept = level_db(&run(filtered, k, 0.5), alias, 0.5);
    let mut c = Converter::new(filtered);
    c.prepare(FS);
    let expect = 20.0 * f64::from(c.input_magnitude_at(Hz::new(hz as f32))).log10();
    assert!(expect < -40.0, "{expect}");
    assert!((kept - bare - expect).abs() < 0.5, "alias {bare:.2} → {kept:.2} dB, filter {expect:.2}");
    // A reconstruction filter takes the image down the same way.
    let image_k = 10_000;
    let image = Machine::SP1200.rate_hz - image_k as f64 * BIN;
    let smoothed = wide.with_filters(Filter::None, Filter::Butterworth { order: 4, hz: 12_000.0 });
    let bare = level_db(&run(wide, image_k, 0.5), image, 0.5);
    let kept = level_db(&run(smoothed, image_k, 0.5), image, 0.5);
    let mut c = Converter::new(smoothed);
    c.prepare(FS);
    let expect = 20.0 * f64::from(c.output_magnitude_at(Hz::new(image as f32))).log10();
    assert!((kept - bare - expect).abs() < 0.5, "image {bare:.2} → {kept:.2} dB, filter {expect:.2}");
}
