//! laws — THE VOICE, bracketed by the corpus.
//! Written before voice.rs. Each law states a population fact the
//! modal bank must reproduce, measured on tolex's own rendered IR:
//!
//! * the_cone_grows_teeth — fine structure exists: ≥5 extrema/oct
//!   (±1 dB) and ≥1.2 dB ripple RMS in 0.7–6 kHz (population:
//!   6–10/oct, 1.6–2.8 dB at this instrument's thresholds).
//! * identity_survives_the_walk — moving the mic cap→edge keeps
//!   45–90% of spectral peaks in place (corpus: 69% persist) AND
//!   moves at least 10% — a position knob that changes nothing, or
//!   scrambles everything, both fail.
//! * position_walks_darker — cap→edge tilts ≥3 dB darker (corpus:
//!   −4.6 dB swing) and never brighter.
//! * mass_slides_the_cliff — heavier cone, lower breakup: doubling
//!   mass drops the −12 dB corner by ≥15%.
//! * doping_damps_the_song — edge damping eats the ripple: doping
//!   0.9 shows ≥20% less ripple RMS than 0.1.
//! * the_depth_band_rings — the 1–1.3 kHz band decays over 40–150 ms
//!   (population median 75 ms; the analysis-filter floor is ~18 ms,
//!   so ≥40 is comfortably a real ring).

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const IR_LEN: usize = 24_064; // block-aligned (the properties.rs lesson)
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

// ---- the same measurement shapes as tests/fidelity.rs ------------

fn log_grid() -> Vec<f64> {
    let (lo, hi, n) = (40.0f64, 20_000.0f64, 600);
    (0..n)
        .map(|i| lo * (hi / lo).powf(i as f64 / (n - 1) as f64))
        .collect()
}

fn power_spectrum(ir: &[f32]) -> Vec<f64> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    buf[..FFT_N / 2]
        .iter()
        .map(|c| f64::from(c.norm_sqr()))
        .collect()
}

fn smoothed_db(power: &[f64], grid: &[f64], frac_oct: f64) -> Vec<f64> {
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let half = 2.0f64.powf(1.0 / (2.0 * frac_oct));
    grid.iter()
        .map(|f| {
            let b0 = ((f / half) / bin_hz).floor().max(1.0) as usize;
            let b1 = (((f * half) / bin_hz).ceil() as usize)
                .min(power.len() - 1)
                .max(b0 + 1);
            let mean = power[b0..=b1].iter().sum::<f64>() / (b1 - b0 + 1) as f64;
            10.0 * mean.max(1e-30).log10()
        })
        .collect()
}

struct Fine {
    grid: Vec<f64>,
    resid: Vec<f64>,
    trend: Vec<f64>,
}

fn fine_structure(ir: &[f32]) -> Fine {
    let grid = log_grid();
    let power = power_spectrum(ir);
    let fine = smoothed_db(&power, &grid, 24.0);
    let trend = smoothed_db(&power, &grid, 3.0);
    let resid = fine.iter().zip(&trend).map(|(a, b)| a - b).collect();
    Fine { grid, resid, trend }
}

fn band_idx(grid: &[f64], lo: f64, hi: f64) -> Vec<usize> {
    grid.iter()
        .enumerate()
        .filter(|(_, f)| **f >= lo && **f <= hi)
        .map(|(i, _)| i)
        .collect()
}

fn extrema_per_oct(fs: &Fine) -> f64 {
    let idx = band_idx(&fs.grid, 700.0, 6_000.0);
    let mut n = 0usize;
    for w in idx.windows(3) {
        let (a, b, c) = (fs.resid[w[0]], fs.resid[w[1]], fs.resid[w[2]]);
        if (b > a && b > c && b > 1.0) || (b < a && b < c && b < -1.0) {
            n += 1;
        }
    }
    n as f64 / (6_000.0f64 / 700.0).log2()
}

fn ripple_rms(fs: &Fine) -> f64 {
    let idx = band_idx(&fs.grid, 700.0, 6_000.0);
    (idx.iter().map(|i| fs.resid[*i] * fs.resid[*i]).sum::<f64>() / idx.len() as f64).sqrt()
}

fn peak_freqs(fs: &Fine) -> Vec<f64> {
    let idx = band_idx(&fs.grid, 700.0, 6_000.0);
    let mut out = Vec::new();
    for w in idx.windows(3) {
        let (a, b, c) = (fs.resid[w[0]], fs.resid[w[1]], fs.resid[w[2]]);
        if b > a && b > c && b > 1.0 {
            out.push(fs.grid[w[1]]);
        }
    }
    out
}

fn tilt_db(fs: &Fine) -> f64 {
    let hi = band_idx(&fs.grid, 2_500.0, 4_000.0);
    let lo = band_idx(&fs.grid, 300.0, 600.0);
    hi.iter().map(|i| fs.trend[*i]).sum::<f64>() / hi.len() as f64
        - lo.iter().map(|i| fs.trend[*i]).sum::<f64>() / lo.len() as f64
}

fn hf_corner(fs: &Fine) -> f64 {
    let rf = band_idx(&fs.grid, 800.0, 1_200.0);
    let ref_db = rf.iter().map(|i| fs.trend[*i]).sum::<f64>() / rf.len() as f64;
    for (i, f) in fs.grid.iter().enumerate() {
        if *f > 1_500.0 && fs.trend[i] < ref_db - 12.0 {
            return *f;
        }
    }
    20_000.0
}

/// Schroeder T20×3 (ms) of the 1–1.3 kHz band — RBJ bandpass pair.
fn depth_band_decay_ms(ir: &[f32]) -> f64 {
    let (fc, q) = (1_130.0f64, 4.0f64);
    let w0 = 2.0 * std::f64::consts::PI * fc / f64::from(FS);
    let alpha = w0.sin() / (2.0 * q);
    let (b0, b1, b2) = (alpha, 0.0, -alpha);
    let a0 = 1.0 + alpha;
    let (a1, a2) = (-2.0 * w0.cos(), 1.0 - alpha);
    let mut y = vec![0.0f64; ir.len()];
    for pass in 0..2 {
        let src: Vec<f64> = if pass == 0 {
            ir.iter().map(|x| f64::from(*x)).collect()
        } else {
            y.clone()
        };
        let (mut z1, mut z2) = (0.0f64, 0.0f64);
        for (i, x) in src.iter().enumerate() {
            let o = (b0 * x + z1) / a0;
            z1 = b1 * x - a1 * o + z2;
            z2 = b2 * x - a2 * o;
            y[i] = o;
        }
    }
    let e: Vec<f64> = y.iter().rev().map(|v| v * v).collect();
    let mut edc: Vec<f64> = e
        .iter()
        .scan(0.0, |acc, v| {
            *acc += v;
            Some(*acc)
        })
        .collect();
    edc.reverse();
    let top = edc[0].max(1e-30);
    let db: Vec<f64> = edc
        .iter()
        .map(|v| 10.0 * (v / top).max(1e-12).log10())
        .collect();
    let i5 = db.iter().position(|d| *d <= -5.0).unwrap_or(0);
    let i25 = db.iter().position(|d| *d <= -25.0).unwrap_or(db.len() - 1);
    (i25.saturating_sub(i5)) as f64 / f64::from(FS) * 3_000.0
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
    ("mic", 3.0),
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
fn the_cone_grows_teeth() {
    let fs = fine_structure(&render_ir(BENCH));
    let ext = extrema_per_oct(&fs);
    let rip = ripple_rms(&fs);
    assert!(
        ext >= 5.0,
        "extrema {ext:.2}/oct — population floor is ~6, gate at 5"
    );
    assert!(
        rip >= 1.2,
        "ripple {rip:.2} dB RMS — population floor is ~1.6, gate at 1.2"
    );
}

#[test]
fn identity_survives_the_walk() {
    let cap = fine_structure(&render_ir(&with(BENCH, &[("position", 0.0)])));
    let edge = fine_structure(&render_ir(&with(BENCH, &[("position", 1.0)])));
    let (pa, pb) = (peak_freqs(&cap), peak_freqs(&edge));
    assert!(
        pa.len() >= 8 && pb.len() >= 8,
        "too few peaks to judge: {} / {}",
        pa.len(),
        pb.len()
    );
    let tol = 2.0f64.powf(1.0 / 24.0);
    let matched = pa
        .iter()
        .filter(|f| pb.iter().any(|g| **f / g < tol && g / **f < tol))
        .count() as f64
        / pa.len() as f64;
    assert!(
        (0.45..=0.90).contains(&matched),
        "peak persistence cap->edge {matched:.2} — corpus says ~0.69: too low is scramble, too high is a dead knob"
    );
}

#[test]
fn position_walks_darker() {
    let t0 = tilt_db(&fine_structure(&render_ir(&with(
        BENCH,
        &[("position", 0.0)],
    ))));
    let t1 = tilt_db(&fine_structure(&render_ir(&with(
        BENCH,
        &[("position", 1.0)],
    ))));
    let swing = t1 - t0;
    assert!(
        swing <= -3.0,
        "cap->edge tilt swing {swing:.2} dB — corpus swing is -4.6"
    );
}

#[test]
fn mass_slides_the_cliff() {
    let light = hf_corner(&fine_structure(&render_ir(&with(BENCH, &[("mass", 0.5)]))));
    let heavy = hf_corner(&fine_structure(&render_ir(&with(BENCH, &[("mass", 2.0)]))));
    let ratio = heavy / light;
    assert!(
        ratio <= 0.85,
        "4x mass moved the corner by x{ratio:.2} — must drop >=15%"
    );
}

#[test]
fn doping_damps_the_song() {
    let wild = ripple_rms(&fine_structure(&render_ir(&with(
        BENCH,
        &[("doping", 0.1)],
    ))));
    let tame = ripple_rms(&fine_structure(&render_ir(&with(
        BENCH,
        &[("doping", 0.9)],
    ))));
    assert!(
        tame <= 0.8 * wild,
        "doping 0.1 -> 0.9 ripple {wild:.2} -> {tame:.2} dB — must damp >=20%"
    );
}

#[test]
fn the_depth_band_rings() {
    let t = depth_band_decay_ms(&render_ir(BENCH));
    assert!(
        (40.0..=150.0).contains(&t),
        "1-1.3 kHz decay {t:.1} ms — population median 75, instrument floor ~18: must be a real ring"
    );
}
