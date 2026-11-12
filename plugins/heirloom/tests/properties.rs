//! Heirloom's OWN properties — the promises only this plugin makes,
//! each measured through the whole plugin (host rate → clock bridge
//! → reverse machine → mix → media):
//! * MIX 0 is bit-exact dry in every machine and medium — however
//!   hard the record wobbles and wears (the media live on the wet).
//! * Silence stays silent until AGE is up AND mixed in — the noise
//!   floor belongs to the worn medium, not the clean one.
//! * Every machine is ALIVE and BOUNDED with the loops maxed on a
//!   worn record.
//! * AGE wears: it band-limits, and each MEDIA's floor has its own
//!   grain (vinyl crackles, tape hisses).
//! * WOBBLE bends pitch; WARP's lurch-and-sag is ASYMMETRIC where
//!   the sine is even.
//! * HOLD is a CLOCK: the buffered tail plays an octave down.

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
    let mut plugin = stave_heirloom_vst3::Heirloom::new();
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


/// Clean, flat, dry-ish defaults a test then overrides.
fn clean() -> Vec<(&'static str, f64)> {
    vec![("wobble", 0.0), ("age", 0.0), ("mix", 0.0)]
}

fn with(mut base: Vec<(&'static str, f64)>, more: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    base.extend_from_slice(more);
    base
}

#[test]
fn mix_zero_is_bit_exact_dry_however_worn_the_record() {
    let input = noise(24_000, 0.5);
    for m in 0..3 {
        for media in 0..3 {
            let set = [
                ("machine", m as f64), ("media", media as f64), ("mix", 0.0), ("repeats", 1.0),
                ("wobble", 1.0), ("rpm", 8.0), ("age", 1.0),
            ];
            let (l, r) = render(&set, &[], &input);
            for (i, x) in input.iter().enumerate() {
                assert!(
                    x.to_bits() == l[i].to_bits() && x.to_bits() == r[i].to_bits(),
                    "machine {m} media {media} sample {i}"
                );
            }
        }
    }
}

#[test]
fn silence_stays_silent_until_the_record_ages() {
    let input = vec![0.0f32; 48_000];
    for m in 0..3 {
        let set = with(clean(), &[("machine", m as f64), ("mix", 0.6), ("wobble", 0.9), ("repeats", 0.9)]);
        let (l, r) = render(&set, &[], &input);
        assert!(l.iter().chain(&r).all(|x| *x == 0.0), "machine {m}: a clean record made noise");
    }
    let (l, _) = render(&with(clean(), &[("age", 1.0)]), &[], &input);
    assert!(l.iter().all(|x| *x == 0.0), "a worn record at MIX 0 made noise");
    for media in 0..3 {
        let set = with(clean(), &[("media", media as f64), ("age", 0.8), ("mix", 1.0)]);
        let (l, _) = render(&set, &[], &input);
        let floor = rms(&l[24_000..]);
        assert!(floor > 1e-4, "media {media}: worn record floor rms {floor}");
        assert!(floor < 0.1, "media {media}: floor rms {floor} drowns the music");
    }
}

#[test]
fn every_machine_is_alive_and_bounded_on_a_worn_record() {
    let input = noise((FS * 6.0) as usize, 0.5);
    for m in 0..3 {
        for media in 0..3 {
            let set = [
                ("machine", m as f64), ("media", media as f64), ("mix", 1.0), ("repeats", 1.0),
                ("age", 1.0), ("wobble", 1.0), ("rpm", 8.0),
            ];
            let (l, r) = render(&set, &[], &input);
            let peak = l.iter().chain(&r).fold(0.0f32, |p, x| p.max(x.abs()));
            let late = rms(&l[l.len() - 48_000..]);
            assert!(l.iter().chain(&r).all(|x| x.is_finite()), "machine {m} media {media}: non-finite");
            assert!(peak < 3.0, "machine {m} media {media}: peak {peak} — a maxed loop ran away");
            assert!(late > 0.02, "machine {m} media {media}: rms {late} — the machine is dead");
        }
    }
}

#[test]
fn age_band_limits_every_medium() {
    let input = noise(65_536 + 8_192, 0.5);
    let air = |x: &[f32]| band(&x[8_192..], 12_000.0, 3_000.0);
    let wet = with(clean(), &[("mix", 1.0), ("repeats", 0.0)]);
    let (fresh, _) = render(&wet, &[], &input);
    for media in 0..3 {
        let (worn, _) = render(&with(wet.clone(), &[("media", media as f64), ("age", 1.0)]), &[], &input);
        let drop_db = 10.0 * (air(&fresh) / air(&worn).max(1e-12)).log10();
        assert!(drop_db > 12.0, "media {media}: AGE cut 12 kHz only {drop_db:.1} dB");
    }
}

#[test]
fn vinyl_crackles_where_tape_hisses() {
    let input = vec![0.0f32; 96_000];
    let crest = |media: f64| {
        let (l, _) = render(&with(clean(), &[("media", media), ("age", 0.7), ("mix", 1.0)]), &[], &input);
        let w = &l[24_000..];
        w.iter().fold(0.0f32, |p, x| p.max(x.abs())) / rms(w).max(1e-12)
    };
    let (vinyl, tape) = (crest(0.0), crest(1.0));
    assert!(vinyl > 1.5 * tape, "vinyl crest {vinyl:.1} vs tape {tape:.1}");
}

/// Short-window pitch track of a 1 kHz tone through the media stage.
fn pitch_track(set: &[(&'static str, f64)], secs: f32) -> Vec<f32> {
    let n = (FS * secs) as usize;
    let set = with(set.to_vec(), &[("mix", 1.0), ("repeats", 0.0)]);
    let (l, _) = render(&set, &[], &tone(n, 1_000.0, 0.3));
    (0..)
        .map(|k| 4_800 + k * 512)
        .take_while(|a| a + 4_096 <= n)
        .map(|a| peak_hz(&l[a..a + 4_096], 4_096, 300.0))
        .collect()
}

#[test]
fn wobble_bends_pitch_well_past_the_machines_own_jitter() {
    let swing = |t: &[f32]| t.iter().cloned().fold(f32::MIN, f32::max) - t.iter().cloned().fold(f32::MAX, f32::min);
    let bent = pitch_track(&with(clean(), &[("wobble", 1.0), ("rpm", 1.0)]), 3.0);
    let flat = pitch_track(&clean(), 3.0);
    // The wet path carries the reverse machine too, whose splices
    // jitter a short-window pitch read by a few Hz (measured 4.8 Hz,
    // 2026-11-09) — the wobble must dwarf that floor.
    assert!(swing(&bent) > 20.0, "WOBBLE 1 swung only {:.1} Hz", swing(&bent));
    assert!(swing(&bent) > 3.0 * swing(&flat), "WOBBLE 1 swung {:.1} Hz vs {:.1} Hz at 0", swing(&bent), swing(&flat));
}

#[test]
fn warp_is_asymmetric_where_sine_is_even() {
    // WARP lurches the delay up fast (pitch dips hard, briefly) and
    // sags back slowly (pitch rides a little sharp, long): most of the
    // turn sits ABOVE the nominal pitch.
    let sharp_share = |warp: f64| {
        let t = pitch_track(&with(clean(), &[("wobble", 1.0), ("rpm", 0.5), ("warp", warp)]), 8.1);
        t.iter().filter(|f| **f > 1_000.0).count() as f32 / t.len() as f32
    };
    let (sine, warp) = (sharp_share(0.0), sharp_share(1.0));
    assert!((sine - 0.5).abs() < 0.12, "SINE sharp share {sine:.2}, expected ~0.5");
    assert!(warp > sine + 0.1, "WARP sharp share {warp:.2} vs SINE {sine:.2}");
}

/// Latch HOLD after the input stops: the buffered 1 kHz comes back
/// at 500 Hz — the clock moved, not a pitch shifter.
#[test]
fn hold_is_a_clock() {
    let n = (FS * 2.2) as usize;
    let mut input = tone(n, 1_000.0, 0.3);
    input[(FS * 0.5) as usize..].fill(0.0);
    let set = with(clean(), &[("machine", 0.0), ("mix", 1.0), ("repeats", 0.95), ("time", 300.0)]);
    let measure = |hold: bool| {
        let at = if hold { vec![((FS * 0.55) as usize, "hold", 1.0)] } else { vec![] };
        let (l, _) = render(&set, &at, &input);
        let a = (FS * 1.6) as usize;
        peak_hz(&l[a..a + 16_384], 16_384, 100.0)
    };
    let (held, free) = (measure(true), measure(false));
    assert!((free - 1_000.0).abs() < 30.0, "free tail at {free} Hz");
    assert!((held - 500.0).abs() < 25.0, "held tail at {held} Hz, expected 500");
}

/// The platter the face shows: run silence for `secs` with `set`,
/// switching `at` = (second, name, value); return the unwrapped spin
/// angle (radians) after every block.
fn platter(set: &[(&str, f64)], at: &[(f32, &str, f64)], secs: f32) -> Vec<f64> {
    let mut plugin = stave_heirloom_vst3::Heirloom::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table.by_name(n).unwrap().set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let tap = stave_core::Tap::new("spin");
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0; BLOCK], vec![0.0; BLOCK], vec![0.0; BLOCK], vec![0.0; BLOCK]);
    let (mut angles, mut last, mut total) = (Vec::new(), 0.0f64, 0.0f64);
    for b in 0..(FS * secs) as usize / BLOCK {
        let t = (b * BLOCK) as f32 / FS;
        for (s, n, v) in at {
            if t >= *s {
                table.by_name(n).unwrap().set_plain(*v);
            }
        }
        il.fill(0.0);
        ir.fill(0.0);
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
        let a = tap.get(&bus).unwrap();
        total += (a - last).rem_euclid(std::f64::consts::TAU);
        last = a;
        angles.push(total);
    }
    angles
}

/// Revolutions per second between two times, from a `platter` run.
fn rps(angles: &[f64], from: f32, to: f32) -> f64 {
    let blk = |t: f32| ((t * FS) as usize / BLOCK).min(angles.len() - 1);
    (angles[blk(to)] - angles[blk(from)]) / std::f64::consts::TAU / f64::from(to - from)
}

#[test]
fn the_record_turns_at_33_and_a_third_sags_on_hold_and_brakes_on_bypass() {
    let clean = [("wobble", 0.0), ("age", 0.0)];
    let free = platter(&clean, &[], 4.0);
    let nominal = rps(&free, 2.5, 4.0);
    assert!((nominal - 33.333 / 60.0).abs() < 0.01, "free platter {nominal:.3} rev/s");

    let held = platter(&clean, &[(1.0, "hold", 1.0)], 8.0);
    let sag = rps(&held, 6.5, 8.0) / nominal;
    assert!((sag - 0.5).abs() < 0.05, "HOLD platter at {sag:.2}× — the clock sag should show");

    let braked = platter(&clean, &[(2.0, "bypass", 1.0)], 9.0);
    let stopped = rps(&braked, 8.0, 9.0);
    assert!(stopped < 0.01, "bypassed platter still turning at {stopped:.3} rev/s");
}
