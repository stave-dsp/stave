//! Tilt's OWN laws — the promises only this project can state:
//! * the SIGN: positive tilt is BRIGHTER (highs up, lows down) —
//!   the first build shipped this backwards past the whole generic
//!   gate stack; only a measurement catches a semantic flip;
//! * the PIVOT holds still (±0.1 dB at 650 Hz at full tilt);
//! * tilt 0 is BIT-FLAT (true bypass, not a unity recursion).

use stave_test::PluginHarness;
use stave_tilt_vst3::Tilt;

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK: usize = 256;

fn run(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut h = PluginHarness::<Tilt>::new(SAMPLE_RATE, BLOCK);
    let table = h.table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_value(*v);
    }
    let mut out = Vec::with_capacity(input.len());
    for chunk in input.chunks(BLOCK) {
        for ch in h.inputs_mut() {
            ch[..chunk.len()].copy_from_slice(chunk);
            ch[chunk.len()..].fill(0.0);
        }
        h.run_block(&[]);
        out.extend_from_slice(&h.outputs()[0]);
    }
    out
}

fn sine(freq: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE).sin() * 0.1)
        .collect()
}

/// Steady-state RMS gain (dB) of a processed sine vs its input,
/// skipping the first half (filter + ramp settling).
fn gain_db(freq: f32, tilt: f64) -> f64 {
    let input = sine(freq, 48_000);
    let out = run(&[("Tilt", tilt)], &input);
    let rms = |w: &[f32]| {
        (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
    };
    20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10()
}

/// Positive tilt is BRIGHTER: at full tilt the highs gain what the
/// lows lose (within the shelf's voiced corner), and mirroring the
/// knob mirrors the curve.
#[test]
fn the_seesaw_tips_the_right_way() {
    let lo = gain_db(100.0, 6.0);
    let hi = gain_db(5_000.0, 6.0);
    assert!(
        hi > 5.0 && lo < -5.0,
        "tilt +6 must brighten: lo {lo:.2} dB / hi {hi:.2} dB"
    );
    let lo_dark = gain_db(100.0, -6.0);
    let hi_dark = gain_db(5_000.0, -6.0);
    assert!(
        lo_dark > 5.0 && hi_dark < -5.0,
        "tilt −6 must darken: lo {lo_dark:.2} dB / hi {hi_dark:.2} dB"
    );
}

/// The pivot holds still: full tilt moves 650 Hz less than 0.1 dB.
#[test]
fn the_pivot_holds_still() {
    for t in [-6.0, 6.0] {
        let g = gain_db(650.0, t);
        assert!(g.abs() < 0.1, "pivot drifted {g:.3} dB at tilt {t}");
    }
}

/// Tilt 0 is BIT-FLAT: a true bypass, sample for sample.
#[test]
fn zero_tilt_is_bit_flat() {
    let input = sine(1_000.0, 12_800);
    let out = run(&[("Tilt", 0.0)], &input);
    for (i, (x, y)) in input.iter().zip(&out).enumerate() {
        assert!(x.to_bits() == y.to_bits(), "sample {i}: {x} vs {y}");
    }
}
