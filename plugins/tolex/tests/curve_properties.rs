//! The display law — THE PICTURE CANNOT LIE. The response scene
//! draws curve::response_into; this law renders ACTUAL AUDIO through
//! the engine for several build sheets and pins the drawn curve
//! against the measured 1/6-octave trend. If any display twin drifts
//! from its audio path (motor consts, rear geometry, mode voicing,
//! cliff, tilt, comb), this fails the build.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};
use stave_tolex_vst3::curve::{response_into, Sheet};

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

const GRID_N: usize = 600;

fn log_grid() -> Vec<f32> {
    (0..GRID_N)
        .map(|i| 40.0 * (20_000.0f32 / 40.0).powf(i as f32 / (GRID_N - 1) as f32))
        .collect()
}

/// Measured 1/6-oct trend of a rendered IR at the grid points.
fn measured_trend(ir: &[f32], grid: &[f32]) -> Vec<f64> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    let power: Vec<f64> = buf[..FFT_N / 2]
        .iter()
        .map(|c| f64::from(c.norm_sqr()))
        .collect();
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let half = 2.0f64.powf(1.0 / 12.0);
    grid.iter()
        .map(|f| {
            let f = f64::from(*f);
            let b0 = ((f / half) / bin_hz).floor().max(1.0) as usize;
            let b1 = (((f * half) / bin_hz).ceil() as usize)
                .min(power.len() - 1)
                .max(b0 + 1);
            let mean = power[b0..=b1].iter().sum::<f64>() / (b1 - b0 + 1) as f64;
            10.0 * mean.max(1e-30).log10()
        })
        .collect()
}

/// The drawn curve, smoothed to the SAME 1/6-oct resolution (grid is
/// uniform in log-f: ±1/12 oct is a fixed index window).
fn drawn_trend(sheet: &Sheet, grid: &[f32]) -> Vec<f64> {
    let mut db = vec![0.0f32; grid.len()];
    response_into(sheet, 0.0, grid, &mut db);
    let pts_per_oct = GRID_N as f64 / (20_000.0f64 / 40.0).log2();
    let win = (pts_per_oct / 12.0).round() as usize;
    (0..grid.len())
        .map(|i| {
            let lo = i.saturating_sub(win);
            let hi = (i + win).min(grid.len() - 1);
            let mean_pow = (lo..=hi)
                .map(|j| 10.0f64.powf(f64::from(db[j]) / 10.0))
                .sum::<f64>()
                / (hi - lo + 1) as f64;
            10.0 * mean_pow.max(1e-30).log10()
        })
        .collect()
}

fn normalize(curve: &mut [f64], grid: &[f32]) {
    let sel: Vec<usize> = grid
        .iter()
        .enumerate()
        .filter(|(_, f)| (800.0..=1_250.0).contains(*f))
        .map(|(i, _)| i)
        .collect();
    let m = sel.iter().map(|i| curve[*i]).sum::<f64>() / sel.len() as f64;
    for c in curve.iter_mut() {
        *c -= m;
    }
}

/// The live display must move the way the physics moves: at a hard
/// operating point (excursion env 2.0) the drawn curve SAGS at the
/// resonance (Bl droop = compression) and its teeth SOFTEN (the
/// drive-softening law) — and returns bit-identical at rest.
#[test]
fn the_picture_breathes() {
    let grid = log_grid();
    let sheet = Sheet {
        size: 2,
        variant: 0,
        resonance: 80.0,
        q: 0.85,
        damping: 0.75,
        open: false,
        cab_l: 60.0,
        material: 0,
        model: 0,
        layout: 0,
        mass: 1.0,
        breakup: 0.6,
        doping: 0.35,
        position: 0.25,
        mic_cm: 3.0,
    };
    let mut rest = vec![0.0f32; grid.len()];
    let mut hot = vec![0.0f32; grid.len()];
    response_into(&sheet, 0.0, &grid, &mut rest);
    response_into(&sheet, 2.0, &grid, &mut hot);

    let at = |curve: &[f32], hz: f32| {
        let i = grid.iter().position(|f| *f >= hz).unwrap();
        f64::from(curve[i])
    };
    // Bl droop sags the resonance region (curves share the resting
    // normalization, so the sag shows as level).
    let sag = at(&rest, 110.0) - at(&hot, 110.0);
    assert!(
        sag >= 2.0,
        "hot curve sags only {sag:.2} dB at the resonance"
    );

    // Teeth soften: RESIDUAL (curve minus its own ±1/3-oct trend)
    // shrinks in 0.7-6 kHz — the raw spread is owned by the cliff.
    let pts_per_oct = grid.len() as f64 / (20_000.0f64 / 40.0).log2();
    let win = (pts_per_oct / 3.0).round() as usize;
    let ripple = |curve: &[f32]| {
        let idx: Vec<usize> = grid
            .iter()
            .enumerate()
            .filter(|(_, f)| (700.0..=6_000.0).contains(*f))
            .map(|(i, _)| i)
            .collect();
        let resid: Vec<f64> = idx
            .iter()
            .map(|i| {
                let lo = i.saturating_sub(win);
                let hi = (i + win).min(curve.len() - 1);
                let trend =
                    (lo..=hi).map(|j| f64::from(curve[j])).sum::<f64>() / (hi - lo + 1) as f64;
                f64::from(curve[*i]) - trend
            })
            .collect();
        (resid.iter().map(|d| d * d).sum::<f64>() / resid.len() as f64).sqrt()
    };
    let (r0, r2) = (ripple(&rest), ripple(&hot));
    assert!(
        r2 < r0 * 0.85,
        "teeth did not soften: rest {r0:.2} vs hot {r2:.2} dB spread"
    );
}

#[test]
fn the_picture_tells_the_truth() {
    let grid = log_grid();
    type Case = (&'static str, Vec<(&'static str, f64)>, Sheet);
    let cases: [Case; 3] = [
        (
            "bench",
            vec![],
            Sheet {
                size: 2,
                variant: 0,
                resonance: 80.0,
                q: 0.85,
                damping: 0.75,
                open: false,
                cab_l: 60.0,
                material: 0,
                model: 0,
                layout: 0,
                mass: 1.0,
                breakup: 0.6,
                doping: 0.35,
                position: 0.25,
                mic_cm: 3.0,
            },
        ),
        (
            "open back, far mic, 4x12",
            vec![("back", 1.0), ("mic", 40.0), ("speakers", 2.0)],
            Sheet {
                size: 2,
                variant: 0,
                resonance: 80.0,
                q: 0.85,
                damping: 0.75,
                open: true,
                cab_l: 60.0,
                material: 0,
                model: 0,
                layout: 2,
                mass: 1.0,
                breakup: 0.6,
                doping: 0.35,
                position: 0.25,
                mic_cm: 40.0,
            },
        ),
        (
            "edge mic, heavy cone, pine, SM7",
            vec![
                ("position", 1.0),
                ("mass", 1.5),
                ("material", 1.0),
                ("model", 4.0),
            ],
            Sheet {
                size: 2,
                variant: 0,
                resonance: 80.0,
                q: 0.85,
                damping: 0.75,
                open: false,
                cab_l: 60.0,
                material: 1,
                model: 4,
                layout: 0,
                mass: 1.5,
                breakup: 0.6,
                doping: 0.35,
                position: 1.0,
                mic_cm: 3.0,
            },
        ),
    ];

    let base: Vec<(&'static str, f64)> = vec![
        ("size", 2.0),
        ("resonance", 80.0),
        ("q", 0.85),
        ("damping", 0.75),
        ("back", 0.0),
        ("cab", 60.0),
        ("drive", 0.0),
        ("mic", 3.0),
        ("mass", 1.0),
        ("breakup", 0.6),
        ("doping", 0.35),
        ("position", 0.25),
        ("material", 0.0),
        ("model", 0.0),
        ("speakers", 0.0),
        ("cone", 0.0),
    ];

    for (name, extra, sheet) in cases {
        let mut set = base.clone();
        for (n, v) in extra {
            if let Some(slot) = set.iter_mut().find(|(m, _)| *m == n) {
                slot.1 = v;
            } else {
                set.push((n, v));
            }
        }
        let mut heard = measured_trend(&render_ir(&set), &grid);
        let mut drawn = drawn_trend(&sheet, &grid);
        normalize(&mut heard, &grid);
        normalize(&mut drawn, &grid);

        let idx: Vec<usize> = grid
            .iter()
            .enumerate()
            .filter(|(_, f)| (120.0..=9_000.0).contains(*f))
            .map(|(i, _)| i)
            .collect();
        let diffs: Vec<f64> = idx.iter().map(|i| drawn[*i] - heard[*i]).collect();
        let rms = (diffs.iter().map(|d| d * d).sum::<f64>() / diffs.len() as f64).sqrt();
        let max = diffs.iter().fold(0.0f64, |a, d| a.max(d.abs()));
        assert!(
            rms <= 1.0 && max <= 2.5,
            "[{name}] the picture lies: rms {rms:.2} dB, max {max:.2} dB vs rendered audio"
        );
    }
}
