//! THE CROSS-MACHINE TIMBRE GATE — the metal meter (built when
//! the plate shipped ringing like a cheap Schroeder) applied to
//! EVERY topology. New machines pass through here or they do not
//! ship: late-tail SPECTRAL FLATNESS (a wash reads high, ringing
//! partials read low) and COMB/LINE REGULARITY (a periodic or
//! sparse-line spectrum is a pitched-metallic percept).
//! Excitation is a broadband sustained burst so charge-based
//! machines (MEMORY) are fed fairly.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32], extra_blocks: usize) -> Vec<f32> {
    let mut plugin = stave_vault_vst3::Vault::new();
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
    let blocks = input.len().div_ceil(BLOCK) + extra_blocks;
    let mut l = Vec::new();
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for b in 0..blocks {
        for i in 0..BLOCK {
            in_l[i] = input.get(b * BLOCK + i).copied().unwrap_or(0.0);
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
        l.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    l
}

/// 0.6 s broadband burst (sustained — feeds charge machines).
fn burst() -> Vec<f32> {
    let mut rng = 0x7137u32;
    (0..(0.6 * FS) as usize)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3
        })
        .collect()
}

fn late_spectrum(out: &[f32], from_s: f64, bins: usize, f0: f64, f1: f64) -> Vec<f64> {
    let a = (from_s * f64::from(FS)) as usize;
    let b = (a + (0.5 * FS) as usize).min(out.len());
    let w = &out[a..b];
    let n = w.len();
    (0..bins)
        .map(|k| {
            let hz = f0 + (f1 - f0) * k as f64 / (bins - 1) as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &x) in w.iter().enumerate() {
                let hann = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
                let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
                re += f64::from(x) * hann * ph.cos();
                im += f64::from(x) * hann * ph.sin();
            }
            (re * re + im * im).sqrt()
        })
        .collect()
}

fn flatness(mag: &[f64]) -> f64 {
    let pow: Vec<f64> = mag.iter().map(|m| (m * m).max(1e-30)).collect();
    let logmean = pow.iter().map(|p| p.ln()).sum::<f64>() / pow.len() as f64;
    let mean = pow.iter().sum::<f64>() / pow.len() as f64;
    logmean.exp() / mean
}

fn comb_regularity(mag: &[f64], bin_hz: f64) -> f64 {
    let lm: Vec<f64> = mag.iter().map(|m| m.max(1e-15).ln()).collect();
    let mean = lm.iter().sum::<f64>() / lm.len() as f64;
    let d: Vec<f64> = lm.iter().map(|x| x - mean).collect();
    let e0: f64 = d.iter().map(|x| x * x).sum();
    let (lag_min, lag_max) = ((25.0 / bin_hz) as usize, (400.0 / bin_hz) as usize);
    let mut best = 0.0f64;
    for lag in lag_min.max(1)..lag_max.min(d.len() / 2) {
        let acc: f64 = (0..d.len() - lag).map(|i| d[i] * d[i + lag]).sum();
        best = best.max(acc / e0.max(1e-30));
    }
    best
}

/// EVERY machine's late tail must be a wash, not a bell: flatness
/// and comb-regularity within the room's own class (the reference
/// that sounds right).
#[test]
fn every_machine_passes_the_metal_meter() {
    let mut fails: Vec<String> = Vec::new();
    for (name, topo) in [
        ("ROOM", 0.0),
        ("PLATE", 1.0),
        ("MEMORY", 2.0),
        ("DETONATE", 3.0),
        ("VOID", 4.0),
    ] {
        let mut set = vec![
            ("mix", 1.0),
            ("predelay", 0.0),
            ("topology", topo),
            ("decay", 3_000.0),
        ];
        // The chamber's envelope closes after DECAY — measure
        // INSIDE its burst window.
        let from_s = if name == "DETONATE" { 0.65 } else { 1.0 };
        if name == "DETONATE" {
            set.retain(|(n, _)| *n != "decay");
            set.push(("decay", 1_400.0));
        }
        let out = render(&set, &burst(), 700);
        let mag = late_spectrum(&out, from_s, 800, 400.0, 6_000.0);
        let bin_hz = (6_000.0 - 400.0) / 799.0;
        let f = flatness(&mag);
        let c = comb_regularity(&mag, bin_hz);
        println!("  {name:<9} flatness {f:.3}  comb {c:.3}");
        if f < 0.10 || c > 0.60 {
            fails.push(format!("{name}: flatness {f:.3} comb {c:.3}"));
        }
    }
    assert!(fails.is_empty(), "machines ring metallic: {fails:?}");
}
