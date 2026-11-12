//! THE MEMORY SUITE — the remembering field must actually
//! remember, measurably:
//! * a held chord keeps ringing AT ITS PITCHES after the source
//!   stops (Goertzel at the played notes vs off-grid probes);
//! * a transient charges almost nothing (driven-gain-flat physics
//!   — the no-smear claim IS this asymmetry);
//! * DECAY is the hold's honest T60;
//! * ASCEND climbs: a held note's energy appears an octave up,
//!   growing through the tail;
//! * WIDTH 0 is mono by geometry; the wet holds its own.

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

fn memory_set() -> Vec<(&'static str, f64)> {
    vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("topology", 2.0),
        ("decay", 4_000.0),
    ]
}

/// A held three-note chord for `secs`, then silence.
fn chord(secs: f32) -> Vec<f32> {
    (0..(secs * FS) as usize)
        .map(|i| {
            let t = i as f32 / FS;
            let env = (t * 8.0).min(1.0);
            0.12 * env
                * ((core::f32::consts::TAU * 220.0 * t).sin()
                    + (core::f32::consts::TAU * 277.2 * t).sin()
                    + (core::f32::consts::TAU * 329.6 * t).sin())
        })
        .collect()
}

fn goertzel(w: &[f32], hz: f64) -> f64 {
    let n = w.len();
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &x) in w.iter().enumerate() {
        let hann = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
        let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
        re += f64::from(x) * hann * ph.cos();
        im += f64::from(x) * hann * ph.sin();
    }
    (re * re + im * im).sqrt()
}

fn energy(w: &[f32], a: usize, b: usize) -> f64 {
    w[a..b.min(w.len())]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum()
}

/// The memory HOLDS THE CHORD: after the source stops, the tail
/// sings at the played pitches, far above off-grid probes.
#[test]
fn the_memory_holds_the_chord() {
    let input = chord(1.5);
    let (l, _) = render_stereo(&memory_set(), &input, 500);
    // 0.5-1.2 s after the chord ends.
    let a = ((1.5 + 0.5) * FS) as usize;
    let b = ((1.5 + 1.2) * FS) as usize;
    let w = &l[a..b];
    // BANDED probes (±3%): the cloud smears each held partial
    // into a breathing band — exact-frequency probes read the
    // line's absence, not the chord's (measured 1.3x and falling
    // as the cloud deepened).
    let banded = |f: f64| -> f64 {
        [0.97, 1.0, 1.03]
            .iter()
            .map(|m| goertzel(w, f * m))
            .sum::<f64>()
    };
    let sung: f64 = [220.0, 277.2, 329.6].iter().map(|f| banded(*f)).sum();
    let off: f64 = [190.0, 250.0, 305.0].iter().map(|f| banded(*f)).sum();
    println!(
        "  MEMORY chord ring: sung {sung:.3e} vs off-grid {off:.3e} ({:.1}x)",
        sung / off.max(1e-30)
    );
    assert!(
        sung > 4.0 * off,
        "the memory must hold the chord: {sung:.2e} vs {off:.2e}"
    );
}

/// The memory IGNORES THE TRANSIENT: an impulse leaves almost
/// nothing behind, while a sustained tone of comparable peak
/// leaves a long ring — the no-smear physics, as a ratio.
#[test]
fn the_memory_ignores_the_transient() {
    let mut click = vec![0.0f32; (0.5 * FS) as usize];
    click[1000] = 0.5;
    let tone: Vec<f32> = (0..(0.5 * FS) as usize)
        .map(|i| 0.5 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
        .collect();
    let (lc, _) = render_stereo(&memory_set(), &click, 400);
    let (lt, _) = render_stereo(&memory_set(), &tone, 400);
    let a = (1.0 * FS) as usize;
    let b = (1.8 * FS) as usize;
    let (ec, et) = (energy(&lc, a, b), energy(&lt, a, b));
    println!("  MEMORY transient/tone tails: click {ec:.3e} vs tone {et:.3e}");
    assert!(
        et > 30.0 * ec,
        "the memory must prefer tone to transient: {ec:.2e} vs {et:.2e}"
    );
}

/// DECAY is the hold's honest T60.
#[test]
fn the_hold_decays_as_dialed() {
    let mut set = memory_set();
    set.retain(|(n, _)| *n != "decay");
    set.push(("decay", 2_000.0));
    set.push(("high energy", 0.0));
    let input = chord(1.0);
    let (l, _) = render_stereo(&set, &input, 700);
    let e1 = energy(&l, ((1.0 + 0.3) * FS) as usize, ((1.0 + 0.6) * FS) as usize);
    let e2 = energy(&l, ((1.0 + 1.3) * FS) as usize, ((1.0 + 1.6) * FS) as usize);
    let db_per_s = 10.0 * (e1 / e2.max(1e-30)).log10();
    let t60 = 60.0 / db_per_s;
    println!("  MEMORY T60 {t60:.2}s (dialed 2.0)");
    assert!(
        (t60 / 2.0 - 1.0).abs() < 0.35,
        "the hold must keep the dialed decay: {t60:.2}s vs 2.0s"
    );
}

/// ASCEND CLIMBS: hold a note; with the ladder open, the octave
/// above GROWS through the tail (and it does not at ascend 0).
#[test]
fn ascend_climbs_the_octaves() {
    let ratio_at = |ascend: f64| {
        let mut set = memory_set();
        set.push(("ascend", ascend));
        let tone: Vec<f32> = (0..(2.0 * FS) as usize)
            .map(|i| 0.4 * (core::f32::consts::TAU * 220.0 * i as f32 / FS).sin())
            .collect();
        let (l, _) = render_stereo(&set, &tone, 500);
        // Late in the HOLD (still sounding): octave/fundamental.
        let a = (1.6 * FS) as usize;
        let b = (2.0 * FS) as usize;
        let w = &l[a..b];
        goertzel(w, 440.0) / goertzel(w, 220.0).max(1e-30)
    };
    let closed = ratio_at(0.0);
    let open = ratio_at(1.0);
    println!("  ASCEND octave/fundamental: closed {closed:.3}  open {open:.3}");
    assert!(
        closed < 0.05,
        "the closed ladder must stay closed: {closed:.3}"
    );
    assert!(open > 0.25, "the ladder must climb audibly: {open:.3}");
}

/// WIDTH 0 is mono by geometry; the wet holds its own on program.
#[test]
fn the_memory_field_behaves() {
    let mut mono_set = memory_set();
    mono_set.push(("width", 0.0));
    let input = chord(1.0);
    let (l, r) = render_stereo(&mono_set, &input, 300);
    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    let a = (0.5 * FS) as usize;
    for i in a..l.len() {
        xy += f64::from(l[i]) * f64::from(r[i]);
        xx += f64::from(l[i]) * f64::from(l[i]);
        yy += f64::from(r[i]) * f64::from(r[i]);
    }
    let corr = xy / (xx * yy).sqrt().max(1e-30);
    assert!(corr > 0.95, "width 0 must be mono: corr {corr:.3}");

    // Level: sustained program through the memory sits near the dry.
    let bed: Vec<f32> = (0..(2.0 * FS) as usize)
        .map(|i| {
            let t = i as f32 / FS;
            0.2 * ((core::f32::consts::TAU * 196.0 * t).sin()
                + (core::f32::consts::TAU * 294.0 * t).sin())
        })
        .collect();
    let (lw, rw) = render_stereo(&memory_set(), &bed, 100);
    let wet = (energy(&lw, (0.5 * FS) as usize, (2.0 * FS) as usize)
        + energy(&rw, (0.5 * FS) as usize, (2.0 * FS) as usize))
        / 2.0;
    let dry = energy(&bed, (0.5 * FS) as usize, (2.0 * FS) as usize);
    let db = 10.0 * (wet / dry.max(1e-30)).log10();
    println!("  MEMORY wet vs dry: {db:+.1} dB");
    assert!(db.abs() < 4.5, "the memory must hold its own: {db:+.1} dB");
}
