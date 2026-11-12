//! ROBUSTNESS (worklist 7) — the machine must sound the same at
//! any host block size, and dials must be automatable without
//! clicks. Two instruments:
//! * BLOCK-SIZE INVARIANCE: block 64 vs 1024 renders compared at
//!   the metric level (block-rate modulation sampling makes the
//!   waveforms legitimately differ in the small; the SOUND must
//!   not);
//! * AUTOMATION GLITCH DETECTION: step a dial mid-render under
//!   sustained program and hunt for a discontinuity spike in the
//!   high-passed output around the switch instant.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;

/// Render with a given block size; `automate` fires once at
/// `at_block` (param name, new plain value).
fn render_blocks(
    set: &[(&str, f64)],
    input: &[f32],
    block: usize,
    extra_blocks: usize,
    automate: Option<(usize, &str, f64)>,
) -> Vec<f32> {
    let mut plugin = stave_vault_vst3::Vault::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    let set_param = |n: &str, v: f64| {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(v);
    };
    for (n, v) in set {
        set_param(n, *v);
    }
    plugin.activate(FS, block);
    let blocks = input.len().div_ceil(block) + extra_blocks;
    let mut out = Vec::new();
    let mut in_l = vec![0.0f32; block];
    let mut in_r = vec![0.0f32; block];
    let mut out_l = vec![0.0f32; block];
    let mut out_r = vec![0.0f32; block];
    for b in 0..blocks {
        if let Some((at, name, v)) = automate {
            if b == at {
                set_param(name, v);
            }
        }
        for i in 0..block {
            in_l[i] = input.get(b * block + i).copied().unwrap_or(0.0);
        }
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], block);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], block);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: block,
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

fn rms(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}

/// THE SOUND IS BLOCK-SIZE INVARIANT: 64 vs 1024 must agree at
/// the metric level — tail energy trajectory and peak.
#[test]
fn the_sound_survives_any_block_size() {
    let set = [("mix", 1.0), ("predelay", 0.0), ("decay", 2_000.0)];
    let mut impulse = vec![0.0f32; 1024];
    impulse[0] = 0.8;
    let a = render_blocks(&set, &impulse, 64, 1_500, None);
    let b = render_blocks(&set, &impulse, 1024, 94, None);
    let n = a.len().min(b.len());
    // Windowed energy trajectory: the tails must decay identically.
    let win = (0.1 * FS) as usize;
    let mut k = 0;
    while (k + 1) * win < n {
        let ra = rms(&a[k * win..(k + 1) * win]);
        let rb = rms(&b[k * win..(k + 1) * win]);
        if ra > 1e-6 || rb > 1e-6 {
            let db = 20.0 * (ra.max(1e-12) / rb.max(1e-12)).log10();
            assert!(
                db.abs() < 1.0,
                "window {k}: block-64 vs block-1024 differ by {db:.2} dB"
            );
        }
        k += 1;
    }
    let pa = a[..n].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let pb = b[..n].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(
        (f64::from(pa) / f64::from(pb) - 1.0).abs() < 0.25,
        "peaks differ: {pa:.3} vs {pb:.3}"
    );
}

/// AUTOMATION IS GLITCHLESS: step a dial mid-render under a
/// sustained 220 Hz TONE (the sensitive bed: HP-2k is empty
/// except zipper/click energy — noise masks its own clicks,
/// measured) and hunt the switch instant for a spike that
/// exceeds BOTH regimes' steady HP levels (a legitimate timbre
/// change raises the new regime; a click towers over both).
#[test]
fn dials_step_without_clicks() {
    let block = 256usize;
    let n = (2.5 * FS) as usize;
    let bed: Vec<f32> = (0..n)
        .map(|i| 0.4 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
        .collect();
    let at = (1.2 * FS) as usize / block;
    let mut fails: Vec<String> = Vec::new();
    for (name, from, to) in [
        ("high energy", 0.1, 0.9),
        ("tone", -0.8, 0.8),
        ("material", 1.0, 5.0),
        ("low energy", 0.4, 3.0),
        ("decay", 800.0, 6_000.0),
        ("size", 0.8, 2.4),
        ("width", 0.2, 1.8),
    ] {
        let set = [("mix", 1.0), ("predelay", 0.0), (name, from)];
        let out = render_blocks(&set, &bed, block, 40, Some((at, name, to)));
        let kh = 1.0 - (-core::f32::consts::TAU * 2_000.0 / FS).exp();
        let mut z = [0.0f32; 3];
        let hp: Vec<f32> = out
            .iter()
            .map(|x| {
                let mut v = *x;
                for zz in z.iter_mut() {
                    *zz += kh * (v - *zz);
                    v -= *zz;
                }
                v
            })
            .collect();
        let sw = at * block;
        let peak = |w: &[f32]| w.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let before = peak(&hp[sw - (0.4 * FS) as usize..sw - (0.02 * FS) as usize]);
        let after = peak(&hp[sw + (0.25 * FS) as usize..sw + (0.9 * FS) as usize]);
        let spike = peak(&hp[sw..sw + (0.06 * FS) as usize]);

        let floor = before.max(after);
        println!("  {name:<9} {from:>6}->{to:<6} spike {spike:.5} regimes {before:.5}/{after:.5} ratio {:.2}", spike / floor.max(1e-9));
        if spike >= 1.5 * floor {
            fails.push(format!(
                "{name} {from}->{to}: spike {spike:.5} vs {before:.5}/{after:.5}"
            ));
        }
    }
    assert!(fails.is_empty(), "dials click: {fails:?}");
}
