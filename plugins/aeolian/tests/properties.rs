//! Aeolian's OWN properties — the promises a sympathetic string bank
//! makes, measured through the whole plugin (detector bank → twelve
//! strings → ensemble → tone → mix):
//! * MIX 0 is bit-exact dry.
//! * Silence stays silent — until WIND and SUSTAIN are both near
//!   the top, where the strings SELF-OSCILLATE on the noise floor.
//! * Sympathy: a played note rings ITS string and not the others —
//!   in any octave (strings are pitch CLASSES).
//! * SUSTAIN is the decay: more sustain, a longer ring; maxed under
//!   sustained noise it stays bounded.
//! * OCTAVE moves the strings' register (LOW → HIGH = four octaves).
//! * WIND is velocity: a quiet note wakes the strings only when the
//!   sensitivity is up.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx, Tap};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const NOTES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A render: output (left, right) and every string's LED level after
/// the last block.
struct Run {
    l: Vec<f32>,
    r: Vec<f32>,
    leds: [f64; 12],
}

fn render(set: &[(&str, f64)], input: &[f32]) -> Run {
    let mut plugin = stave_aeolian_vst3::Aeolian::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table.by_name(n).unwrap_or_else(|| panic!("param '{n}'")).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let (mut l, mut r) = (Vec::with_capacity(input.len()), Vec::with_capacity(input.len()));
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for chunk in input.chunks(BLOCK) {
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
        l.extend_from_slice(&ol[..chunk.len()]);
        r.extend_from_slice(&or[..chunk.len()]);
    }
    let mut leds = [0.0; 12];
    for (k, led) in leds.iter_mut().enumerate() {
        *led = Tap::new(stave_aeolian_vst3::LED_TAPS[k]).get(&bus).unwrap_or(0.0);
    }
    let _ = Arc::strong_count(&bus);
    Run { l, r, leds }
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

/// A plucked note: a decaying tone with a few harmonics, then silence.
fn pluck(hz: f32, amp: f32, secs: f32, total: f32) -> Vec<f32> {
    let n = (FS * total) as usize;
    let len = (FS * secs) as usize;
    (0..n)
        .map(|i| {
            if i >= len {
                return 0.0;
            }
            let t = i as f32 / FS;
            let env = (-t * 3.0).exp() * (1.0 - (-(i as f32) / 40.0).exp());
            let w = 2.0 * std::f32::consts::PI * hz * t;
            amp * env * (w.sin() + 0.4 * (2.0 * w).sin() + 0.15 * (3.0 * w).sin())
        })
        .collect()
}

fn hz(class: usize, octave: i32) -> f32 {
    16.351_6 * 2f32.powf(octave as f32 + class as f32 / 12.0)
}

/// The fundamental of a slice: the LOWEST partial above `min_hz`
/// within 6 dB of the strongest (a string's 2nd harmonic can rival
/// its fundamental, so the loudest peak is not the pitch).
fn fundamental_hz(x: &[f32], min_hz: f32) -> f32 {
    let n = x.len();
    let mag = |f: f32| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            let ph = 2.0 * std::f64::consts::PI * f64::from(f) * i as f64 / f64::from(FS);
            re += f64::from(*v) * w * ph.cos();
            im += f64::from(*v) * w * ph.sin();
        }
        re.hypot(im) as f32
    };
    let mut grid = Vec::new();
    let mut f = min_hz;
    while f < 4_000.0 {
        grid.push((f, mag(f)));
        f *= 1.01;
    }
    let top = grid.iter().fold(0.0f32, |m, g| m.max(g.1));
    let k = (1..grid.len() - 1)
        .find(|&k| grid[k].1 >= 0.5 * top && grid[k].1 >= grid[k - 1].1 && grid[k].1 >= grid[k + 1].1)
        .unwrap_or(0);
    let (mut lo, mut hi) = (grid[k].0 / 1.01, grid[k].0 * 1.01);
    for _ in 0..30 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if mag(a) < mag(b) {
            lo = a;
        } else {
            hi = b;
        }
    }
    0.5 * (lo + hi)
}

#[test]
fn mix_zero_is_bit_exact_dry() {
    let input = noise(24_000, 0.5);
    let run = render(&[("mix", 0.0), ("sustain", 1.0), ("wind", 1.0)], &input);
    for (i, x) in input.iter().enumerate() {
        assert!(x.to_bits() == run.l[i].to_bits() && x.to_bits() == run.r[i].to_bits(), "sample {i}");
    }
}

#[test]
fn silence_stays_silent_until_the_strings_self_oscillate() {
    let quiet = vec![0.0f32; 96_000];
    let calm = render(&[("mix", 1.0), ("sustain", 1.0), ("wind", 0.6)], &quiet);
    assert!(calm.l.iter().chain(&calm.r).all(|x| *x == 0.0), "the strings sang with no input");
    let wild = render(&[("mix", 1.0), ("sustain", 1.0), ("wind", 1.0)], &quiet);
    let late = rms(&wild.l[48_000..]);
    assert!(late > 1e-3, "maxed WIND + SUSTAIN did not self-oscillate (rms {late})");
    assert!(wild.l.iter().all(|x| x.abs() < 3.0), "self-oscillation ran away");
}

#[test]
fn a_note_rings_its_own_string_in_any_octave() {
    for (class, octave) in [(4usize, 2), (4, 4), (9, 3), (0, 5)] {
        let run = render(&[("mix", 1.0), ("sustain", 0.8), ("wind", 0.6)], &pluck(hz(class, octave), 0.3, 0.6, 1.2));
        let (best, lvl) = run.leds.iter().enumerate().fold((0, 0.0), |b, (k, v)| if *v > b.1 { (k, *v) } else { b });
        assert_eq!(best, class, "{}{octave}: the {} string rang loudest {:?}", NOTES[class], NOTES[best], run.leds);
        let others = run.leds.iter().enumerate().filter(|(k, _)| *k != class && *k != (class + 7) % 12);
        for (k, v) in others {
            assert!(*v < lvl * 0.6, "{}{octave}: the {} string rang too ({v:.2} vs {lvl:.2})", NOTES[class], NOTES[k]);
        }
    }
}

#[test]
fn feedback_is_the_decay_and_maxed_stays_bounded() {
    let note = pluck(hz(9, 3), 0.3, 0.3, 3.0);
    let ring = |fb: f64| {
        let run = render(&[("mix", 1.0), ("sustain", fb), ("wind", 0.6)], &note);
        rms(&run.l[(FS * 2.0) as usize..])
    };
    let (short, long) = (ring(0.3), ring(0.85));
    let db = 20.0 * (long / short.max(1e-9)).log10();
    assert!(db > 15.0, "SUSTAIN 0.85 rings only {db:.1} dB longer than 0.3");

    let run = render(&[("mix", 1.0), ("sustain", 1.0), ("wind", 1.0)], &noise((FS * 4.0) as usize, 0.5));
    let peak = run.l.iter().chain(&run.r).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(run.l.iter().all(|x| x.is_finite()) && peak < 3.0, "maxed strings ran away (peak {peak})");
}

#[test]
fn octave_moves_the_register_four_octaves() {
    let note = pluck(hz(9, 3), 0.3, 0.4, 1.4);
    let pitch = |oct: f64| {
        let run = render(&[("mix", 1.0), ("sustain", 0.9), ("wind", 0.6), ("register", oct), ("air", 0.0)], &note);
        fundamental_hz(&run.l[(FS * 0.9) as usize..(FS * 0.9) as usize + 8_192], 40.0)
    };
    let (lo, hi) = (pitch(0.0), pitch(4.0));
    let ratio = hi / lo;
    assert!((ratio - 16.0).abs() < 0.5, "LOW {lo:.1} Hz → HIGH {hi:.1} Hz = ×{ratio:.2}, expected ×16");
    assert!((lo - hz(9, 2)).abs() < 2.0, "LOW A string at {lo:.1} Hz, expected {:.1}", hz(9, 2));
}

#[test]
fn sens_is_velocity() {
    let soft = pluck(hz(7, 3), 0.01, 0.4, 1.2);
    let tail = |wind: f64| {
        let run = render(&[("mix", 1.0), ("sustain", 0.8), ("wind", wind)], &soft);
        rms(&run.l[(FS * 0.6) as usize..])
    };
    let (deaf, keen) = (tail(0.1), tail(0.9));
    let db = 20.0 * (keen / deaf.max(1e-9)).log10();
    assert!(db > 20.0, "a soft note: WIND 0.9 only {db:.1} dB over WIND 0.1");
}

