//! THE ANCHOR FIT — coarse search of the physical space
//! against a named reference capture. Prints the best build sheets
//! and their spectral distances; the winners become factory anchor
//! presets. Documents, does not block; skips without refs or in
//! debug. Objective: rms of the 1/6-oct trend difference plus half
//! the 1/12-oct fine difference, 120 Hz-9 kHz, both re the 1 kHz
//! band — macro shape first, tooth placement second.

use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const IR_LEN: usize = 24_064;
const FFT_N: usize = 32_768;
const GRID_N: usize = 500;

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
    let (mut in_l, mut in_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut out_l, mut out_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
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

struct Curves {
    trend: Vec<f64>,
    fine: Vec<f64>,
}

fn curves(ir: &[f32], fft: &std::sync::Arc<dyn rustfft::Fft<f32>>) -> Curves {
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    let power: Vec<f64> = buf[..FFT_N / 2]
        .iter()
        .map(|c| f64::from(c.norm_sqr()))
        .collect();
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let grid: Vec<f64> = (0..GRID_N)
        .map(|i| 80.0 * (12_000.0f64 / 80.0).powf(i as f64 / (GRID_N - 1) as f64))
        .collect();
    let smooth = |frac: f64| -> Vec<f64> {
        let half = 2.0f64.powf(1.0 / (2.0 * frac));
        let mut out: Vec<f64> = grid
            .iter()
            .map(|f| {
                let b0 = ((f / half) / bin_hz).floor().max(1.0) as usize;
                let b1 = (((f * half) / bin_hz).ceil() as usize)
                    .min(power.len() - 1)
                    .max(b0 + 1);
                let mean = power[b0..=b1].iter().sum::<f64>() / (b1 - b0 + 1) as f64;
                10.0 * mean.max(1e-30).log10()
            })
            .collect();
        let sel: Vec<usize> = grid
            .iter()
            .enumerate()
            .filter(|(_, f)| (800.0..=1_250.0).contains(*f))
            .map(|(i, _)| i)
            .collect();
        let m = sel.iter().map(|i| out[*i]).sum::<f64>() / sel.len() as f64;
        for o in out.iter_mut() {
            *o -= m;
        }
        out
    };
    Curves {
        trend: smooth(6.0),
        fine: smooth(12.0),
    }
}

fn distance_parts(a: &Curves, b: &Curves) -> (f64, f64) {
    let grid: Vec<f64> = (0..GRID_N)
        .map(|i| 80.0 * (12_000.0f64 / 80.0).powf(i as f64 / (GRID_N - 1) as f64))
        .collect();
    let idx: Vec<usize> = grid
        .iter()
        .enumerate()
        .filter(|(_, f)| (120.0..=9_000.0).contains(*f))
        .map(|(i, _)| i)
        .collect();
    let rms = |x: &Vec<f64>, y: &Vec<f64>| {
        (idx.iter().map(|i| (x[*i] - y[*i]).powi(2)).sum::<f64>() / idx.len() as f64).sqrt()
    };
    (rms(&a.trend, &b.trend), rms(&a.fine, &b.fine))
}

fn distance(a: &Curves, b: &Curves) -> f64 {
    let grid: Vec<f64> = (0..GRID_N)
        .map(|i| 80.0 * (12_000.0f64 / 80.0).powf(i as f64 / (GRID_N - 1) as f64))
        .collect();
    let idx: Vec<usize> = grid
        .iter()
        .enumerate()
        .filter(|(_, f)| (120.0..=9_000.0).contains(*f))
        .map(|(i, _)| i)
        .collect();
    let rms = |x: &Vec<f64>, y: &Vec<f64>| {
        (idx.iter().map(|i| (x[*i] - y[*i]).powi(2)).sum::<f64>() / idx.len() as f64).sqrt()
    };
    rms(&a.trend, &b.trend) + 0.5 * rms(&a.fine, &b.fine)
}

#[test]
fn anchor_diagnose() {
    if cfg!(debug_assertions) {
        return;
    }
    let target_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "refs/york/York Audio YORK 212 M65 Cream v1.00/YA YORK 212 M65/48k (Fractal, Line 6, etc.)/Natural Phase Singles/YA YORK 212 M65 57m-CNT.wav",
    );
    let Ok((_, chans)) =
        stave_measure::wav::read(&target_path).map(|a| (a.sample_rate, a.channels))
    else {
        return;
    };
    let mut target_ir = chans[0].clone();
    target_ir.truncate(IR_LEN);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let target = curves(&target_ir, &fft);
    for (name, cone, fs, q, bk, dp, pos) in [
        ("hash-21 best", 21.0, 100.0, 1.1, 0.65, 0.15, 0.05),
        ("anchor natural", 32.0, 100.0, 1.1, 0.6, 0.35, 0.10),
    ] {
        let set: Vec<(&'static str, f64)> = vec![
            ("size", 2.0),
            ("resonance", fs),
            ("q", q),
            ("mass", 1.0),
            ("damping", 0.5),
            ("drive", 0.0),
            ("mic", 3.0),
            ("cab", 100.0),
            ("speakers", 1.0),
            ("back", 0.0),
            ("cone", cone),
            ("breakup", bk),
            ("doping", dp),
            ("position", pos),
        ];
        let c = curves(&render_ir(&set), &fft);
        let (t, f) = distance_parts(&c, &target);
        eprintln!(
            "  {name}: trend rms {t:.2}  fine rms {f:.2}  total {:.2}",
            t + 0.5 * f
        );
    }
}

#[test]
fn anchor_fit_for_the_record() {
    if cfg!(debug_assertions) {
        eprintln!("anchor fit: skipped in debug");
        return;
    }
    let target_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "refs/york/York Audio YORK 212 M65 Cream v1.00/YA YORK 212 M65/48k (Fractal, Line 6, etc.)/Natural Phase Singles/YA YORK 212 M65 57m-CNT.wav",
    );
    let Ok((sr, chans)) =
        stave_measure::wav::read(&target_path).map(|a| (a.sample_rate, a.channels))
    else {
        eprintln!("anchor fit: reference not present — skipped");
        return;
    };
    assert_eq!(sr, FS as u32);
    let mut target_ir = chans[0].clone();
    target_ir.truncate(IR_LEN);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let target = curves(&target_ir, &fft);

    // Stage 1: which CONES are candidates (cone is categorical —
    // no neighbor continuity; the physical dims refine later).
    type Sheet = [(&'static str, f64); 15];
    let sheet =
        |cone: f64, res: f64, q: f64, mass: f64, damp: f64, bk: f64, dp: f64, back: f64| -> Sheet {
            [
                ("size", 2.0),
                ("resonance", res),
                ("q", q),
                ("mass", mass),
                ("damping", damp),
                ("drive", 0.0),
                ("mic", 3.0),
                ("cab", 100.0),
                ("speakers", 1.0),
                ("back", back),
                ("cone", cone),
                ("breakup", bk),
                ("doping", dp),
                ("material", 0.0),
                ("model", 0.0),
            ]
        };
    let eval = |set: &Sheet, pos: f64| {
        let mut v: Vec<(&'static str, f64)> = set.to_vec();
        v.push(("position", pos));
        distance(&curves(&render_ir(&v), &fft), &target)
    };

    let mut stage1: Vec<(f64, usize)> = (0..32usize)
        .map(|c| {
            let mut best = f64::INFINITY;
            for bk in [0.5f64, 0.65] {
                for pos in [0.05f64, 0.15, 0.3] {
                    let s = sheet(c as f64, 80.0, 0.85, 1.0, 0.75, bk, 0.3, 0.0);
                    best = best.min(eval(&s, pos));
                }
            }
            (best, c)
        })
        .collect();
    stage1.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    eprintln!("== stage 1: candidate cones {:?} ==", &stage1[..3]);
    // The measured anchor always contends.
    stage1.insert(0, (0.0, 32));

    // Stage 2: free the PHYSICAL dims around the candidate cones.
    // (distance, cone, res, q, mass, damp, breakup, doping, position)
    type Refined = (f64, usize, f64, f64, f64, f64, f64, f64, f64);
    let mut best: Vec<Refined> = Vec::new();
    for &(_, cone) in stage1.iter().take(3) {
        for res in [70.0f64, 80.0, 90.0, 100.0] {
            for q in [0.6f64, 0.85, 1.1] {
                for mass in [0.85f64, 1.0, 1.2] {
                    for damp in [0.5f64, 0.9] {
                        for bk in [0.5f64, 0.65, 0.8] {
                            for dp in [0.15f64, 0.3, 0.45] {
                                for pos in [0.05f64, 0.15] {
                                    let s = sheet(cone as f64, res, q, mass, damp, bk, dp, 0.0);
                                    let d = eval(&s, pos);
                                    best.push((d, cone, res, q, mass, damp, bk, dp, pos));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    best.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    eprintln!("== stage 2: {} refined sheets — top 5 ==", best.len());
    for (d, c, res, q, m, dmp, bk, dp, pos) in best.iter().take(5) {
        eprintln!(
            "  dist {d:.3}  cone {c:2}  fs {res:.0}  q {q:.2}  mass {m:.2}  damp {dmp:.2}  breakup {bk:.2}  doping {dp:.2}  pos {pos:.2}"
        );
    }
}
