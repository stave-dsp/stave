//! The FIDELITY REPORT — tolex measured against a population of
//! real measured IRs (plugins/tolex/docs/design.md, the linear leg's precursor).
//! Not curve-matching: the question is whether tolex's response has
//! the same KIND of structure a real cabinet has — fine-structure
//! density, ripple energy, corner behavior, decay shape — scored
//! against the DISTRIBUTION of a reference pack, not any one member.
//!
//! Like the perf report, it DOCUMENTS, it does not block. Reference IRs live untracked in refs/york/ (commercial
//! content — never committed); the test skips gracefully without
//! them, and skips in debug builds (thousands of FFTs).
//!
//! Run: cargo test --release -p stave-tolex-vst3 --test fidelity -- --nocapture

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const IR_LEN: usize = 24_000; // 0.5 s — the York single length
const FFT_N: usize = 32_768;

// ===================================================================
// Reference discovery — 48k natural-phase single-mic IRs only
// ===================================================================

fn collect_refs(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_refs(&p, out);
        } else if p.extension().is_some_and(|x| x == "wav") {
            let s = p.to_string_lossy();
            if s.contains("48k") && s.contains("Natural Phase Singles") {
                out.push(p);
            }
        }
    }
}

// ===================================================================
// Analysis — log-frequency magnitude + structure metrics
// ===================================================================

/// Log-spaced evaluation grid, 40 Hz – 20 kHz.
fn log_grid() -> Vec<f64> {
    let (lo, hi, n) = (40.0f64, 20_000.0f64, 600);
    (0..n)
        .map(|i| lo * (hi / lo).powf(i as f64 / (n - 1) as f64))
        .collect()
}

/// Power spectrum of an IR, zero-padded to FFT_N.
fn power_spectrum(ir: &[f32], fft: &std::sync::Arc<dyn rustfft::Fft<f32>>) -> Vec<f64> {
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    buf[..FFT_N / 2]
        .iter()
        .map(|c| f64::from(c.norm_sqr()))
        .collect()
}

/// Fractional-octave smoothed dB curve on the log grid: mean POWER
/// over a ±1/(2N)-octave rectangular window per eval point.
fn smoothed_db(power: &[f64], grid: &[f64], frac_oct: f64) -> Vec<f64> {
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let half = 2.0f64.powf(1.0 / (2.0 * frac_oct));
    grid.iter()
        .map(|f| {
            let b0 = ((f / half) / bin_hz).floor().max(1.0) as usize;
            let b1 = (((f * half) / bin_hz).ceil() as usize).min(power.len() - 1);
            let b1 = b1.max(b0 + 1);
            let mean = power[b0..=b1].iter().sum::<f64>() / (b1 - b0 + 1) as f64;
            10.0 * mean.max(1e-30).log10()
        })
        .collect()
}

struct Metrics {
    /// Local extrema (±1 dB prominence vs the 1/3-oct trend) per
    /// octave, 700 Hz – 6 kHz: the fine-structure density that IS a
    /// real cone's fingerprint.
    extrema_per_oct: f64,
    /// RMS of the fine-structure residual (1/24 oct − 1/3 oct), dB,
    /// 700 Hz – 6 kHz.
    ripple_mid_db: f64,
    /// Same residual RMS, 100 – 600 Hz (box/reflection comb region).
    ripple_low_db: f64,
    /// −12 dB (re 1 kHz reference band) high corner on the 1/3-oct
    /// trend, Hz.
    hf_corner_hz: f64,
    /// Trend slope over the octave above the corner, dB/oct.
    hf_slope_db_oct: f64,
    /// −6 dB low corner below 200 Hz on the trend, Hz.
    lf_corner_hz: f64,
    /// Time for 90% of total energy, ms.
    t90_ms: f64,
    /// Energy after 10 ms vs total, dB — reflections + modal ring.
    tail_db: f64,
}

fn analyze(ir: &[f32], grid: &[f64], fft: &std::sync::Arc<dyn rustfft::Fft<f32>>) -> Metrics {
    let power = power_spectrum(ir, fft);
    let fine = smoothed_db(&power, grid, 24.0);
    let trend = smoothed_db(&power, grid, 3.0);

    // Reference level: trend mean over 800–1200 Hz.
    let ref_db = {
        let pts: Vec<f64> = grid
            .iter()
            .zip(&trend)
            .filter(|(f, _)| **f >= 800.0 && **f <= 1200.0)
            .map(|(_, d)| *d)
            .collect();
        pts.iter().sum::<f64>() / pts.len() as f64
    };

    let residual: Vec<f64> = fine.iter().zip(&trend).map(|(a, b)| a - b).collect();
    let band = |lo: f64, hi: f64| {
        grid.iter()
            .enumerate()
            .filter(move |(_, f)| **f >= lo && **f <= hi)
    };

    let rms_band = |lo: f64, hi: f64| {
        let vals: Vec<f64> = band(lo, hi).map(|(i, _)| residual[i]).collect();
        (vals.iter().map(|x| x * x).sum::<f64>() / vals.len() as f64).sqrt()
    };

    // Extrema: sign flips of the residual's slope with ±1 dB reach.
    let idx: Vec<usize> = band(700.0, 6_000.0).map(|(i, _)| i).collect();
    let mut extrema = 0usize;
    for w in idx.windows(3) {
        let (a, b, c) = (residual[w[0]], residual[w[1]], residual[w[2]]);
        let peak = b > a && b > c && b > 1.0;
        let dip = b < a && b < c && b < -1.0;
        if peak || dip {
            extrema += 1;
        }
    }
    let octaves = (6_000.0f64 / 700.0).log2();

    // HF corner: first trend crossing of ref −12 dB above 1.5 kHz.
    let mut hf_corner = 20_000.0;
    for (i, f) in grid.iter().enumerate() {
        if *f > 1_500.0 && trend[i] < ref_db - 12.0 {
            hf_corner = *f;
            break;
        }
    }
    // Slope over the octave above the corner.
    let at = |hz: f64| {
        let i = grid.iter().position(|f| *f >= hz).unwrap_or(grid.len() - 1);
        trend[i]
    };
    let hf_slope = at(hf_corner) - at((hf_corner * 2.0).min(19_000.0));

    // LF corner: walking DOWN from 200 Hz, ref is the 150–300 trend.
    let lf_ref = {
        let pts: Vec<f64> = band(150.0, 300.0).map(|(i, _)| trend[i]).collect();
        pts.iter().sum::<f64>() / pts.len() as f64
    };
    let mut lf_corner = 40.0;
    for (i, f) in grid.iter().enumerate().rev() {
        if *f < 200.0 && trend[i] < lf_ref - 6.0 {
            lf_corner = *f;
            break;
        }
    }

    // Time structure.
    let total: f64 = ir.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    let mut acc = 0.0;
    let mut t90 = ir.len();
    for (n, x) in ir.iter().enumerate() {
        acc += f64::from(*x) * f64::from(*x);
        if acc >= 0.9 * total {
            t90 = n;
            break;
        }
    }
    let after_10ms: f64 = ir[(0.010 * FS as f64) as usize..]
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum();

    Metrics {
        extrema_per_oct: extrema as f64 / octaves,
        ripple_mid_db: rms_band(700.0, 6_000.0),
        ripple_low_db: rms_band(100.0, 600.0),
        hf_corner_hz: hf_corner,
        hf_slope_db_oct: hf_slope,
        lf_corner_hz: lf_corner,
        t90_ms: t90 as f64 / f64::from(FS) * 1_000.0,
        tail_db: 10.0 * (after_10ms.max(1e-30) / total.max(1e-30)).log10(),
    }
}

// ===================================================================
// Tolex's own IR — the in-process render (properties.rs harness shape)
// ===================================================================

fn tolex_ir() -> Vec<f32> {
    let mut plugin = stave_tolex_vst3::Tolex::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    // The bench cabinet: 12" / fs 80 / Qts 0.85 / closed 60 L /
    // moderate tube damping / drive at reference / close mic.
    for (n, v) in [
        ("size", 2.0),
        ("resonance", 80.0),
        ("q", 0.85),
        ("damping", 0.75),
        ("back", 0.0),
        ("cab", 60.0),
        ("drive", 0.0),
        ("mic", 3.0),
        ("mass", 1.0),
        ("cone", 0.0),
        ("breakup", 0.6),
        ("doping", 0.35),
        ("position", 0.25),
        ("material", 0.0),
        ("speakers", 0.0),
        ("model", 0.0),
    ] {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(v);
    }
    plugin.activate(FS, BLOCK);

    // A tiny impulse keeps the motor in its linear region — this is
    // the LINEAR snapshot, the thing an IR actually is.
    let mut out = Vec::with_capacity(IR_LEN + BLOCK);
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    let blocks = IR_LEN.div_ceil(BLOCK);
    for b in 0..blocks {
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
    out.truncate(IR_LEN);
    out
}

// ===================================================================
// The report
// ===================================================================

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

#[test]
fn fidelity_report_for_the_record() {
    if cfg!(debug_assertions) {
        eprintln!("fidelity: skipped in debug (run --release)");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("refs/york");
    let mut files = Vec::new();
    collect_refs(&root, &mut files);
    if files.is_empty() {
        eprintln!("fidelity: no reference IRs at refs/york — skipped");
        return;
    }
    files.sort();

    let grid = log_grid();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);

    // Export the model's IR for the corpus instrument (analysis/
    // aggregate.py section 9): model-vs-population rows must come
    // from the SAME measurement code, or the comparison is biased.
    let ir = tolex_ir();
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("refs/analysis/out");
    if std::fs::create_dir_all(&out_dir).is_ok() {
        let bytes: Vec<u8> = ir.iter().flat_map(|x| x.to_le_bytes()).collect();
        let _ = std::fs::write(out_dir.join("tolex_ir.f32"), bytes);
    }

    let mut pop: Vec<Metrics> = Vec::with_capacity(files.len());
    let mut skipped = 0usize;
    for f in &files {
        let Ok((sr, chans)) = stave_measure::wav::read(f).map(|a| (a.sample_rate, a.channels))
        else {
            skipped += 1;
            continue;
        };
        if sr != FS as u32 || chans.is_empty() || chans[0].len() < 4_800 {
            skipped += 1;
            continue;
        }
        let mut ir = chans[0].clone();
        ir.truncate(IR_LEN);
        pop.push(analyze(&ir, &grid, &fft));
    }
    let tolex = analyze(&ir, &grid, &fft);

    println!(
        "tolex fidelity vs York population ({} IRs, {} skipped, 12 cabs, 48k natural-phase singles)",
        pop.len(),
        skipped
    );
    println!(
        "{:<26} {:>8} {:>8} {:>8} {:>9}  verdict",
        "metric", "p10", "median", "p90", "tolex"
    );

    type Row = (&'static str, fn(&Metrics) -> f64);
    let rows: [Row; 8] = [
        ("extrema/oct 0.7-6k", |m| m.extrema_per_oct),
        ("ripple rms dB 0.7-6k", |m| m.ripple_mid_db),
        ("ripple rms dB 0.1-0.6k", |m| m.ripple_low_db),
        ("hf corner Hz (-12dB)", |m| m.hf_corner_hz),
        ("hf slope dB/oct", |m| m.hf_slope_db_oct),
        ("lf corner Hz (-6dB)", |m| m.lf_corner_hz),
        ("t90 ms", |m| m.t90_ms),
        ("tail(>10ms) dB", |m| m.tail_db),
    ];

    for (name, get) in rows {
        let mut vals: Vec<f64> = pop.iter().map(get).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (p10, med, p90) = (
            percentile(&vals, 0.10),
            percentile(&vals, 0.50),
            percentile(&vals, 0.90),
        );
        let t = get(&tolex);
        let verdict = if t < p10 {
            "OUTSIDE (low)"
        } else if t > p90 {
            "OUTSIDE (high)"
        } else {
            "inside"
        };
        println!("{name:<26} {p10:>8.2} {med:>8.2} {p90:>8.2} {t:>9.2}  {verdict}");
    }
}
