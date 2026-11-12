//! Ribbon's OWN properties — what a physical tape loop promises,
//! measured through the whole plugin (tube preamp → record head →
//! the loop → playback heads → spring → mix):
//! * MIX 0 is bit-exact dry.
//! * Silence stays silent on fresh tape; WEAR brings the hiss.
//! * The heads LAND: each playback head's echo arrives at its distance
//!   over the tape speed (1 : 2 : 3), and RATE scales them all.
//! * Speed IS pitch: what is on the tape plays back an octave down
//!   when the motor runs at half speed.
//! * ERASE: wiped, the loop forgets after the last head; unwiped, the
//!   recording comes back around a full loop later (sound-on-sound).
//! * The BRAKE stops the tape — and the loop keeps what it holds.
//! * The SPRING rings before the first echo can arrive.
//! * Maxed INTENSITY + DRIVE self-oscillates but stays bounded.

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], at: &[(f32, &str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_ribbon_vst3::Ribbon::new();
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

fn tone(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / FS).sin() * amp).collect()
}

/// A short click at t = 0 inside `secs` of silence.
fn click(secs: f32) -> Vec<f32> {
    let mut x = vec![0.0f32; (FS * secs) as usize];
    for (i, v) in x.iter_mut().take(48).enumerate() {
        *v = 0.8 * (1.0 - i as f32 / 48.0);
    }
    x
}

/// Where the loudest moment of `x` falls between `from` and `to` s.
fn loudest_at(x: &[f32], from: f32, to: f32) -> f32 {
    let (a, b) = ((from * FS) as usize, (to * FS) as usize);
    let k = (a..b).max_by(|p, q| x[*p].abs().total_cmp(&x[*q].abs())).unwrap();
    k as f32 / FS
}

fn peak_hz(x: &[f32]) -> f32 {
    let n = x.len();
    let mag = |f: f32| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            let ph = 2.0 * std::f64::consts::PI * f64::from(f) * i as f64 / f64::from(FS);
            re += f64::from(*v) * w * ph.cos();
            im += f64::from(*v) * w * ph.sin();
        }
        re.hypot(im)
    };
    let mut best = (0.0f32, 0.0f64);
    let mut f = 100.0f32;
    while f < 4_000.0 {
        let m = mag(f);
        if m > best.1 {
            best = (f, m);
        }
        f *= 1.005;
    }
    best.0
}

/// A clean wet-only machine: no spring, no wear, one repeat.
fn clean() -> Vec<(&'static str, f64)> {
    vec![("mix", 1.0), ("spring", 0.0), ("wear", 0.0), ("intensity", 0.0), ("drive", 0.0), ("erase", 1.0)]
}

fn with(mut base: Vec<(&'static str, f64)>, more: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    base.extend_from_slice(more);
    base
}

#[test]
fn mix_zero_is_bit_exact_dry() {
    let input = noise(24_000, 0.5);
    let out = render(&[("mix", 0.0), ("intensity", 1.0), ("wear", 1.0), ("spring", 1.0), ("drive", 1.0)], &[], &input);
    for (i, x) in input.iter().enumerate() {
        assert!(x.to_bits() == out[i].to_bits(), "sample {i}");
    }
}

#[test]
fn silence_is_silent_on_fresh_tape_and_hisses_when_worn() {
    let quiet = vec![0.0f32; 96_000];
    let fresh = render(&with(clean(), &[("intensity", 1.0), ("spring", 1.0)]), &[], &quiet);
    assert!(fresh.iter().all(|x| *x == 0.0), "fresh tape made noise");
    let worn = render(&with(clean(), &[("wear", 0.8)]), &[], &quiet);
    let floor = rms(&worn[48_000..]);
    assert!(floor > 1e-4 && floor < 0.05, "worn tape hiss rms {floor}");
}

#[test]
fn the_heads_land_at_one_two_three() {
    // Heads 1, 2, 3 alone at RATE 1: echoes at 90, 180, 270 ms.
    for (sel, ms) in [(0.0, 90.0f32), (1.0, 180.0), (2.0, 270.0)] {
        let out = render(&with(clean(), &[("heads", sel), ("rate", 1.0)]), &[], &click(0.6));
        let at = loudest_at(&out, 0.02, 0.5) * 1000.0;
        assert!((at - ms).abs() < 4.0, "head {sel}: echo at {at:.1} ms, expected {ms}");
    }
    // RATE 0.5: the tape crawls, head 1 lands twice as late.
    let out = render(&with(clean(), &[("heads", 0.0), ("rate", 0.5)]), &[], &click(0.6));
    let at = loudest_at(&out, 0.02, 0.5) * 1000.0;
    assert!((at - 180.0).abs() < 5.0, "head 1 at RATE 0.5: {at:.1} ms, expected 180");
}

#[test]
fn speed_is_pitch() {
    // Fill the whole loop (4 s of tape at RATE 1) with a 1 kHz tone on
    // unwiped tape, stop playing, then halve the motor: what is on the
    // tape comes back an octave down.
    let n = (FS * 6.0) as usize;
    let mut input = tone(n, 1_000.0, 0.3);
    input[(FS * 3.95) as usize..].fill(0.0);
    let set = with(clean(), &[("erase", 0.0), ("heads", 0.0), ("rate", 1.0)]);
    let out = render(&set, &[(3.95, "rate", 0.5)], &input);
    let a = (FS * 5.5) as usize;
    let hz = peak_hz(&out[a..a + 8_192]);
    assert!((hz - 500.0).abs() < 15.0, "half-speed tape played {hz:.0} Hz, expected 500");
}

#[test]
fn erase_decides_whether_the_loop_remembers() {
    // A burst, then silence past one full loop (4 s of tape at RATE 1).
    let mut input = noise((FS * 6.0) as usize, 0.4);
    input[(FS * 0.3) as usize..].fill(0.0);
    let back = |erase: f64| {
        let out = render(&with(clean(), &[("erase", erase), ("heads", 0.0)]), &[], &input);
        rms(&out[(FS * 4.0) as usize..(FS * 4.5) as usize])
    };
    let (wiped, kept) = (back(1.0), back(0.0));
    assert!(wiped < 1e-5, "ERASE 1: the loop came back (rms {wiped})");
    assert!(kept > 0.02, "ERASE 0: the recording did not come around (rms {kept})");
}

#[test]
fn the_brake_stops_the_tape_and_the_loop_keeps_it() {
    let n = (FS * 9.0) as usize;
    let mut input = noise(n, 0.4);
    input[(FS * 0.5) as usize..].fill(0.0);
    let set = with(clean(), &[("erase", 0.0), ("heads", 6.0)]);
    let out = render(&set, &[(0.6, "brake", 1.0), (5.0, "brake", 0.0)], &input);
    let stopped = rms(&out[(FS * 3.5) as usize..(FS * 4.5) as usize]);
    let rolling = rms(&out[(FS * 6.5) as usize..(FS * 9.0) as usize]);
    assert!(stopped < 2e-3, "braked tape still playing (rms {stopped})");
    assert!(rolling > 0.02, "after the brake the loop was empty (rms {rolling})");
}

#[test]
fn the_spring_rings_before_the_first_echo() {
    let early = |spring: f64| {
        let out = render(&with(clean(), &[("spring", spring), ("rate", 1.0)]), &[], &click(0.3));
        rms(&out[(FS * 0.01) as usize..(FS * 0.06) as usize])
    };
    let (dry_tank, wet_tank) = (early(0.0), early(1.0));
    assert!(dry_tank < 1e-4, "no spring, yet sound before the first head ({dry_tank})");
    assert!(wet_tank > 1e-3, "the spring did not ring ({wet_tank})");
}

#[test]
fn runaway_is_bounded() {
    let mut input = noise((FS * 6.0) as usize, 0.5);
    input[(FS * 0.5) as usize..].fill(0.0);
    let out = render(&with(clean(), &[("intensity", 1.0), ("drive", 1.0), ("heads", 6.0), ("spring", 1.0)]), &[], &input);
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let late = rms(&out[(FS * 5.0) as usize..]);
    assert!(out.iter().all(|x| x.is_finite()) && peak < 3.0, "runaway peak {peak}");
    assert!(late > 0.01, "maxed INTENSITY did not self-oscillate (rms {late})");
}
