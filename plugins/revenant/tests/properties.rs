//! Revenant's OWN properties — what a listening, learning machine
//! promises, measured through the whole plugin (decimate → reservoir
//! → online readout → interpolate → mix):
//! * MIX 0 is bit-exact dry.
//! * A fresh brain is silent: silence in, silence out; and before it
//!   has learned anything it has nothing to say.
//! * It LEARNS: on a steady phrase its prediction of your near future
//!   converges — late output tracks the input HORIZON ahead far better
//!   than early output does.
//! * LEARN 0 never learns.
//! * It keeps playing when you stop: after learning a tone, SLEEP
//!   (fully dreaming) sustains sound near the learned pitch — in the
//!   calm regime (CHAOS below the edge). Near and past the edge the
//!   dream wanders off into the network's own attractors: that is the
//!   instrument, not a fault. Swept 2026-11-09 (`sweep_dream`): with
//!   long MEMORY the RLS generator dreams within 2% of the learned
//!   pitch at the input's level; short MEMORY wanders.
//! * CHAOS past the edge rings on its own; below it, the network
//!   settles.
//! * It lets go: when you stop playing (down to an interface's noise
//!   floor), its voice fades within seconds however high CHAOS and
//!   DREAM are — only SLEEP keeps it going.
//! * Maxed CHAOS + DREAM + LEARN under noise stays bounded.

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx, Tap};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

struct Run {
    out: Vec<f32>,
    /// Mean |activity| over the reported neurons, after every block.
    activity: Vec<f64>,
}

fn render(set: &[(&str, f64)], at: &[(f32, &str, f64)], input: &[f32]) -> Run {
    let mut plugin = stave_revenant_vst3::Revenant::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    let find = |n: &str| table.by_name(n).unwrap_or_else(|| panic!("param '{n}'"));
    for (n, v) in set {
        find(n).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let taps: Vec<Tap> = stave_revenant_vst3::NEURON_TAPS.iter().map(|n| Tap::new(n)).collect();
    let mut out = Vec::with_capacity(input.len());
    let mut activity = Vec::new();
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
        let a: f64 = taps.iter().map(|t| t.get(&bus).unwrap_or(0.0).abs()).sum::<f64>() / taps.len() as f64;
        activity.push(a);
    }
    Run { out, activity }
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

/// A steady, slightly rich tone (fundamental + two harmonics).
fn phrase(n: usize, hz: f32) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let w = 2.0 * std::f32::consts::PI * hz * i as f32 / FS;
            0.3 * (w.sin() + 0.35 * (2.0 * w).sin() + 0.15 * (3.0 * w).sin())
        })
        .collect()
}

/// Normalised correlation of `out[t]` with `inp[t + lag]` over a window.
fn tracks(out: &[f32], inp: &[f32], from: usize, len: usize, lag: usize) -> f32 {
    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
    for t in from..from + len {
        let (x, y) = (f64::from(out[t]), f64::from(inp[t + lag]));
        xy += x * y;
        xx += x * x;
        yy += y * y;
    }
    (xy / (xx * yy).sqrt().max(1e-12)) as f32
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
    let mut f = 60.0f32;
    while f < 2_000.0 {
        let m = mag(f);
        if m > best.1 {
            best = (f, m);
        }
        f *= 1.004;
    }
    best.0
}

/// Wet-only, listening (no dreaming), a short horizon.
fn listening() -> Vec<(&'static str, f64)> {
    vec![("mix", 1.0), ("dream", 0.0), ("learn", 0.8), ("chaos", 0.5), ("horizon", 2.0)]
}

fn with(mut base: Vec<(&'static str, f64)>, more: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    base.extend_from_slice(more);
    base
}

#[test]
fn mix_zero_is_bit_exact_dry() {
    let input = noise(24_000, 0.5);
    let run = render(&[("mix", 0.0), ("chaos", 1.0), ("dream", 1.0), ("learn", 1.0)], &[], &input);
    for (i, x) in input.iter().enumerate() {
        assert!(x.to_bits() == run.out[i].to_bits(), "sample {i}");
    }
}

#[test]
fn a_fresh_brain_is_silent() {
    let quiet = vec![0.0f32; 96_000];
    let run = render(&with(listening(), &[("chaos", 1.0), ("dream", 1.0)]), &[], &quiet);
    assert!(run.out.iter().all(|x| *x == 0.0), "a fresh brain made sound from silence");
}

#[test]
fn it_learns_to_predict_you() {
    let n = (FS * 6.0) as usize;
    let input = phrase(n, 220.0);
    let run = render(&listening(), &[], &input);
    // HORIZON 2 ms = 96 host samples ahead. The error of its guess,
    // relative to what you played: it starts silent (error 100%) and
    // must close in on you.
    let lag = 96;
    let err = |from: usize, len: usize| {
        let e: Vec<f32> = (from..from + len).map(|t| run.out[t] - input[t + lag]).collect();
        rms(&e) / rms(&input[from + lag..from + lag + len])
    };
    let early = err(0, 2_400);
    let late = err((FS * 5.0) as usize, 24_000);
    let r = tracks(&run.out, &input, (FS * 5.0) as usize, 24_000, lag);
    assert!(r > 0.9, "after 5 s it predicts the input {lag} samples ahead with r = {r:.2}");
    assert!(late < 0.35, "late prediction error {:.0}%", late * 100.0);
    assert!(late < 0.5 * early, "no learning: error {:.0}% early vs {:.0}% late", early * 100.0, late * 100.0);
}

#[test]
fn learn_zero_never_learns() {
    let input = phrase((FS * 3.0) as usize, 220.0);
    let run = render(&with(listening(), &[("learn", 0.0)]), &[], &input);
    let level = rms(&run.out[(FS * 2.0) as usize..]);
    assert!(level < 1e-6, "LEARN 0 learned anyway (rms {level})");
}

#[test]
fn it_keeps_playing_when_you_stop() {
    let n = (FS * 8.0) as usize;
    let mut input = phrase(n, 220.0);
    input[(FS * 5.0) as usize..].fill(0.0);
    let calm = with(listening(), &[("learn", 0.6), ("memory", 0.8), ("chaos", 0.35)]);
    let run = render(&calm, &[(5.0, "sleep", 1.0)], &input);
    let dream = &run.out[(FS * 6.0) as usize..(FS * 7.0) as usize];
    let level = rms(dream);
    assert!(level > 0.02, "asleep, it fell silent (rms {level})");
    let hz = peak_hz(&dream[..16_384]);
    assert!((hz - 220.0).abs() < 220.0 * 0.06, "its dream sits at {hz:.0} Hz, not near the learned 220");
}

#[test]
fn chaos_past_the_edge_rings_on_its_own() {
    let mut input = noise((FS * 4.0) as usize, 0.5);
    input[(FS * 0.5) as usize..].fill(0.0);
    let after = |chaos: f64| {
        let run = render(&with(listening(), &[("chaos", chaos), ("memory", 0.3)]), &[], &input);
        let k = (FS * 3.5) as usize / BLOCK;
        run.activity[k]
    };
    let (calm, wild) = (after(0.2), after(1.0));
    assert!(calm < 1e-3, "below the edge the network should settle (activity {calm})");
    assert!(wild > 0.02, "past the edge the network should ring (activity {wild})");
}

#[test]
fn it_lets_go_when_you_stop_unless_asleep() {
    // A phrase, then 6 s of −70 dBFS interface hiss.
    let mut input = phrase((FS * 3.0) as usize, 220.0);
    input.extend(noise((FS * 6.0) as usize, 0.000_55));
    let wild = [("chaos", 1.0), ("dream", 0.6), ("memory", 0.8)];
    let awake = render(&with(listening(), &wild), &[], &input);
    let tail = rms(&awake.out[(FS * 6.5) as usize..(FS * 8.5) as usize]);
    assert!(tail < 3e-5, "awake, it hummed on after you stopped ({:.1} dBFS)", 20.0 * tail.log10());
    let asleep = render(&with(listening(), &wild), &[(3.0, "sleep", 1.0)], &input);
    let dream = rms(&asleep.out[(FS * 6.5) as usize..(FS * 8.5) as usize]);
    assert!(dream > 0.01, "asleep, it should play on (rms {dream})");
}

#[test]
fn a_wild_brain_stays_bounded() {
    let input = noise((FS * 5.0) as usize, 0.6);
    let run = render(&[("mix", 1.0), ("chaos", 1.0), ("dream", 1.0), ("learn", 1.0), ("horizon", 25.0)], &[], &input);
    let peak = run.out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(run.out.iter().all(|x| x.is_finite()) && peak < 3.0, "peak {peak}");
}

/// The dream-fidelity map (ignored; prints): where the generator's
/// free-running pitch lands across LEARN × MEMORY × CHAOS × HORIZON.
#[test]
#[ignore]
fn sweep_dream() {
    let n = (FS * 8.0) as usize;
    let mut input = phrase(n, 220.0);
    input[(FS * 5.0) as usize..].fill(0.0);
    for &learn in &[0.4, 0.7] {
        for &memory in &[0.2, 0.5, 0.8] {
            for &chaos in &[0.2, 0.35, 0.5] {
                for &horizon in &[0.25, 2.0] {
                    let set = with(listening(), &[("learn", learn), ("memory", memory), ("chaos", chaos), ("horizon", horizon)]);
                    let run = render(&set, &[(5.0, "sleep", 1.0)], &input);
                    let d = &run.out[(FS * 6.0) as usize..(FS * 7.0) as usize];
                    println!("learn {learn} mem {memory} chaos {chaos} hor {horizon}: {:.0} Hz rms {:.3}", peak_hz(&d[..16_384]), rms(d));
                }
            }
        }
    }
}
