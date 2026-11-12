//! THE VOID SUITE — the wall-less field, measurably:
//! * NO early structure: the onset is a smooth wash (low crest),
//!   with no discrete-arrival spikes;
//! * DAWN swells the send (onset energy shifts later with the
//!   dial; bit-absent at 0);
//! * DECAY honest at long holds; WIDTH 0 mono; level holds.

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

fn void_set() -> Vec<(&'static str, f64)> {
    vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("topology", 4.0),
        ("decay", 8_000.0),
        ("dawn", 0.0),
    ]
}

fn impulse() -> Vec<f32> {
    let mut v = vec![0.0f32; 512];
    v[0] = 0.8;
    v
}

fn rms(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}

fn energy(w: &[f32], a: usize, b: usize) -> f64 {
    w[a..b.min(w.len())]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum()
}

/// NO WALLS: the early wet is a wash, not arrivals — its crest
/// stays low from the first window.
#[test]
fn there_are_no_walls() {
    let (l, _) = render_stereo(&void_set(), &impulse(), 500);
    let a = (0.05 * FS) as usize;
    let b = (0.25 * FS) as usize;
    let peak = l[a..b].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let crest = f64::from(peak) / rms(&l[a..b]).max(1e-15);
    println!("  VOID early crest {crest:.2}");
    assert!(crest < 6.0, "the void must be a wash: crest {crest:.2}");
}

/// DAWN swells the send: with the dial up, a burst's wet onset
/// shifts LATER (energy-time centroid of the first 300 ms); at 0
/// the swell is bit-absent.
#[test]
fn dawn_makes_sources_rise() {
    let burst: Vec<f32> = {
        let mut rng = 0xda31u32;
        (0..(0.4 * FS) as usize)
            .map(|i| {
                if i < (0.25 * FS) as usize {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.4
                } else {
                    0.0
                }
            })
            .collect()
    };
    // Early/late energy WITHIN the burst window — a centroid gets
    // diluted by the field's own tail (measured +30 ms and stuck).
    let front_back = |dawn: f64| {
        let mut set = void_set();
        set.retain(|(n, _)| *n != "dawn");
        set.push(("dawn", dawn));
        let (l, _) = render_stereo(&set, &burst, 300);
        let front = energy(&l, 0, (0.08 * FS) as usize);
        let back = energy(&l, (0.15 * FS) as usize, (0.25 * FS) as usize);
        front / back.max(1e-30)
    };
    let flat = front_back(0.0);
    let risen = front_back(1.0);
    println!("  DAWN front/back: 0 -> {flat:.3}  1 -> {risen:.3}");
    assert!(
        flat > 2.5 * risen,
        "dawn must make the source rise: front/back {flat:.3} -> {risen:.3}"
    );

    // Bit-absent at 0 vs an explicit zero.
    let (l0, _) = render_stereo(&void_set(), &impulse(), 200);
    let mut zset = void_set();
    zset.retain(|(n, _)| *n != "dawn");
    zset.push(("dawn", 0.0));
    let (l1, _) = render_stereo(&zset, &impulse(), 200);
    for i in 0..l0.len() {
        assert!(
            l0[i].to_bits() == l1[i].to_bits(),
            "dawn 0 must be absent: {i}"
        );
    }
}

/// DECAY honest at a long hold.
#[test]
fn the_field_holds_as_dialed() {
    let mut set = void_set();
    set.retain(|(n, _)| *n != "decay");
    set.push(("decay", 6_000.0));
    set.push(("high energy", 0.0));
    let (l, _) = render_stereo(&set, &impulse(), 1_400);
    let e1 = energy(&l, (1.0 * FS) as usize, (1.5 * FS) as usize);
    let e2 = energy(&l, (4.0 * FS) as usize, (4.5 * FS) as usize);
    let db_per_s = 10.0 * (e1 / e2.max(1e-30)).log10() / 3.0;
    let t60 = 60.0 / db_per_s;
    println!("  VOID T60 {t60:.2}s (dialed 6.0)");
    assert!(
        (t60 / 6.0 - 1.0).abs() < 0.35,
        "the field must hold as dialed: {t60:.2}s vs 6.0s"
    );
}

/// WIDTH 0 mono; level holds on program.
#[test]
fn the_void_field_behaves() {
    let mut mono_set = void_set();
    mono_set.push(("width", 0.0));
    let (l, r) = render_stereo(&mono_set, &impulse(), 400);
    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    for i in (0.1 * FS) as usize..l.len() {
        xy += f64::from(l[i]) * f64::from(r[i]);
        xx += f64::from(l[i]) * f64::from(l[i]);
        yy += f64::from(r[i]) * f64::from(r[i]);
    }
    let corr = xy / (xx * yy).sqrt().max(1e-30);
    assert!(corr > 0.95, "width 0 must be mono: corr {corr:.3}");

    let noise: Vec<f32> = {
        let mut rng = 0x005e_ed11_u32;
        (0..(2.0 * FS) as usize)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.25
            })
            .collect()
    };
    let (lw, rw) = render_stereo(&void_set(), &noise, 100);
    let wet = (energy(&lw, (0.5 * FS) as usize, (2.0 * FS) as usize)
        + energy(&rw, (0.5 * FS) as usize, (2.0 * FS) as usize))
        / 2.0;
    let dry = energy(&noise, (0.5 * FS) as usize, (2.0 * FS) as usize);
    let db = 10.0 * (wet / dry.max(1e-30)).log10();
    println!("  VOID wet vs dry: {db:+.1} dB");
    assert!(db.abs() < 4.5, "the void must hold its own: {db:+.1} dB");
}
