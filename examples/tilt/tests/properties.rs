//! Tilt's OWN laws — the promises only this project can state:
//! * the SIGN: positive tilt is BRIGHTER (highs up, lows down) —
//!   the first build shipped this backwards past the whole generic
//!   gate stack; only a measurement catches a semantic flip;
//! * the PIVOT holds still (±0.1 dB at 650 Hz at full tilt);
//! * tilt 0 is BIT-FLAT (true bypass, not a unity recursion).

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_tilt_vst3::Tilt::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

fn sine(freq: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / FS).sin() * 0.1)
        .collect()
}

/// Steady-state RMS gain (dB) of a rendered sine vs its input,
/// skipping the first half (filter + ramp settling).
fn gain_db(freq: f32, tilt: f64) -> f64 {
    let input = sine(freq, 48_000);
    let out = render(&[("tilt", tilt)], &input);
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
    let out = render(&[("tilt", 0.0)], &input);
    for (i, (x, y)) in input.iter().zip(&out).enumerate() {
        assert!(x.to_bits() == y.to_bits(), "sample {i}: {x} vs {y}");
    }
}
