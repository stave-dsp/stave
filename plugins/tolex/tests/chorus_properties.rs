//! law — THE CHORUS COMBS AT THE MIC. A multi-speaker cab is
//! mostly GEOMETRY: neighbor drivers reach the mic over longer
//! paths and comb against the near cone. The law pins the physics:
//! the first null lands where the path difference says
//! (f₀ = c / 2Δ, Δ = √(r² + d²) − r for 12" centers d = 0.33 m),
//! the comb DEEPENS as the mic pulls back (the real reason close-
//! mic'd 4×12s sound like 1×12s), and more drivers comb harder.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const IR_LEN: usize = 24_064;
const FFT_N: usize = 32_768;

fn render_ir(set: &[(&'static str, f64)]) -> Vec<f32> {
    let mut plugin = stave_tolex_vst3::Tolex::new();
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
    let mut out = Vec::with_capacity(IR_LEN);
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for b in 0..(IR_LEN / BLOCK) {
        in_l.fill(0.0);
        if b == 0 {
            in_l[0] = 0.01;
        }
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

/// Narrow-band gain (dB) around `hz` (±1/12 oct power mean).
fn gain_db(ir: &[f32], hz: f64) -> f64 {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let half = 2.0f64.powf(1.0 / 24.0);
    let b0 = ((hz / half) / bin_hz).floor() as usize;
    let b1 = ((hz * half) / bin_hz).ceil() as usize;
    let mean = buf[b0..=b1]
        .iter()
        .map(|c| f64::from(c.norm_sqr()))
        .sum::<f64>()
        / (b1 - b0 + 1) as f64;
    10.0 * mean.max(1e-30).log10()
}

const BENCH: &[(&str, f64)] = &[
    ("size", 2.0),
    ("resonance", 80.0),
    ("q", 0.85),
    ("damping", 0.75),
    ("back", 0.0),
    ("cab", 60.0),
    ("drive", 0.0),
    ("mass", 1.0),
    ("cone", 0.0),
    ("breakup", 0.6),
    ("doping", 0.35),
    ("position", 0.25),
    ("material", 0.0),
    ("speakers", 0.0),
    ("model", 0.0),
];

fn with(base: &[(&'static str, f64)], extra: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    let mut v: Vec<(&'static str, f64)> = Vec::new();
    for (n, x) in base.iter().chain(extra.iter()) {
        if let Some(slot) = v.iter_mut().find(|(m, _)| m == n) {
            slot.1 = *x;
        } else {
            v.push((n, *x));
        }
    }
    v
}

#[test]
fn the_chorus_combs_at_the_mic() {
    // The physics, restated: 12" centers d = 0.33 m; at mic r the
    // neighbor's extra path is Δ = √(r² + d²) − r; first null at
    // c / 2Δ.
    let (r, d, c) = (0.40f64, 0.33f64, 343.0f64);
    let delta = (r * r + d * d).sqrt() - r;
    let f_null = c / (2.0 * delta); // ≈ 1.46 kHz

    let one_far = render_ir(&with(BENCH, &[("mic", 40.0), ("speakers", 0.0)]));
    let four_far = render_ir(&with(BENCH, &[("mic", 40.0), ("speakers", 2.0)]));
    let two_far = render_ir(&with(BENCH, &[("mic", 40.0), ("speakers", 1.0)]));
    let d4 = gain_db(&four_far, f_null) - gain_db(&one_far, f_null);
    let d2 = gain_db(&two_far, f_null) - gain_db(&one_far, f_null);
    assert!(
        d4 <= -2.0,
        "4x12 null at {f_null:.0} Hz only {d4:.2} dB — geometry not combing"
    );
    assert!(
        d2 > d4,
        "2x12 ({d2:.2} dB) must comb SHALLOWER than 4x12 ({d4:.2} dB)"
    );

    // Close-mic'd, the neighbors nearly vanish — the real reason a
    // close 4x12 sounds like its one nearest cone.
    let one_close = render_ir(&with(BENCH, &[("mic", 3.0), ("speakers", 0.0)]));
    let four_close = render_ir(&with(BENCH, &[("mic", 3.0), ("speakers", 2.0)]));
    let (r, _) = (0.03f64, d);
    let delta_c = (r * r + d * d).sqrt() - r;
    let f_null_c = c / (2.0 * delta_c);
    let dc = gain_db(&four_close, f_null_c) - gain_db(&one_close, f_null_c);
    assert!(
        dc.abs() <= 1.2,
        "close-mic'd 4x12 moved {dc:.2} dB at its null — neighbors should nearly vanish"
    );
}
