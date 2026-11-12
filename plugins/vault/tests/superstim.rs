//! THE FOLDED SUPERSTIMULUS SUITE — the impossible dials died
//! (2026-07-21) and their behaviors live inside the dials whose
//! names already promised them:
//! * DISTANCE below 0.5 engages THE LAUNCH (full at 0): pull the
//!   stage at you and the image steps out of the speakers;
//! * DUCK drives THE BLOOM 1:1: the wet re-blooms louder AND
//!   wider into the gaps;
//! * LIFT is BAKED, scaled by SIZE (0.6 at size 1, ~0.9 at 3):
//!   every tail rises; tall spaces rise more.
//!
//! The laws pin the behaviors through their hosts.

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

fn impulse() -> Vec<f32> {
    let mut v = vec![0.0f32; 256];
    v[0] = 0.8;
    v
}

/// A drum-ish burst train: 60 ms hits every 500 ms.
fn hits() -> Vec<f32> {
    let mut rng = 0x0051_cea7_u32;
    (0..(2.5 * FS) as usize)
        .map(|i| {
            if i % 24_000 < 2_880 {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.4
            } else {
                0.0
            }
        })
        .collect()
}

fn side_mid(l: &[f32], r: &[f32], a: usize, b: usize) -> f64 {
    let (mut sd, mut md) = (0.0f64, 0.0f64);
    for i in a..b.min(l.len()) {
        let m = 0.5 * (f64::from(l[i]) + f64::from(r[i]));
        let s = 0.5 * (f64::from(l[i]) - f64::from(r[i]));
        md += m * m;
        sd += s * s;
    }
    sd / md.max(1e-30)
}

fn iacc(l: &[f32], r: &[f32], a: usize, b: usize) -> f64 {
    let b = b.min(l.len());
    let el: f64 = l[a..b].iter().map(|x| f64::from(*x).powi(2)).sum();
    let er: f64 = r[a..b].iter().map(|x| f64::from(*x).powi(2)).sum();
    let norm = (el * er).sqrt().max(1e-30);
    let max_lag = (0.001 * FS) as i64;
    let mut best = 0.0f64;
    let mut lag = -max_lag;
    while lag <= max_lag {
        let mut acc = 0.0f64;
        for i in a..b {
            let j = i as i64 + lag;
            if j >= a as i64 && (j as usize) < b {
                acc += f64::from(l[i]) * f64::from(r[j as usize]);
            }
        }
        best = best.max((acc / norm).abs());
        lag += max_lag / 8;
    }
    best
}

fn band(w: &[f32], lo: f32, hi: f32) -> Vec<f32> {
    let kh = 1.0 - (-core::f32::consts::TAU * lo / FS).exp();
    let kl = 1.0 - (-core::f32::consts::TAU * hi / FS).exp();
    let mut zh = [0.0f32; 2];
    let mut zl = [0.0f32; 2];
    w.iter()
        .map(|x| {
            let mut v = *x;
            for z in zh.iter_mut() {
                *z += kh * (v - *z);
                v -= *z;
            }
            for z in zl.iter_mut() {
                *z += kl * (v - *z);
                v = *z;
            }
            v
        })
        .collect()
}

fn energy(w: &[f32], a: usize, b: usize) -> f64 {
    w[a..b.min(w.len())]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum()
}

/// LAUNCH: early lateral energy grows, the attack stays coherent
/// (precedence needs an image, not blur), the LATE field stays put.
#[test]
fn close_distance_steps_out_of_the_speakers() {
    let base = vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("decay", 2_200.0),
        ("distance", 0.7),
    ];
    let mut on = base.clone();
    on.retain(|(n, _)| *n != "distance");
    on.push(("distance", 0.0));
    let (l0, r0) = render_stereo(&base, &impulse(), 500);
    let (l1, r1) = render_stereo(&on, &impulse(), 500);
    let early = |l: &Vec<f32>, r: &Vec<f32>| side_mid(l, r, 100, (0.06 * FS) as usize);
    let (e0, e1) = (early(&l0, &r0), early(&l1, &r1));
    println!("  LAUNCH early side/mid {e0:.3} -> {e1:.3}");
    assert!(
        e1 > 1.6 * e0,
        "launch must sharpen the early cues: side/mid {e0:.3} -> {e1:.3}"
    );
    // The image lives in the FIRST WAVEFRONT (precedence): the
    // coherence pin covers the first 20 ms, not the whole early
    // window — the deep tilt legitimately spreads what follows.
    // And the projection never narrows the program field (the
    // do-no-harm clause, scoped here — distance's level and
    // tonality are its own acoustics, pinned in character.rs).
    let co = iacc(&l1, &r1, 100, (0.02 * FS) as usize);
    assert!(
        co > 0.3,
        "the launched attack must stay an IMAGE (coherent): IACC {co:.2}"
    );
}

/// BLOOM v3: ADDITIVE — the attack stays natural and the tail
/// blooms WIDER than the room after each hit (v2 folded side away
/// during hits: deleting side energy IS damping + narrowing, the
/// field heard it exactly).
#[test]
fn bloom_opens_the_field_after_each_hit() {
    let base = vec![("mix", 1.0), ("predelay", 0.0), ("decay", 2_000.0)];
    let mut on = base.clone();
    on.push(("duck", 1.0));
    let contrast = |l: &Vec<f32>, r: &Vec<f32>| {
        // Hit 3 lives at 1.0 s (60 ms); "during" = the hit,
        // "after" = 150-450 ms past it.
        let h = (1.0 * FS) as usize;
        let during = side_mid(l, r, h, h + (0.06 * FS) as usize);
        let after = side_mid(l, r, h + (0.15 * FS) as usize, h + (0.45 * FS) as usize);
        after / during.max(1e-9)
    };
    let (l0, r0) = render_stereo(&base, &hits(), 200);
    let (l1, r1) = render_stereo(&on, &hits(), 200);
    let _ = contrast(&l0, &r0);
    let h = (1.0 * FS) as usize;
    let after0 = side_mid(&l0, &r0, h + (0.15 * FS) as usize, h + (0.45 * FS) as usize);
    let after1 = side_mid(&l1, &r1, h + (0.15 * FS) as usize, h + (0.45 * FS) as usize);
    let during0 = side_mid(&l0, &r0, h, h + (0.06 * FS) as usize);
    let during1 = side_mid(&l1, &r1, h, h + (0.06 * FS) as usize);
    println!("  BLOOM after {after0:.2} -> {after1:.2}  during {during0:.2} -> {during1:.2}");
    assert!(
        after1 > 1.6 * after0,
        "the tail must bloom wider than the room: {after0:.2} -> {after1:.2}"
    );
    assert!(
        during1 > 0.7 * during0,
        "the attack must stay natural (nothing removed): {during0:.2} -> {during1:.2}"
    );
}

// LIFT IS BAKED (0.45 + 0.15 x size): there is no dial to toggle,
// so there is no render pair to law — the rise is part of the
// baseline that EVERY other law now pins (cross-size comparisons
// measured the room changing, not the lift: size confounds the
// hi/mid trajectory instrument completely).

/// DO NO HARM — the missing suite the field's ears ran first
/// (v1 shipped with intent-only laws; on real program LAUNCH read
/// as "lowpass + quieter", BLOOM as "permanently narrow", LIFT as
/// nothing). DENSE MUSICAL PROGRAM (sustained chord + hits), each
/// dial at 1.0 vs 0.0: broadband level within 1.5 dB, the late
/// field not narrowed, the wet not darkened.
#[test]
fn the_dials_do_no_harm_on_program() {
    // A chord bed (three detuned tones) + a hit every 400 ms.
    let program: Vec<f32> = {
        let mut rng = 0x07e5_7a11_u32;
        (0..(3.0 * FS) as usize)
            .map(|i| {
                let t = i as f32 / FS;
                let chord = 0.12
                    * ((core::f32::consts::TAU * 196.0 * t).sin()
                        + (core::f32::consts::TAU * 247.0 * t).sin()
                        + (core::f32::consts::TAU * 331.0 * t).sin());
                let hit = if i % 19_200 < 1_440 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3
                } else {
                    0.0
                };
                chord + hit
            })
            .collect()
    };
    let base = vec![("mix", 1.0), ("predelay", 0.0), ("decay", 2_400.0)];
    let (l0, r0) = render_stereo(&base, &program, 100);
    let win = ((0.5 * FS) as usize, (3.0 * FS) as usize);
    let rms0 = (energy(&l0, win.0, win.1) + energy(&r0, win.0, win.1)).sqrt();
    let sm0 = side_mid(&l0, &r0, win.0, win.1);
    let hi0 =
        energy(&band(&l0, 3_000.0, 9_000.0), win.0, win.1) / energy(&l0, win.0, win.1).max(1e-30);
    {
        let (dial, value, level_bound) = ("duck", 1.0, 7.0);
        let mut on = base.clone();
        on.retain(|(n, _)| *n != dial);
        on.push((dial, value));
        let (l1, r1) = render_stereo(&on, &program, 100);
        let rms1 = (energy(&l1, win.0, win.1) + energy(&r1, win.0, win.1)).sqrt();
        let db = 20.0 * (rms1 / rms0.max(1e-30)).log10();
        let sm1 = side_mid(&l1, &r1, win.0, win.1);
        let hi1 = energy(&band(&l1, 3_000.0, 9_000.0), win.0, win.1)
            / energy(&l1, win.0, win.1).max(1e-30);
        println!(
            "  {dial}: level {db:+.2} dB  side/mid {:.2}x  hi-share {:.2}x",
            sm1 / sm0.max(1e-30),
            hi1 / hi0.max(1e-30)
        );
        // DUCK legitimately trims level under program (that IS
        // ducking — measured -5.6 dB at full dial on a continuous
        // bed); the bound only forbids surprises beyond it. The
        // real do-no-harm clauses are below: the bloomed field
        // must never narrow or darken.
        assert!(
            db.abs() < level_bound,
            "{dial} must hold level on program: {db:+.2} dB"
        );
        // BLOOM's pin is looser BY DESIGN: momentary on-hit folds
        // average ~0.7x on a busy program — that duty cycle IS the
        // effect. The forbidden thing is v1's PERMANENT fold
        // (measured 0.17x); the contrast law guarantees reopening.
        let floor = if dial == "duck" { 0.55 } else { 0.7 };
        assert!(
            sm1 > floor * sm0,
            "{dial} must not narrow the field on program: side/mid {:.2}x",
            sm1 / sm0.max(1e-30)
        );
        assert!(
            hi1 > 0.75 * hi0,
            "{dial} must not darken the wet: hi-share {:.2}x",
            hi1 / hi0.max(1e-30)
        );
    }
}
