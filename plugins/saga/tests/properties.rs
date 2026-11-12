//! Saga's OWN properties — the promises only this plugin can make, each
//! measured through the whole plugin (host rate → clock bridge →
//! core → wet → mix):
//! * MIX 0 is bit-exact dry, in every program.
//! * Every program is ALIVE (a real wet) and BOUNDED with both loops
//!   maxed under sustained noise — the analog paths saturate, never
//!   explode.
//! * The five programs are DISTINCT (pairwise wet correlation).
//! * The octaves LAND: REGEN in II puts energy an octave up, in III
//!   an octave down.
//! * DIVE is a CLOCK: held at SHIFT 0.5×, what is already in the
//!   buffers plays an octave lower.
//! * The reverse line plays BACKWARD (a rising chirp's repeat falls).

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

/// Hann-windowed magnitude spectrum of the first `n` (power of two)
/// samples: bins 0..=n/2, bin k at k·FS/n Hz.
fn mags(x: &[f32], n: usize) -> Vec<f32> {
    let mut re: Vec<f64> = x[..n]
        .iter()
        .enumerate()
        .map(|(i, v)| f64::from(*v) * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()))
        .collect();
    let mut im = vec![0.0f64; n];
    stave_measure::spectrum::fft(&mut re, &mut im);
    (0..=n / 2).map(|k| re[k].hypot(im[k]) as f32).collect()
}

fn bin_hz(n: usize) -> f32 {
    FS / n as f32
}

/// The strongest peak above `min_hz`, parabolically interpolated.
fn peak_hz(x: &[f32], n: usize, min_hz: f32) -> f32 {
    let m = mags(x, n);
    let lo = (min_hz / bin_hz(n)).ceil() as usize;
    let k = (lo..m.len() - 1).max_by(|a, b| m[*a].total_cmp(&m[*b])).unwrap();
    let (y0, y1, y2) = (m[k - 1], m[k], m[k + 1]);
    let d = y0 - 2.0 * y1 + y2;
    let off = if d.abs() > 1e-12 { 0.5 * (y0 - y2) / d } else { 0.0 };
    (k as f32 + off) * bin_hz(n)
}

fn rms(x: &[f32]) -> f32 {
    stave_measure::level::rms(x) as f32
}

/// Render `input` (mono, fed to both ears) with `set` applied up
/// front and `at` = (sample, name, value) automation applied at the
/// block containing that sample. Returns (left, right).
pub fn render(set: &[(&str, f64)], at: &[(usize, &str, f64)], input: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut plugin = stave_saga_vst3::Saga::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    let find = |n: &str| table.by_name(n).unwrap_or_else(|| panic!("param '{n}'"));
    for (n, v) in set {
        find(n).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let (mut out_l, mut out_r) = (Vec::with_capacity(input.len()), Vec::with_capacity(input.len()));
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut ol = vec![0.0f32; BLOCK];
    let mut or = vec![0.0f32; BLOCK];
    for (b, chunk) in input.chunks(BLOCK).enumerate() {
        for (s, n, v) in at {
            if s / BLOCK == b {
                find(n).set_plain(*v);
            }
        }
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut ol, &mut or], BLOCK);
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
        out_l.extend_from_slice(&ol[..chunk.len()]);
        out_r.extend_from_slice(&or[..chunk.len()]);
    }
    let _ = Arc::strong_count(&bus);
    (out_l, out_r)
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

fn tone(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / FS).sin() * amp).collect()
}

/// Energy in a ±`width` Hz band around `hz`.
fn band(x: &[f32], hz: f32, width: f32) -> f32 {
    let n = x.len().next_power_of_two() / 2;
    let m = mags(x, n);
    let (lo, hi) = (((hz - width) / bin_hz(n)) as usize, ((hz + width) / bin_hz(n)) as usize);
    m[lo..=hi].iter().map(|v| v * v).sum()
}

fn wet_only(mode: f64) -> Vec<(&'static str, f64)> {
    vec![("mode", mode), ("mix", 1.0), ("mod", 0.0)]
}

#[test]
fn mix_zero_is_bit_exact_dry_in_every_program() {
    let input = noise(24_000, 0.5);
    for mode in 0..5 {
        let (l, r) = render(&[("mode", mode as f64), ("mix", 0.0), ("feedback", 1.0), ("regen", 1.0)], &[], &input);
        for (i, x) in input.iter().enumerate() {
            assert!(x.to_bits() == l[i].to_bits() && x.to_bits() == r[i].to_bits(), "mode {mode} sample {i}");
        }
    }
}

#[test]
fn every_program_is_alive_and_bounded_with_loops_maxed() {
    let input = noise((FS * 8.0) as usize, 0.5);
    for mode in 0..5 {
        let mut set = wet_only(mode as f64);
        set.extend([("feedback", 1.0), ("regen", 1.0), ("x", 1.0)]);
        let (l, r) = render(&set, &[], &input);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
        let late = rms(&l[l.len() - 48_000..]);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()), "mode {mode}: non-finite");
        assert!(peak < 3.0, "mode {mode}: peak {peak} — a maxed loop ran away");
        assert!(late > 0.02, "mode {mode}: wet rms {late} — program is dead");
    }
}

#[test]
fn programs_are_distinct() {
    // A plucked phrase, then silence: the tails are the programs.
    // Broadband attacks (a pick has air) over pitched bodies.
    let mut input = vec![0.0f32; (FS * 3.0) as usize];
    let burst = noise(9_000, 0.3);
    for (k, hz) in [220.0f32, 330.0, 440.0].iter().enumerate() {
        let start = k * 12_000;
        for i in 0..9_000 {
            let env = (-(i as f32) / 2_400.0).exp();
            input[start + i] += ((2.0 * std::f32::consts::PI * hz * i as f32 / FS).sin() * 0.4
                + burst[i] * (-(i as f32) / 400.0).exp())
                * env;
        }
    }
    let wets: Vec<Vec<f32>> = (0..5)
        .map(|m| {
            let mut set = wet_only(m as f64);
            set.extend([("feedback", 0.5), ("regen", 0.5)]);
            render(&set, &[], &input).0
        })
        .collect();
    // Air: the share of wet energy above 2 kHz.
    let air = |x: &[f32]| {
        let m = mags(x, 131_072);
        let edge = (2_000.0 / bin_hz(131_072)) as usize;
        let e = |m: &[f32]| m.iter().map(|v| v * v).sum::<f32>();
        e(&m[edge..]) / e(&m).max(1e-12)
    };
    let c: Vec<f32> = wets.iter().map(|w| air(w)).collect();
    for a in 0..5 {
        for b in a + 1..5 {
            let (x, y) = (&wets[a], &wets[b]);
            let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
            let n = (x.iter().map(|p| p * p).sum::<f32>() * y.iter().map(|q| q * q).sum::<f32>()).sqrt();
            let corr = dot / n.max(1e-9);
            // Measured 2026-10-25: II–III 0.82 (they share the reverse-
            // delay front end, as on the pedal); every other pair < 0.2.
            assert!(corr.abs() < 0.9, "programs {a} and {b} correlate {corr}");
        }
    }
    // II and III share a front end; the light/dark split is the
    // difference, and it must be a real one.
    // Measured 2026-10-25: II 0.0029 vs III 0.00085 (3.4×).
    assert!(c[1] > 2.5 * c[2], "II air {} vs III air {}", c[1], c[2]);
}

/// II with REGEN up puts energy an octave UP; III an octave DOWN.
#[test]
fn the_octaves_land() {
    let n = (FS * 4.0) as usize;
    let input = tone(n, 440.0, 0.3);
    let measure = |mode: f64, regen: f64| {
        let mut set = wet_only(mode);
        set.extend([("feedback", 0.0), ("regen", regen), ("x", 0.6), ("tone", 1.0)]);
        let (l, _) = render(&set, &[], &input);
        l[n - 65_536..].to_vec()
    };
    for (mode, octave) in [(1.0, 880.0), (2.0, 220.0)] {
        let off = measure(mode, 0.0);
        let on = measure(mode, 0.8);
        let gain_db = 10.0 * (band(&on, octave, 25.0) / band(&off, octave, 25.0).max(1e-12)).log10();
        assert!(gain_db > 10.0, "mode {mode}: REGEN raised {octave} Hz only {gain_db:.1} dB");
    }
}

/// Hold DIVE at 0.5× after the input stops: the buffered 1 kHz comes
/// back at 500 Hz — the clock moved, not a pitch shifter.
#[test]
fn dive_is_a_clock() {
    let n = (FS * 2.0) as usize;
    let mut input = tone(n, 1_000.0, 0.3);
    input[(FS * 0.5) as usize..].fill(0.0);
    let set = [
        ("mode", 0.0), ("mix", 1.0), ("mod", 0.0), ("x", 0.0), ("feedback", 0.95),
        ("regen", 0.0), ("time", 300.0), ("shift", 0.5), ("ramp", 50.0), ("tone", 1.0),
    ];
    let measure = |dive: bool| {
        let at = if dive { vec![((FS * 0.6) as usize, "dive", 1.0)] } else { vec![] };
        let (l, _) = render(&set, &at, &input);
        let w = &l[(FS * 1.2) as usize..(FS * 1.2) as usize + 16_384];
        peak_hz(w, 16_384, 100.0)
    };
    let (held, free) = (measure(true), measure(false));
    assert!((free - 1_000.0).abs() < 30.0, "undived tail at {free} Hz");
    assert!((held - 500.0).abs() < 20.0, "dived tail at {held} Hz, expected 500");
}

/// A rising chirp through the reverse delay comes back FALLING
/// (geometry: the repeat at 370 ms reads the chirp at ~230 ms ≈
/// 2.4 kHz, at 520 ms it reads ~80 ms ≈ 1 kHz).
#[test]
fn the_reverse_line_plays_backward() {
    let len = (FS * 0.3) as usize;
    let mut input = vec![0.0f32; (FS * 1.0) as usize];
    let mut ph = 0.0f32;
    for (i, x) in input.iter_mut().take(len).enumerate() {
        let f = 300.0 + 2_700.0 * i as f32 / len as f32;
        ph += 2.0 * std::f32::consts::PI * f / FS;
        *x = ph.sin() * 0.4;
    }
    // II with the shortest tank and no regen: the repeat is the reverse
    // delay; measure the pitch trend across the repeat's window.
    let set = [
        ("mode", 1.0), ("mix", 1.0), ("mod", 0.0), ("feedback", 0.0), ("regen", 0.0),
        ("x", 0.0), ("time", 300.0), ("tone", 1.0),
    ];
    let (l, _) = render(&set, &[], &input);
    // The dominant pitch in two 43 ms slices of the repeat.
    let pitch = |a: usize| {
        let w = &l[a..a + 2_048];
        peak_hz(w, 2_048, 150.0)
    };
    let early = pitch((FS * 0.37) as usize);
    let late = pitch((FS * 0.52) as usize);
    // Measured 2026-10-25: 2174 → 1479 Hz (the tank's tail lifts the
    // late slice). A FORWARD tap reads ~930 → ~2280 Hz (ratio 0.4).
    assert!(early > late * 1.3, "repeat not reversed: {early:.0} Hz early vs {late:.0} Hz late");
}
