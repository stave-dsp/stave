//! Forge 2 variety conformance — the "broad range of distortion types"
//! claim, measured. Each selector position must be DISTINCT on the axis
//! it names:
//!
//! * CHARACTER — distinct harmonic ladders (even/odd balance, falloff).
//! * VOICE — distinct G(f) placements (scoop vs bump vs bright vs dark).
//! * STACK — distinct topologies (cascade adds a stage; push-pull
//!   suppresses the evens the same bias creates in cascade; screamer is
//!   its own animal).
//! * MEMORY — distinct program-dependent gain trajectories.

use std::sync::Mutex;

use stave_core::{DebugBus, Plugin};
use stave_forge_vst3::Forge;
use stave_test::PluginHarness;

const FS: f64 = 48_000.0;
const BLOCK: usize = 256;
const N_FFT: usize = 32_768;

fn snap(freq: f64) -> f64 {
    (freq * N_FFT as f64 / FS).round() * FS / N_FFT as f64
}

fn goertzel(buf: &[f32], f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / FS;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in buf {
        let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    ((s1 - s2 * w.cos()).powi(2) + (s2 * w.sin()).powi(2)).sqrt() / (buf.len() as f64 * 0.5)
}

struct Rig {
    h: PluginHarness<Forge>,
    table: std::sync::Arc<dyn stave_core::params::Params>,
}

/// Params are set through Forge's typed table. LOCK stays: the suites are
/// CPU-heavy and serializing keeps timings comparable.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Neutral baseline: everything off except the axis under test.
const BASE: &[(&str, f64)] = &[
    ("drive", 5.0),
    ("character", 1.0),
    ("edge", 0.4),
    ("bias", 0.0),
    ("stack", 0.0),
    ("filter", 0.0),
    ("post filter", 0.0),
    ("tone", 0.0),
    ("memory", 0.0),
    ("depth", 0.0),
    ("grit", 0.0),
    ("iron", 0.0),
    ("width", 0.5),
    ("input", 0.0),
    ("mix", 1.0),
    ("output", 0.0),
    ("bypass", 0.0),
];

fn rig(overrides: &[(&str, f64)]) -> Rig {
    let mut h = PluginHarness::<Forge>::new(FS as f32, BLOCK);
    h.plugin_mut().debug_attach(DebugBus::new());
    let table = h.plugin_mut().param_table();
    let r = Rig { h, table };
    for &(k, v) in BASE {
        r.set(k, v);
    }
    for &(k, v) in overrides {
        r.set(k, v);
    }
    r
}

impl Rig {
    fn set(&self, name: &str, v: f64) {
        let p = self
            .table
            .by_name(name)
            .unwrap_or_else(|| panic!("param '{name}' unknown"));
        p.set_plain(v);
    }

    /// Drive a dual-mono sine (mid path only) and return the LAST
    /// `N_FFT` samples of the left output, past latency and settle.
    fn render_tone(&mut self, freq: f64, amp: f64, seconds: f64) -> Vec<f32> {
        let total = (FS * seconds) as usize / BLOCK * BLOCK;
        let mut out = Vec::with_capacity(total);
        let mut buf = vec![0.0f32; BLOCK];
        for b in 0..(total / BLOCK) {
            for i in 0..BLOCK {
                let t = (b * BLOCK + i) as f64;
                buf[i] = (amp * (2.0 * std::f64::consts::PI * freq * t / FS).sin()) as f32;
            }
            self.h.set_input(0, &buf);
            self.h.set_input(1, &buf);
            let blocks = self.h.process_block(&[]);
            out.extend_from_slice(&blocks[0]);
        }
        out.split_off(out.len() - N_FFT.min(out.len()))
    }

    /// Amplitude-step program: `quiet` then `hot`, returning the whole
    /// left output (for gain-trajectory measures).
    fn render_step(
        &mut self,
        freq: f64,
        quiet: f64,
        hot: f64,
        t_quiet: f64,
        t_hot: f64,
    ) -> (Vec<f32>, usize) {
        let n_quiet = (FS * t_quiet) as usize / BLOCK * BLOCK;
        let n_hot = (FS * t_hot) as usize / BLOCK * BLOCK;
        let mut out = Vec::with_capacity(n_quiet + n_hot);
        let mut buf = vec![0.0f32; BLOCK];
        for b in 0..((n_quiet + n_hot) / BLOCK) {
            for i in 0..BLOCK {
                let t = (b * BLOCK + i) as f64;
                let amp = if (b * BLOCK + i) < n_quiet {
                    quiet
                } else {
                    hot
                };
                buf[i] = (amp * (2.0 * std::f64::consts::PI * freq * t / FS).sin()) as f32;
            }
            self.h.set_input(0, &buf);
            self.h.set_input(1, &buf);
            let blocks = self.h.process_block(&[]);
            out.extend_from_slice(&blocks[0]);
        }
        (out, n_quiet)
    }
}

fn rms_db(buf: &[f32]) -> f64 {
    let p = buf.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / buf.len() as f64;
    10.0 * p.max(1e-24).log10()
}

/// Harmonic amplitudes h1..h8 of a tone rendering.
fn harmonics(out: &[f32], f0: f64) -> [f64; 8] {
    let mut h = [0.0f64; 8];
    for (k, hk) in h.iter_mut().enumerate() {
        let f = f0 * (k + 1) as f64;
        if f < FS / 2.0 * 0.95 {
            *hk = goertzel(out, f);
        }
    }
    h
}

// ---------------------------------------------------------------------------

#[test]
fn characters_have_distinct_harmonic_signatures() {
    let _g = lock();
    let f0 = snap(220.0);
    let names = ["SILK", "TRIODE", "CLASS AB", "TAPE", "DIODE", "FUZZ"];
    let mut sigs: Vec<[f64; 8]> = vec![];
    for ch in 0..6 {
        // Moderate drive — the recipe region. (At max drive all bounded
        // shapers converge toward the clip ladder; the per-character
        // saturation speeds keep them apart on the way there.)
        let mut r = rig(&[("character", ch as f64), ("drive", 2.5)]);
        let out = r.render_tone(f0, 0.45, 1.2);
        let h = harmonics(&out, f0);
        println!(
            "{:<9} h2/h1 {:.4}  h3/h1 {:.4}  h5/h1 {:.4}  h7/h1 {:.4}  (h5/h3 {:.3})",
            names[ch],
            h[1] / h[0],
            h[2] / h[0],
            h[4] / h[0],
            h[6] / h[0],
            h[4] / h[2].max(1e-12),
        );
        sigs.push(h);
    }
    let rel = |s: &[f64; 8], k: usize| s[k] / s[0].max(1e-12);
    // TRIODE is even-dominant; SILK is odd-only.
    assert!(
        rel(&sigs[1], 1) > rel(&sigs[1], 2),
        "TRIODE should lead with H2"
    );
    assert!(
        rel(&sigs[0], 1) < 0.15 * rel(&sigs[0], 2).max(1e-6) + 1e-4,
        "SILK should have no even content: h2/h1 {:.5}",
        rel(&sigs[0], 1)
    );
    // CLASS AB's ladder falls off slower than SILK's.
    let falloff = |s: &[f64; 8]| s[4] / s[2].max(1e-12); // h5/h3
    assert!(
        falloff(&sigs[2]) > 2.0 * falloff(&sigs[0]),
        "CLASS AB should decay slower than SILK: {:.3} vs {:.3}",
        falloff(&sigs[2]),
        falloff(&sigs[0])
    );
    // DIODE at moderate drive: the steepest ladder (hard knee below the
    // corner ≈ compression without high orders)…
    assert!(
        falloff(&sigs[4]) < 0.6 * falloff(&sigs[0]),
        "DIODE should have the steepest moderate-drive ladder: {:.3} vs SILK {:.3}",
        falloff(&sigs[4]),
        falloff(&sigs[0])
    );
    // …and the fastest escalation once pushed (the knee corner).
    let h3_at = |drive: f64| {
        let mut r = rig(&[("character", 4.0), ("drive", drive)]);
        let out = r.render_tone(f0, 0.45, 1.2);
        let h = harmonics(&out, f0);
        h[2] / h[0].max(1e-12)
    };
    let escalation = h3_at(7.0) / h3_at(2.5).max(1e-9);
    println!("DIODE h3 escalation 2.5→7.0 drive: {escalation:.2}×");
    assert!(
        escalation > 1.5,
        "DIODE should escalate hard with drive: {escalation:.2}×"
    );
    // FUZZ carries even bite.
    assert!(
        rel(&sigs[5], 1) > 0.02,
        "FUZZ needs H2 bite: {:.4}",
        rel(&sigs[5], 1)
    );
    // Global pairwise distinctness over the normalized signature vector.
    let mut min_d = f64::MAX;
    let mut pair = (0, 0);
    for a in 0..6 {
        for b in (a + 1)..6 {
            let d: f64 = (1..8)
                .map(|k| (rel(&sigs[a], k) - rel(&sigs[b], k)).abs())
                .sum();
            if d < min_d {
                min_d = d;
                pair = (a, b);
            }
        }
    }
    println!(
        "min pairwise signature distance {min_d:.4} ({} vs {})",
        names[pair.0], names[pair.1]
    );
    assert!(
        min_d > 0.02,
        "two characters are near-identical: {min_d:.4}"
    );
}

#[test]
fn voices_place_energy_differently() {
    let _g = lock();
    let freqs: Vec<f64> = [200.0, 900.0, 1_400.0, 3_000.0, 6_000.0]
        .iter()
        .map(|&f| snap(f))
        .collect();
    let names = ["FLAT", "SCOOP", "BUMP", "BRIGHT", "DARK", "HONK"];
    // Profile = fundamental gain at f, relative to the 200 Hz anchor —
    // measured THROUGH the full nonlinear chain, which is the point.
    let mut profiles: Vec<Vec<f64>> = vec![];
    for v in 0..6 {
        let mut p = vec![];
        for &f in &freqs {
            let mut r = rig(&[("stack", 1.0), ("filter", v as f64)]);
            let out = r.render_tone(f, 0.35, 0.8);
            p.push(20.0 * (goertzel(&out, f) / 0.35).log10());
        }
        let anchor = p[0];
        let rel: Vec<f64> = p.iter().map(|g| g - anchor).collect();
        println!(
            "{:<7} rel gains @900/1.4k/3k/6k: {:+.2} {:+.2} {:+.2} {:+.2} dB",
            names[v], rel[1], rel[2], rel[3], rel[4]
        );
        profiles.push(rel);
    }
    let flat = profiles[0].clone();
    assert!(
        profiles[1][1] < flat[1] - 3.0,
        "SCOOP should dip the mids vs FLAT"
    );
    assert!(
        profiles[2][1] > flat[1] + 1.5,
        "BUMP should push the mids vs FLAT"
    );
    assert!(
        profiles[3][4] > flat[4] + 1.8,
        "BRIGHT should lift the top vs FLAT"
    );
    // Note: the interstage lowpass loses part of its cut to stage-2
    // compression (quieter input → less gain reduction) — the measured
    // −2.5 dB IS the G(f, level) bend at work.
    assert!(
        profiles[4][4] < flat[4] - 2.0,
        "DARK should cut the top vs FLAT"
    );
    assert!(
        profiles[5][2] > flat[2] + 2.5,
        "HONK should peak at 1.4 kHz vs FLAT"
    );
}

#[test]
fn stacks_are_distinct_topologies() {
    let _g = lock();
    let f0 = snap(220.0);
    // Same character/bias for all stacks.
    let h_for = |stack: f64| -> [f64; 8] {
        let mut r = rig(&[("character", 1.0), ("bias", 0.4), ("stack", stack)]);
        let out = r.render_tone(f0, 0.5, 1.2);
        harmonics(&out, f0)
    };
    let single = h_for(0.0);
    let cascade = h_for(1.0);
    let screamer = h_for(2.0);
    let pushpull = h_for(3.0);
    let thd = |h: &[f64; 8]| (h[1..].iter().map(|a| a * a).sum::<f64>()).sqrt() / h[0].max(1e-12);
    let even_ratio = |h: &[f64; 8]| h[1] / h[2].max(1e-12);
    println!(
        "THD: single {:.4} cascade {:.4} screamer {:.4} push-pull {:.4}",
        thd(&single),
        thd(&cascade),
        thd(&screamer),
        thd(&pushpull)
    );
    println!(
        "H2/H3: cascade {:.3} push-pull {:.3}",
        even_ratio(&cascade),
        even_ratio(&pushpull)
    );
    // A second clipper adds harmonic energy.
    assert!(
        thd(&cascade) > thd(&single) * 1.15,
        "cascade should out-distort single: {:.4} vs {:.4}",
        thd(&cascade),
        thd(&single)
    );
    // Push-pull's opposed bias suppresses the evens cascade creates
    // from the SAME bias knob.
    assert!(
        even_ratio(&pushpull) < 0.6 * even_ratio(&cascade),
        "push-pull should suppress evens: {:.3} vs cascade {:.3}",
        even_ratio(&pushpull),
        even_ratio(&cascade)
    );
    // Screamer is its own topology — signature far from cascade.
    let d: f64 = (1..8)
        .map(|k| (screamer[k] / screamer[0] - cascade[k] / cascade[0]).abs())
        .sum();
    println!("screamer vs cascade signature distance {d:.4}");
    assert!(d > 0.03, "screamer should not just be a cascade: {d:.4}");
}

#[test]
fn memory_modes_shape_gain_over_time() {
    let _g = lock();
    let f0 = snap(220.0);
    let names = ["NONE", "SAG", "BLOCKING", "SQUEEZE"];
    // Settled GIVE: hot-passage steady-state level at depth 0.8 relative
    // to depth 0 — window-timing-insensitive (envelope attacks are fast
    // next to the settle window).
    let settled = |m: f64, d: f64| -> f64 {
        let mut r = rig(&[
            ("character", 1.0),
            ("stack", 1.0),
            ("drive", 6.0),
            ("memory", m),
            ("depth", d),
        ]);
        let (out, n_quiet) = r.render_step(f0, 0.05, 0.7, 0.3, 0.7);
        let start = n_quiet + (FS * 0.5) as usize;
        rms_db(&out[start..start + 4_800])
    };
    let mut gives = vec![];
    for m in 0..4 {
        let give = settled(m as f64, 0.8) - settled(m as f64, 0.0);
        println!("{:<9} settled give @220 {:.2} dB", names[m], give);
        gives.push(give);
    }
    assert!(
        gives[0].abs() < 0.1,
        "NONE must be static: {:.2} dB",
        gives[0]
    );
    assert!(
        gives[1] < -0.4,
        "SAG should give under sustain: {:.2} dB",
        gives[1]
    );
    assert!(
        gives[3] < -0.7,
        "SQUEEZE should compress: {:.2} dB",
        gives[3]
    );

    // SAG darkens (supply tilt) where SQUEEZE stays spectrally flat —
    // the distinction is the TILT under heat, not the level give.
    let f_hi = snap(3_000.0);
    let settled_hi = |m: f64, d: f64| -> f64 {
        let mut r = rig(&[
            ("character", 1.0),
            ("stack", 1.0),
            ("drive", 6.0),
            ("memory", m),
            ("depth", d),
        ]);
        let (out, n_quiet) = r.render_step(f_hi, 0.05, 0.7, 0.3, 0.7);
        let start = n_quiet + (FS * 0.5) as usize;
        rms_db(&out[start..start + 4_800])
    };
    let tilt_sag = (settled_hi(1.0, 0.8) - settled_hi(1.0, 0.0)) - gives[1];
    let tilt_squeeze = (settled_hi(3.0, 0.8) - settled_hi(3.0, 0.0)) - gives[3];
    println!("tilt under heat: SAG {tilt_sag:.2} dB, SQUEEZE {tilt_squeeze:.2} dB");
    assert!(
        tilt_sag < tilt_squeeze - 1.0,
        "SAG should darken vs SQUEEZE: {tilt_sag:.2} vs {tilt_squeeze:.2}"
    );

    // BLOCKING chokes the BIAS — on a symmetric curve its fingerprint is
    // even-harmonic generation under sustained heat (grid-charge asym),
    // with clean recovery, not a level drop.
    let h2_of = |m: f64| -> f64 {
        let mut r = rig(&[
            ("character", 0.0), // SILK: no even content of its own
            ("stack", 1.0),
            ("drive", 6.0),
            ("memory", m),
            ("depth", 0.8),
        ]);
        let out = r.render_tone(f0, 0.7, 1.2);
        let h = harmonics(&out, f0);
        h[1] / h[0].max(1e-12)
    };
    let h2_none = h2_of(0.0);
    let h2_blocking = h2_of(2.0);
    println!("hot H2/H1: NONE {h2_none:.4}, BLOCKING {h2_blocking:.4}");
    assert!(h2_none < 0.005, "NONE should stay symmetric: {h2_none:.4}");
    assert!(
        h2_blocking > 10.0 * h2_none.max(1e-4),
        "BLOCKING should generate even content from grid charge: {h2_blocking:.4}"
    );
}

#[test]
fn post_filters_are_explicit_eq_moves() {
    let _g = lock();
    // Measured through the full nonlinear chain at the default stack.
    let gain_at = |post: f64, freq: f64| -> f64 {
        let mut r = rig(&[("stack", 1.0), ("post filter", post)]);
        let f = snap(freq);
        let out = r.render_tone(f, 0.35, 0.8);
        20.0 * (goertzel(&out, f) / 0.35).log10()
    };
    // Probe each move where its corner actually acts: the 90 Hz
    // highpass is FLAT at 200 Hz and the resonant 5.5 kHz lowpass is
    // still unity at 6 kHz (both measured — wrong-frequency bars pass
    // vacuously or fail spuriously).
    let flat_lo = gain_at(0.0, 60.0);
    let flat_hi6 = gain_at(0.0, 6_000.0);
    let flat_hi10 = gain_at(0.0, 10_000.0);
    let air = gain_at(1.0, 6_000.0) - flat_hi6; // AIR lifts the top
    let lowcut = gain_at(2.0, 60.0) - flat_lo; // LOW CUT clears the sub
    let warm = gain_at(3.0, 60.0) - flat_lo; // WARM blooms the lows
    let hicut = gain_at(4.0, 10_000.0) - flat_hi10; // HI CUT rolls the top
    println!(
        "post rel: AIR@6k {air:+.2}  LOWCUT@200 {lowcut:+.2}  WARM@200 {warm:+.2}  HICUT@6k {hicut:+.2} dB"
    );
    assert!(air > 1.5, "AIR should lift the top: {air:+.2}");
    assert!(lowcut < -3.0, "LOW CUT should clear the lows: {lowcut:+.2}");
    assert!(warm > 1.5, "WARM should bloom the lows: {warm:+.2}");
    assert!(hicut < -3.0, "HI CUT should roll the top: {hicut:+.2}");
}

/// RIPPLE memory: env-scaled 120 Hz supply wobble → ghost sidebands at
/// n·f0 ± 120 Hz that ride program heat.
#[test]
fn ripple_memory_makes_ghost_sidebands() {
    let _g = lock();
    let f0 = snap(220.0);
    let m = (f0 * N_FFT as f64 / FS).round() as usize;
    let render = |memory: f64| -> Vec<f64> {
        let mut r = rig(&[
            ("stack", 1.0),
            ("drive", 6.0),
            ("memory", memory),
            ("depth", 0.8),
        ]);
        r.render_tone(f0, 0.5, 1.2)
            .iter()
            .map(|&x| x as f64)
            .collect()
    };
    // 120 Hz ≈ 81.9 bins — not bin-centred, so the lines spread; a wide
    // band around H1 catches them.
    let none = stave_measure::modulation::sideband_report(
        &render(0.0),
        &stave_measure::modulation::SidebandConfig {
            carrier_bin: m,
            skirt_bins: 120,
            harmonics: 1,
        },
    );
    let ripple = stave_measure::modulation::sideband_report(
        &render(4.0),
        &stave_measure::modulation::SidebandConfig {
            carrier_bin: m,
            skirt_bins: 120,
            harmonics: 1,
        },
    );
    println!(
        "ghost sidebands: NONE {:.1} dB, RIPPLE {:.1} dB (worst {:?})",
        none.total_db, ripple.total_db, ripple.worst
    );
    assert!(
        ripple.total_db > none.total_db + 15.0,
        "RIPPLE should grow ghost sidebands: {:.1} vs {:.1}",
        ripple.total_db,
        none.total_db
    );
    // The true multiplicative pair sits at f0 ± 120 Hz (≈ ±82 bins);
    // measure the upper sideband explicitly (the worst single line can
    // legitimately be the gated hum leak at 120 Hz absolute, from the
    // bias half of the route — real supply ripple hums too).
    let upper_db = {
        let out = render(4.0);
        let n = out.len();
        let mut re = out.clone();
        let mut im = vec![0.0; n];
        stave_measure::spectrum::fft(&mut re, &mut im);
        let pow = |b: usize| re[b] * re[b] + im[b] * im[b];
        let carrier = pow(m).max(1e-300);
        let lo = m + 79;
        let hi = m + 85;
        let band: f64 = (lo..=hi).map(pow).sum();
        10.0 * (band / carrier).max(1e-30).log10()
    };
    println!("upper ghost (f0 + 120 Hz) {upper_db:.1} dBc");
    assert!(
        upper_db > -45.0,
        "upper ghost sideband missing: {upper_db:.1} dBc"
    );
}

/// SUB controls: MIX populates the half-integer comb through the chain;
/// MOD does it multiplicatively (gain at f0/2 → 1.5/2.5·f0). Off = floor.
#[test]
fn sub_controls_populate_the_half_comb() {
    let _g = lock();
    // Even FFT bin so the half comb is coherent: bin 160 → 234.375 Hz.
    let f0 = snap(234.0);
    let m = (f0 * N_FFT as f64 / FS).round() as usize;
    assert_eq!(m % 2, 0);
    let half_comb = |mix: f64, mv: f64| -> f64 {
        let mut r = rig(&[
            ("stack", 1.0),
            ("drive", 5.0),
            ("sub mix", mix),
            ("sub mod", mv),
        ]);
        let out: Vec<f64> = r
            .render_tone(f0, 0.5, 1.2)
            .iter()
            .map(|&x| x as f64)
            .collect();
        stave_measure::modulation::subharmonic_ratio_db(
            &out,
            &stave_measure::modulation::SubharmonicConfig {
                carrier_bin: m,
                max_order: 8,
            },
        )
    };
    let off = half_comb(0.0, 0.0);
    let mixed = half_comb(0.6, 0.0);
    let modded = half_comb(0.0, 0.6);
    println!("half comb: off {off:.1} dBc, sub mix {mixed:.1} dBc, sub mod {modded:.1} dBc");
    assert!(
        off < -70.0,
        "sub off must stay on the integer comb: {off:.1}"
    );
    assert!(
        mixed > -15.0,
        "SUB MIX should own the half comb: {mixed:.1}"
    );
    assert!(
        modded > -35.0,
        "SUB MOD should energize the half comb: {modded:.1}"
    );
}
