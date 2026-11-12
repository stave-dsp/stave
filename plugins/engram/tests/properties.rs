//! Engram's OWN properties — a reverb with an associative memory,
//! measured through the whole plugin:
//! * MIX 0 is bit-exact dry; silence in, silence out (a fresh memory
//!   holds nothing to recall).
//! * It is a REVERB: an impulse leaves a decaying tail, DECAY sets its
//!   T60; HOLD freezes it; the drawn shape changes the room; every
//!   drawing is stable.
//! * It REMEMBERS BY HARMONY: play A, then B, then A again — what the
//!   memory recalls during the second A is the first A, not B.
//! * …and DRUMS BY THEIR SOUND: kicks, then hats, then kicks — it
//!   recalls the kicks; and percussive memories ANSWER your hits.
//! * MATCH decides what "alike" means: with harmony and timbre at odds,
//!   MATCH 0 recalls the same notes, MATCH 1 the same sound.
//! * TEXTURE runs seamless → shattered: at the top, grains leap octaves.
//! * FOCUS is attention's temperature: soft focus blends B in, sharp
//!   focus does not.
//! * DÉJÀ gates by age: memories younger than it are never recalled.
//! * FORGET wipes the bank; and the memory LETS GO when you stop.
//! * Maxed everything under noise stays bounded.
//! * The GENERATED ROOM (SPACE 1) is a reverb, DECAY stretches it, the
//!   map spans rooms from small to vast (all bounded), and memory STEERS
//!   it: a recalled moment pulls the room toward where it was heard.
//!   (The knob-room properties above pin SPACE 0.)

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], at: &[(f32, &str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_engram_vst3::Engram::new();
    plugin.debug_attach(DebugBus::new());
    let table = plugin.param_table();
    let find = |n: &str| table.by_name(n).unwrap_or_else(|| panic!("param '{n}'"));
    for (n, v) in set {
        find(n).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(input.len());
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for (b, chunk) in input.chunks(BLOCK).enumerate() {
        let t = (b * BLOCK) as f32 / FS;
        for (s, n, v) in at {
            if t >= *s && t < *s + BLOCK as f32 / FS {
                find(n).set_plain(*v);
            }
        }
        il[..chunk.len()].copy_from_slice(chunk);
        il[chunk.len()..].fill(0.0);
        ir.copy_from_slice(&il);
        let mut ip = [core::ptr::null_mut(); 2];
        let mut op = [core::ptr::null_mut(); 2];
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: AudioBuffer::from_slices(&mut ip, [&mut il, &mut ir], BLOCK),
            output: AudioBuffer::from_slices(&mut op, [&mut ol, &mut or], BLOCK),
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&ol[..chunk.len()]);
    }
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut s = 0x1234_5678u32;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

fn wet() -> Vec<(&'static str, f64)> {
    vec![("mix", 1.0), ("recall", 0.0), ("size", 0.5), ("tone", 0.7), ("plasticity", 0.0)]
}

fn with(mut base: Vec<(&'static str, f64)>, more: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    base.extend_from_slice(more);
    base
}

/// A click at t = 0 in `secs` of silence.
fn click(secs: f32) -> Vec<f32> {
    let mut x = vec![0.0f32; (FS * secs) as usize];
    for (i, v) in x.iter_mut().take(24).enumerate() {
        *v = 0.8 * (1.0 - i as f32 / 24.0);
    }
    x
}

/// T60 from the tail's energy decay between −10 and −30 dB of its
/// 50–150 ms level (Schroeder-free, window RMS; fine for a property).
fn t60(out: &[f32]) -> f32 {
    let win = 2_400;
    let level = |t: usize| 20.0 * rms(&out[t..t + win]).max(1e-12).log10();
    let start = level((FS * 0.05) as usize);
    let (mut t10, mut t30) = (None, None);
    let mut t = (FS * 0.05) as usize;
    while t + win < out.len() {
        let l = level(t);
        if t10.is_none() && l < start - 10.0 {
            t10 = Some(t);
        }
        if l < start - 30.0 {
            t30 = Some(t);
            break;
        }
        t += win / 2;
    }
    match (t10, t30) {
        (Some(a), Some(b)) => 3.0 * (b - a) as f32 / FS,
        _ => f32::INFINITY,
    }
}

#[test]
fn mix_zero_is_bit_exact_and_silence_is_silent() {
    let input = noise(24_000, 0.5);
    let out = render(&[("mix", 0.0), ("decay", 1.0), ("recall", 1.0)], &[], &input);
    for (i, x) in input.iter().enumerate() {
        assert!(x.to_bits() == out[i].to_bits(), "sample {i}");
    }
    let quiet = render(&with(wet(), &[("recall", 1.0), ("decay", 1.0), ("deja", 0.0)]), &[(0.5, "hold", 1.0)], &vec![0.0; 96_000]);
    assert!(quiet.iter().all(|x| *x == 0.0), "the room made sound from silence");
}

#[test]
fn it_is_a_reverb_and_decay_sets_the_tail() {
    let short = t60(&render(&with(wet(), &[("decay", 1.0), ("space", 0.0)]), &[], &click(6.0)));
    let long = t60(&render(&with(wet(), &[("decay", 4.0), ("space", 0.0)]), &[], &click(12.0)));
    assert!((short - 1.0).abs() < 0.4, "DECAY 1 s measured T60 {short:.2} s");
    assert!((long - 4.0).abs() < 1.4, "DECAY 4 s measured T60 {long:.2} s");
}

#[test]
fn hold_freezes_the_room() {
    let out = render(&with(wet(), &[("decay", 1.0), ("space", 0.0)]), &[(0.3, "hold", 1.0)], &click(8.0));
    let late = rms(&out[(FS * 6.0) as usize..(FS * 7.0) as usize]);
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(late > 1e-3, "held, the room still died (rms {late})");
    assert!(peak < 3.0, "held, the room ran away (peak {peak})");
}

#[test]
fn the_drawing_shapes_the_room_and_every_drawing_is_stable() {
    // Every node dragged to the centre (a tiny, tightly coupled room)
    // vs every node at the rim (a big, sparse one).
    let mut tight: Vec<(&'static str, f64)> = with(wet(), &[("decay", 2.0), ("plasticity", 0.0), ("space", 0.0)]);
    let mut wide = tight.clone();
    for i in 0..16 {
        let a = std::f64::consts::TAU * i as f64 / 16.0;
        tight.push((NODE_X[i], 0.5 + 0.02 * a.cos()));
        tight.push((NODE_Y[i], 0.5 + 0.02 * a.sin()));
        wide.push((NODE_X[i], 0.5 + 0.49 * a.cos()));
        wide.push((NODE_Y[i], 0.5 + 0.49 * a.sin()));
    }
    let a = render(&tight, &[], &click(3.0));
    let b = render(&wide, &[], &click(3.0));
    for (name, out) in [("tight", &a), ("wide", &b)] {
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(out.iter().all(|x| x.is_finite()) && peak < 3.0, "{name} drawing ran away (peak {peak})");
    }
    // The wide room's first reflections arrive later (its lines are
    // longer): its energy in the first 50 ms is a far smaller share of
    // its first 400 ms than the tight room's.
    let early = |x: &[f32]| rms(&x[..2_400]) / rms(&x[..19_200]);
    // Measured 2026-11-09: tight 1.39, wide 0.84 (0.60×).
    assert!(early(&b) < 0.75 * early(&a), "tight {:.2} vs wide {:.2} early share", early(&a), early(&b));
}

const NODE_X: [&str; 16] = [
    "node 1 x", "node 2 x", "node 3 x", "node 4 x", "node 5 x", "node 6 x", "node 7 x", "node 8 x",
    "node 9 x", "node 10 x", "node 11 x", "node 12 x", "node 13 x", "node 14 x", "node 15 x", "node 16 x",
];
const NODE_Y: [&str; 16] = [
    "node 1 y", "node 2 y", "node 3 y", "node 4 y", "node 5 y", "node 6 y", "node 7 y", "node 8 y",
    "node 9 y", "node 10 y", "node 11 y", "node 12 y", "node 13 y", "node 14 y", "node 15 y", "node 16 y",
];

#[test]
fn everything_maxed_stays_bounded() {
    let out = render(&[("mix", 1.0), ("decay", 30.0), ("recall", 1.0), ("focus", 0.0), ("deja", 0.0), ("size", 1.0), ("tone", 1.0), ("plasticity", 1.0)], &[(2.0, "hold", 1.0)], &noise((FS * 5.0) as usize, 0.7));
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(out.iter().all(|x| x.is_finite()) && peak < 3.0, "peak {peak}");
}


// ------------------------------------------------------------ memory

/// A held chord: three notes, each with two harmonics.
fn chord(freqs: [f32; 3], secs: f32) -> Vec<f32> {
    (0..(FS * secs) as usize)
        .map(|i| {
            let t = i as f32 / FS;
            freqs.iter().map(|f| {
                let w = 2.0 * std::f32::consts::PI * f * t;
                0.12 * (w.sin() + 0.4 * (2.0 * w).sin() + 0.15 * (3.0 * w).sin())
            }).sum()
        })
        .collect()
}

const A: [f32; 3] = [220.0, 261.63, 329.63]; // A C E
const B: [f32; 3] = [185.0, 233.08, 277.18]; // F# A# C#

/// A, a breath, B, a breath, A again (the second A starts at 7 s).
fn a_b_a() -> Vec<f32> {
    let mut x = chord(A, 3.0);
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(chord(B, 3.0));
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(chord(A, 2.5));
    x.extend(std::iter::repeat_n(0.0, (FS * 6.0) as usize));
    x
}

/// Energy on a chord's notes (± 6 Hz round each of the first three
/// harmonics of each note) in `x`.
fn on_chord(x: &[f32], freqs: [f32; 3]) -> f64 {
    let n = x.len().next_power_of_two() / 2;
    let mut re: Vec<f64> = x[..n]
        .iter()
        .enumerate()
        .map(|(i, v)| f64::from(*v) * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()))
        .collect();
    let mut im = vec![0.0f64; n];
    stave_measure::spectrum::fft(&mut re, &mut im);
    let bin = f64::from(FS) / n as f64;
    let mut e = 0.0;
    for f in freqs {
        for h in 1..=3 {
            let c = f64::from(f) * h as f64;
            for k in ((c - 6.0) / bin) as usize..=((c + 6.0) / bin) as usize {
                e += re[k] * re[k] + im[k] * im[k];
            }
        }
    }
    e
}

/// What the MEMORY added: the render with RECALL up minus the same
/// render with RECALL off (plasticity off, so nothing else differs).
/// A short room (0.4 s) so that what is measured is what is recalled
/// NOW, not the room still ringing with what was recalled earlier.
fn memory_part(set: &[(&'static str, f64)], at: &[(f32, &str, f64)], input: &[f32]) -> Vec<f32> {
    let base = with(wet(), &[("decay", 0.4), ("deja", 2.0), ("space", 0.0), ("drift", 0.0)]);
    let on = render(&with(with(base.clone(), set), &[("recall", 1.0)]), at, input);
    let off = render(&with(with(base, set), &[("recall", 0.0)]), at, input);
    on.iter().zip(&off).map(|(a, b)| a - b).collect()
}

/// The memory's answer during the second A (8.0 – 8.68 s).
fn during_second_a(m: &[f32]) -> &[f32] {
    &m[(FS * 8.0) as usize..(FS * 8.0) as usize + 32_768]
}

#[test]
fn it_remembers_by_harmony() {
    let m = memory_part(&[("focus", 0.9)], &[], &a_b_a());
    let w = during_second_a(&m);
    let (a, b) = (on_chord(w, A), on_chord(w, B));
    assert!(rms(w) > 1e-3, "the memory said nothing during the second A (rms {})", rms(w));
    assert!(a > 10.0 * b, "playing A again, it recalled B: A {a:.3e} vs B {b:.3e}");
}

#[test]
fn focus_is_attentions_temperature() {
    let share = |focus: f64| {
        let m = memory_part(&[("focus", focus)], &[], &a_b_a());
        let w = during_second_a(&m);
        let (a, b) = (on_chord(w, A), on_chord(w, B));
        b / (a + b)
    };
    let (soft, sharp) = (share(0.0), share(1.0));
    assert!(soft > 0.03, "soft focus should blend B in (B share {soft:.3})");
    assert!(soft > 3.0 * sharp, "soft B share {soft:.3} vs sharp {sharp:.3}");
}

#[test]
fn deja_gates_recall_by_age() {
    // DÉJÀ 30 s: every memory is younger than that — nothing to recall.
    let m = memory_part(&[("focus", 0.9), ("deja", 30.0)], &[], &a_b_a());
    let quiet = rms(during_second_a(&m));
    assert!(quiet < 1e-5, "with DÉJÀ 30 s it recalled a memory only seconds old (rms {quiet})");
}

#[test]
fn forget_wipes_the_bank() {
    // Press FORGET in the breath before the second A.
    let m = memory_part(&[("focus", 0.9)], &[(6.8, "forget", 1.0)], &a_b_a());
    let after = rms(during_second_a(&m));
    assert!(after < 1e-5, "after FORGET it still remembered (rms {after})");
}

#[test]
fn the_memory_lets_go_when_you_stop() {
    // The input ends at 9.5 s; DECAY 3 s (the recall fades at the
    // room's T60).
    let m = memory_part(&[("focus", 0.9), ("decay", 3.0)], &[], &a_b_a());
    let at = |t: f32| rms(&m[(FS * t) as usize..(FS * t) as usize + 4_800]);
    let (now, later) = (at(9.4), at(14.0));
    assert!(later < 0.01 * now, "4.5 s after you stopped the memory is still at {:.1} dB", 20.0 * (later / now).log10());
}

// --------------------------------------------------------- percussion

/// A drum pattern: `hit(t)` for each hit, every `every` seconds.
fn pattern(secs: f32, every: f32, hit: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..(FS * secs) as usize).map(|i| { let t = i as f32 / FS; hit(t % every) }).collect()
}

/// A kick: a 60 Hz thump, falling in pitch, decaying in 120 ms.
fn kick(t: f32) -> f32 {
    0.7 * (-t / 0.12).exp() * (2.0 * std::f32::consts::PI * (60.0 * t + 40.0 * (1.0 - (-t / 0.03).exp()) * 0.03)).sin()
}

/// A hat: bright noise (a crude high-pass: successive differences),
/// decaying in 25 ms.
fn hat(t: f32) -> f32 {
    let i = (t * FS) as u32;
    let n = |k: u32| { let mut h = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B; h ^= h >> 15; h = h.wrapping_mul(0x2C1B_3C6D); h ^= h >> 12; h as f32 / u32::MAX as f32 - 0.5 };
    0.9 * (-t / 0.025).exp() * (n(i) - n(i.wrapping_sub(1)))
}

/// Kicks, a breath, hats, a breath, kicks again (from 7 s).
fn k_h_k() -> Vec<f32> {
    let mut x = pattern(3.0, 0.25, kick);
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(pattern(3.0, 0.125, hat));
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(pattern(2.5, 0.25, kick));
    x.extend(std::iter::repeat_n(0.0, (FS * 4.0) as usize));
    x
}

/// Energy below `lo` Hz and above `hi` Hz.
fn low_high(x: &[f32], lo: f64, hi: f64) -> (f64, f64) {
    let n = x.len().next_power_of_two() / 2;
    let mut re: Vec<f64> = x[..n].iter().enumerate()
        .map(|(i, v)| f64::from(*v) * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()))
        .collect();
    let mut im = vec![0.0f64; n];
    stave_measure::spectrum::fft(&mut re, &mut im);
    let bin = f64::from(FS) / n as f64;
    let (mut l, mut h) = (0.0, 0.0);
    for k in 1..n / 2 {
        let e = re[k] * re[k] + im[k] * im[k];
        if (k as f64 * bin) < lo {
            l += e;
        } else if (k as f64 * bin) > hi {
            h += e;
        }
    }
    (l, h)
}

#[test]
fn it_remembers_drums_by_their_sound() {
    let m = memory_part(&[("focus", 0.9), ("texture", 0.1)], &[], &k_h_k());
    let w = during_second_a(&m);
    let (low, high) = low_high(w, 250.0, 3_000.0);
    assert!(rms(w) > 1e-3, "the memory said nothing during the second kicks (rms {})", rms(w));
    assert!(low > 5.0 * high, "playing kicks again, it recalled hats: low {low:.3e} vs high {high:.3e}");
}

#[test]
fn percussive_memories_answer_your_hits() {
    // The tightest room (recalled memories mostly play INTO the room,
    // and a big one smears hits 250 ms apart into each other).
    let m = memory_part(&[("focus", 0.9), ("texture", 0.1), ("size", 0.0), ("decay", 0.3)], &[], &k_h_k());
    // The second kicks land every 0.25 s from 7.0 s. Energy in the
    // 40 ms after each hit vs the 40 ms halfway between hits.
    let win = (0.04 * FS) as usize;
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for k in 4..9 {
        let hit = (FS * (7.0 + 0.25 * k as f32)) as usize + (0.012 * FS) as usize;
        let mid = hit + (0.125 * FS) as usize;
        on += f64::from(rms(&m[hit..hit + win]));
        off += f64::from(rms(&m[mid..mid + win]));
    }
    // Measured 2026-11-10: 1.86× here (the bank's raw answers are ~4×;
    // the rest is the room's own reverb of each answer).
    assert!(on > 1.5 * off, "the memory does not answer on the hits: on {on:.4} vs between {off:.4}");
}

/// Energy on the notes' FUNDAMENTALS only (± 6 Hz).
fn on_notes(x: &[f32], freqs: [f32; 3]) -> f64 {
    let n = x.len().next_power_of_two() / 2;
    let mut re: Vec<f64> = x[..n].iter().enumerate()
        .map(|(i, v)| f64::from(*v) * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()))
        .collect();
    let mut im = vec![0.0f64; n];
    stave_measure::spectrum::fft(&mut re, &mut im);
    let bin = f64::from(FS) / n as f64;
    freqs.iter().map(|f| {
        let c = f64::from(*f);
        (((c - 6.0) / bin) as usize..=((c + 6.0) / bin) as usize).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f64>()
    }).sum()
}

/// A chord of pure sines (`bright` adds a saw-like stack of partials).
fn voiced(freqs: [f32; 3], secs: f32, bright: bool) -> Vec<f32> {
    (0..(FS * secs) as usize)
        .map(|i| {
            let t = i as f32 / FS;
            freqs.iter().map(|f| {
                let w = 2.0 * std::f32::consts::PI * f * t;
                if bright { (1..=14).map(|h| 0.12 * (h as f32 * w).sin() / h as f32).sum::<f32>() } else { 0.12 * w.sin() }
            }).sum()
        })
        .collect()
}

#[test]
fn match_decides_what_alike_means() {
    // Memories: A played BRIGHT, then B played DARK. Query: A played
    // DARK — the harmony says A, the sound says B.
    let mut x = voiced(A, 3.0, true);
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(voiced(B, 3.0, false));
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(voiced(A, 2.5, false));
    x.extend(std::iter::repeat_n(0.0, (FS * 3.0) as usize));
    let recalled = |m: f64| {
        let part = memory_part(&[("focus", 0.9), ("match", m), ("texture", 0.0)], &[], &x);
        let w = during_second_a(&part);
        (on_chord(w, A), on_chord(w, B))
    };
    let (ha, hb) = recalled(0.0);
    let (ta, tb) = recalled(1.0);
    assert!(ha > 3.0 * hb, "MATCH 0 (harmony) should recall the A: A {ha:.3e} vs B {hb:.3e}");
    assert!(tb > 3.0 * ta, "MATCH 1 (sound) should recall the dark B: A {ta:.3e} vs B {tb:.3e}");
}

#[test]
fn texture_runs_from_seamless_to_shattered() {
    // A pure-sine A remembered, then recalled: at TEXTURE 1 the grains
    // leap octaves — energy appears an octave away from the notes.
    let mut x = voiced(A, 3.0, false);
    x.extend(std::iter::repeat_n(0.0, (FS * 4.5) as usize));
    x.extend(voiced(A, 2.5, false));
    x.extend(std::iter::repeat_n(0.0, (FS * 3.0) as usize));
    let leap_share = |texture: f64| {
        let part = memory_part(&[("focus", 0.9), ("texture", texture)], &[], &x);
        let w = during_second_a(&part);
        let up = A.map(|f| f * 2.0);
        let down = A.map(|f| f * 0.5);
        (on_notes(w, up) + on_notes(w, down)) / on_notes(w, A).max(1e-12)
    };
    let (smooth, shattered) = (leap_share(0.0), leap_share(1.0));
    assert!(shattered > 4.0 * smooth.max(1e-3), "TEXTURE 1 leap share {shattered:.3} vs TEXTURE 0 {smooth:.3}");
}

// ------------------------------------------------------ generated room

fn generated() -> Vec<(&'static str, f64)> {
    with(wet(), &[("space", 1.0), ("drift", 0.0), ("steer", 0.0), ("tone", 0.6), ("decay", 3.0)])
}

/// T60 of the generated room's impulse response at a map point.
fn map_t60(x: f64, y: f64, secs: f32) -> f32 {
    t60(&render(&with(generated(), &[("space x", x), ("space y", y)]), &[], &click(secs)))
}

#[test]
fn the_generated_room_is_a_reverb_and_decay_stretches_it() {
    let base = t60(&render(&with(generated(), &[("decay", 3.0)]), &[], &click(12.0)));
    let long = t60(&render(&with(generated(), &[("decay", 6.0)]), &[], &click(20.0)));
    assert!(base.is_finite() && base > 0.1, "T60 {base}");
    let r = long / base;
    assert!((1.5..2.6).contains(&r), "DECAY ×2 stretched T60 ×{r:.2} ({base:.2} → {long:.2} s)");
}

/// Every map point, at DECAY's top under loud noise, never RUNS AWAY:
/// finite, sane headroom (a 30 s room under −3 dBFS noise peaks near 3
/// by physics), and once the noise stops it decays. (Per-pass stability
/// is proven in decay_eq's `no_frequency_ever_gains`.) And the map spans
/// rooms from small to vast.
#[test]
fn the_map_spans_small_to_vast_rooms_all_bounded() {
    let input = noise((FS * 3.0) as usize, 0.7);
    let peak = |out: &[f32]| out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let mut t = Vec::new();
    for i in 0..4 {
        for j in 0..4 {
            let (x, y) = (0.1 + 0.8 * f64::from(i) / 3.0, 0.1 + 0.8 * f64::from(j) / 3.0);
            t.push(map_t60(x, y, 14.0));
            let mut x_in = input.clone();
            x_in.extend(std::iter::repeat_n(0.0, (FS * 3.0) as usize));
            let out = render(&with(generated(), &[("space x", x), ("space y", y), ("decay", 30.0)]), &[], &x_in);
            let p = peak(&out);
            let after = |a: f32, b: f32| rms(&out[(FS * a) as usize..(FS * b) as usize]);
            assert!(out.iter().all(|v| v.is_finite()) && p < 4.0, "map ({x:.2}, {y:.2}): peak {p:.2}");
            assert!(after(5.5, 6.0) < after(3.0, 3.5), "map ({x:.2}, {y:.2}) does not decay once the noise stops");
        }
    }
    let lo = t.iter().copied().fold(f32::MAX, f32::min);
    let hi = t.iter().copied().filter(|v| v.is_finite()).fold(0.0f32, f32::max);
    assert!(hi / lo > 3.0, "the map's rooms span only {lo:.2} … {hi:.2} s");
}

/// Play A in a small room, B in a vast one, then A again (the map left
/// on the vast room): with STEER the room is pulled back toward where A
/// was heard, and the tail after the second A is shorter.
#[test]
fn memory_steers_the_room_to_where_it_heard_you() {
    // The smallest and the largest room on a coarse scan.
    let mut pts = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let (x, y) = (0.15 + 0.35 * f64::from(i), 0.15 + 0.35 * f64::from(j));
            pts.push((map_t60(x, y, 14.0), x, y));
        }
    }
    pts.retain(|p| p.0.is_finite());
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (small, vast) = (pts[0], pts[pts.len() - 1]);
    assert!(vast.0 > 2.0 * small.0, "scan found no contrast: {small:?} {vast:?}");

    let mut x = chord(A, 4.0);
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(chord(B, 4.0));
    x.extend(std::iter::repeat_n(0.0, (FS * 0.5) as usize));
    x.extend(chord(A, 4.0));
    let stop = x.len();
    x.extend(std::iter::repeat_n(0.0, (FS * 10.0) as usize));
    let tail_t60 = |steer: f64| {
        let set = with(generated(), &[("space x", small.1), ("space y", small.2), ("steer", steer), ("deja", 3.0), ("focus", 0.8)]);
        let out = render(&set, &[(4.25, "space x", vast.1), (4.25, "space y", vast.2)], &x);
        t60(&out[stop..])
    };
    let (free, steered) = (tail_t60(0.0), tail_t60(1.0));
    assert!(steered < 0.75 * free, "STEER 1 tail {steered:.2} s vs STEER 0 {free:.2} s (rooms {:.2} / {:.2} s)", small.0, vast.0);
}

/// HOLD in the generated room: once held, the tail neither dies nor grows.
#[test]
fn hold_freezes_the_generated_room() {
    let out = render(&with(generated(), &[("decay", 3.0)]), &[(0.05, "hold", 1.0)], &click(8.0));
    let (a, b) = (rms(&out[(FS * 3.0) as usize..(FS * 4.0) as usize]), rms(&out[(FS * 7.0) as usize..(FS * 8.0) as usize]));
    assert!(b > 1e-4 && (b / a).log10().abs() * 20.0 < 3.0, "held: {:.1} dB over 4 s", 20.0 * (b / a).log10());
}
