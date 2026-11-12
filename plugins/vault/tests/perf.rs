//! The perf report: representative configurations measured against
//! the realtime budget (stave-test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<stave_vault_vst3::Vault>(
        "vault",
        &[PerfCase {
            name: "DEFAULT",
            set: &[],
        }],
        &PerfConfig::default(),
    );
}

/// THE PERF RATCHET — a hard ceiling, generous enough to ignore
/// machine noise (median ~830 ns/frame across sessions; a single
/// 489 ns reading after the tape purge was a MACHINE-STATE FLUKE
/// that briefly mis-lowered these ceilings — never ratchet on one
/// run), tight enough to
/// catch the regression class the field caught by hand (per-tap
/// cubic scattering, 1263 ns/frame). Lower it when the median
/// legitimately drops.
#[test]
fn the_machine_stays_inside_its_budget() {
    budget_for(&[], 1_000.0);
}

/// THE PLATE BUDGET — the topology arc's CPU thesis, pinned: a
/// plate runs NO geometry stages (no image source, no midfield,
/// no modes), so it must come in far under the room.
#[test]
fn the_plate_stays_inside_its_budget() {
    budget_for(&[("topology", 1.0)], 600.0);
}

/// THE MEMORY BUDGET: 126 resonators and nothing else.
#[test]
fn the_memory_stays_inside_its_budget() {
    budget_for(&[("topology", 2.0)], 700.0);
}

/// THE CHAMBER BUDGET.
#[test]
fn the_chamber_stays_inside_its_budget() {
    budget_for(&[("topology", 3.0)], 600.0);
}

/// THE VOID BUDGET.
#[test]
fn the_void_stays_inside_its_budget() {
    budget_for(&[("topology", 4.0)], 700.0);
}

fn budget_for(set: &[(&str, f64)], release_ceiling: f64) {
    use stave_core::{AudioBuffer, Plugin, ProcessCtx};
    const FS: f32 = 48_000.0;
    const BLOCK: usize = 256;
    let mut plugin = stave_vault_vst3::Vault::new();
    {
        let table = plugin.param_table();
        for (n, v) in set {
            table
                .by_name(n)
                .unwrap_or_else(|| panic!("param '{n}'"))
                .set_plain(*v);
        }
    }
    plugin.activate(FS, BLOCK);
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    let mut lcg: u32 = 7;
    let mut run = |blocks: usize| -> f64 {
        let t0 = std::time::Instant::now();
        for _ in 0..blocks {
            for x in in_l.iter_mut() {
                lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
                *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3;
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
        }
        t0.elapsed().as_secs_f64() / (blocks * BLOCK) as f64 * 1e9
    };
    run(100); // warm
    let mut samples: Vec<f64> = (0..5).map(|_| run(200)).collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[2];
    println!(
        "  MEDIAN {median:.0} ns/frame ({:.1}% of the 48k budget)",
        median / 20_833.0 * 100.0
    );
    // Debug builds run ~10-20x slower — the ceiling scales.
    let ceiling = if cfg!(debug_assertions) {
        22_000.0
    } else {
        release_ceiling
    };
    assert!(
        median < ceiling,
        "perf regression: {median:.0} ns/frame (ceiling {ceiling:.0})"
    );
}
