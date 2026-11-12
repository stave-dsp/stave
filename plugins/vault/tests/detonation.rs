//! THE DETONATION SUITE — the event chamber must behave like an
//! event system, measurably:
//! * each hit DETONATES (a dense burst follows every onset);
//! * between hits THE SILENCE STAYS BLACK (post-window floor);
//! * DECAY is the burst's LENGTH (duration tracks the dial);
//! * THE FUSE shapes the burst (energy centroid moves from front
//!   to back across the dial);
//! * WIDTH 0 is mono; the wet holds its own on hits.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render_stereo(set: &[(&str, f64)], input: &[f32], extra_blocks: usize) -> (Vec<f32>, Vec<f32>) {
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
    let (mut l, mut r) = (Vec::new(), Vec::new());
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
        r.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (l, r)
}

fn det_set() -> Vec<(&'static str, f64)> {
    vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("topology", 3.0),
        ("decay", 400.0),
    ]
}

/// Two snare-ish hits, 1 s apart.
fn hits() -> Vec<f32> {
    let mut rng = 0xbea7u32;
    (0..(2.2 * FS) as usize)
        .map(|i| {
            let in_hit = (i % 48_000) < 1_920 && i < 96_000;
            if in_hit {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5
            } else {
                0.0
            }
        })
        .collect()
}

fn energy(w: &[f32], a: usize, b: usize) -> f64 {
    w[a..b.min(w.len())]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum()
}

/// Each hit detonates, and between bursts the silence is BLACK.
#[test]
fn hits_detonate_and_silence_stays_black() {
    let (l, _) = render_stereo(&det_set(), &hits(), 300);
    // Burst windows: hit at 0 and at 1.0 s, burst len 400 ms.
    let b1 = energy(&l, (0.05 * FS) as usize, (0.35 * FS) as usize);
    let b2 = energy(&l, (1.05 * FS) as usize, (1.35 * FS) as usize);
    // Black windows: well after each burst closes.
    let k1 = energy(&l, (0.65 * FS) as usize, (0.95 * FS) as usize);
    let k2 = energy(&l, (1.75 * FS) as usize, (2.05 * FS) as usize);
    let black_db = 10.0 * ((k1 + k2) / (b1 + b2).max(1e-30)).log10();
    println!("  DET bursts {b1:.2e}/{b2:.2e}  black {black_db:.1} dB");
    assert!(
        b1 > 1e-4 && b2 > 1e-4,
        "every hit must detonate: {b1:.2e} {b2:.2e}"
    );
    assert!(
        black_db < -50.0,
        "the silence must stay black: {black_db:.1} dB"
    );
}

/// DECAY is the burst's LENGTH.
#[test]
fn decay_sets_the_burst_length() {
    let dur = |decay: f64| {
        let mut set = det_set();
        set.retain(|(n, _)| *n != "decay");
        set.push(("decay", decay));
        let mut one = vec![0.0f32; (0.3 * FS) as usize];
        let mut rng = 0x1234u32;
        for x in one.iter_mut().take(1_920) {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *x = ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5;
        }
        let (l, _) = render_stereo(&set, &one, 400);
        // Last window where energy stays above -40 dB of the peak.
        let win = (0.02 * FS) as usize;
        let peak: f64 = (0..12)
            .map(|k| energy(&l, k * win, (k + 1) * win))
            .fold(0.0, f64::max);
        let mut last = 0usize;
        for k in 0..80 {
            if energy(&l, k * win, (k + 1) * win) > peak * 1e-4 {
                last = k;
            }
        }
        last as f64 * 0.02
    };
    let short = dur(150.0);
    let long = dur(900.0);
    println!("  DET burst duration: 150ms dial {short:.2}s  900ms dial {long:.2}s");
    assert!(
        long > 2.5 * short,
        "the dial must set the burst: {short:.2}s -> {long:.2}s"
    );
}

/// THE FUSE moves the burst's weight from front to back.
#[test]
fn the_fuse_shapes_the_burst() {
    let centroid = |fuse: f64| {
        let mut set = det_set();
        set.push(("fuse", fuse));
        let mut one = vec![0.0f32; (0.3 * FS) as usize];
        let mut rng = 0x9997u32;
        for x in one.iter_mut().take(1_920) {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *x = ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5;
        }
        let (l, _) = render_stereo(&set, &one, 300);
        let a = (0.03 * FS) as usize;
        let b = (0.43 * FS) as usize;
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for i in a..b {
            let e = f64::from(l[i]) * f64::from(l[i]);
            num += (i - a) as f64 * e;
            den += e;
        }
        num / den.max(1e-30) / f64::from(FS)
    };
    let front = centroid(0.0);
    let back = centroid(1.0);
    println!("  FUSE centroid: 0 -> {front:.3}s  1 -> {back:.3}s");
    assert!(
        back > front + 0.08,
        "the fuse must move the burst's weight: {front:.3}s -> {back:.3}s"
    );
}

/// WIDTH 0 is mono; the wet holds its own on hits.
#[test]
fn the_chamber_field_behaves() {
    let mut mono_set = det_set();
    mono_set.push(("width", 0.0));
    let (l, r) = render_stereo(&mono_set, &hits(), 100);
    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    for i in (0.05 * FS) as usize..(0.4 * FS) as usize {
        xy += f64::from(l[i]) * f64::from(r[i]);
        xx += f64::from(l[i]) * f64::from(l[i]);
        yy += f64::from(r[i]) * f64::from(r[i]);
    }
    let corr = xy / (xx * yy).sqrt().max(1e-30);
    assert!(corr > 0.95, "width 0 must be mono: corr {corr:.3}");

    let input = hits();
    let (lw, rw) = render_stereo(&det_set(), &input, 100);
    let wet = (energy(&lw, 0, (0.4 * FS) as usize) + energy(&rw, 0, (0.4 * FS) as usize)) / 2.0;
    let dry = energy(&input, 0, (0.4 * FS) as usize);
    let db = 10.0 * (wet / dry.max(1e-30)).log10();
    println!("  DET wet vs dry (burst window): {db:+.1} dB");
    assert!(db.abs() < 5.0, "the burst must hold its own: {db:+.1} dB");
}
