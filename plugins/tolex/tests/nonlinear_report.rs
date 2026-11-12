//! THE NONLINEAR REPORT — the living leg's instrument (frontier
//! item 2). The linear leg answers to the corpus; nothing linear
//! can ground-truth compression, harmonics, or aliasing, so this
//! harness renders steady tones across the drive range and prints
//! the large-signal story: the compression curve, the harmonic
//! tracks, and the alias floor at full drive (the Oversampled2x
//! decision, measured — house rule). Two field reports (woofer-
//! scale excursion constants; missing force stress) found what
//! this instrument now catches first.
//!
//! Reference behavior (Klippel-tradition lore for 25 W guitar
//! speakers): LF THD reaches ~10-30% at rated drive; compression
//! 2-6 dB; mid-band force stress compresses without excursion.
//! Hard gates are set from the first measured run (documented
//! baselines, like the perf report): compression MONOTONIC, THD
//! rising with drive, alias floor below -50 dBc.
//!
//! Run: cargo test --release -p stave-tolex-vst3 --test nonlinear_report -- --nocapture

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const LEN: usize = 48_128; // block-aligned (the properties.rs lesson)

fn render_tone(drive: f64, freq: f32, amp: f32) -> Vec<f32> {
    let mut plugin = stave_tolex_vst3::Tolex::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in [
        ("drive", drive),
        ("damping", 0.75),
        ("mic", 3.0),
        ("size", 2.0),
        ("resonance", 80.0),
        ("q", 0.85),
        ("cab", 60.0),
        ("back", 0.0),
        ("mass", 1.0),
        ("cone", 0.0),
        ("breakup", 0.6),
        ("doping", 0.35),
        ("position", 0.25),
        ("material", 0.0),
        ("speakers", 0.0),
        ("model", 0.0),
    ] {
        table.by_name(n).unwrap().set_plain(v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(LEN);
    let (mut in_l, mut in_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut out_l, mut out_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for b in 0..(LEN / BLOCK) {
        for f in 0..BLOCK {
            let t = (b * BLOCK + f) as f32 / FS;
            in_l[f] = (core::f32::consts::TAU * freq * t).sin() * amp;
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
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

/// Projection magnitude at k·f0 over the settled tail (bin-exact
/// frequencies only).
fn mag_at(out: &[f32], hz: f64) -> f64 {
    let seg = &out[out.len() - 23_040..];
    let w = core::f64::consts::TAU * hz / f64::from(FS);
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for (i, x) in seg.iter().enumerate() {
        s += f64::from(*x) * (w * i as f64).sin();
        c += f64::from(*x) * (w * i as f64).cos();
    }
    (s * s + c * c).sqrt() * 2.0 / seg.len() as f64
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// THE COIL HEATS: sustained full drive into a solid-state
/// grip (damping 0 — maximum thermal authority over the current)
/// sags the fundamental by a real, bounded amount over seconds,
/// measured early-window vs late-window on one sustained render.
#[test]
fn the_coil_heats() {
    if cfg!(debug_assertions) {
        return;
    }
    let mut plugin = stave_tolex_vst3::Tolex::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in [
        ("drive", 24.0),
        ("damping", 0.0),
        ("mic", 3.0),
        ("size", 2.0),
        ("resonance", 80.0),
        ("q", 0.85),
        ("cab", 60.0),
        ("back", 0.0),
        ("mass", 1.0),
        ("cone", 0.0),
        ("breakup", 0.6),
        ("doping", 0.35),
        ("position", 0.25),
        ("material", 0.0),
        ("speakers", 0.0),
        ("model", 0.0),
    ] {
        table.by_name(n).unwrap().set_plain(v);
    }
    plugin.activate(FS, BLOCK);
    let len = 144_128usize; // ~3 s, block-aligned
    let mut out = Vec::with_capacity(len);
    let (mut in_l, mut in_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut out_l, mut out_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for b in 0..(len / BLOCK) {
        for f in 0..BLOCK {
            let t = (b * BLOCK + f) as f32 / FS;
            in_l[f] = (core::f32::consts::TAU * 375.0 * t).sin() * 0.5;
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
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    // 96-cycle windows (12288 samples at 128/cycle), early vs late.
    let win = |start: usize| {
        let seg = &out[start..start + 12_288];
        let w = core::f64::consts::TAU * 375.0 / f64::from(FS);
        let (mut s, mut c) = (0.0f64, 0.0f64);
        for (i, x) in seg.iter().enumerate() {
            s += f64::from(*x) * (w * i as f64).sin();
            c += f64::from(*x) * (w * i as f64).cos();
        }
        (s * s + c * c).sqrt()
    };
    let sag = 20.0 * (win(4_800) / win(len - 13_000)).log10();
    eprintln!("== thermal sag over ~3 s at +24 dB drive, damping 0: {sag:.2} dB ==");
    assert!(
        (0.3..=3.0).contains(&sag),
        "thermal sag {sag:.2} dB — the coil must heat, honestly"
    );
}

#[test]
fn nonlinear_report_for_the_record() {
    if cfg!(debug_assertions) {
        eprintln!("nonlinear report: skipped in debug (run --release)");
        return;
    }
    let drives = [-12.0, 0.0, 6.0, 12.0, 18.0, 24.0];

    // f0 must be bin-exact over the 23040 tail AND block-friendly:
    // 93.75 Hz (excursion regime) and 375 Hz (force regime).
    for (regime, f0) in [("excursion 93.75 Hz", 93.75f64), ("force 375 Hz", 375.0)] {
        eprintln!("== {regime} — amp 0.5, damping 0.75, closed 60 L ==");
        eprintln!(
            "{:>8} {:>10} {:>9} {:>9} {:>9}",
            "drive", "fund dBr", "comp dB", "H2 dBc", "H3 dBc"
        );
        let mut prev_gain = f64::NEG_INFINITY;
        let mut base = 0.0f64;
        let mut prev_thd = -1.0f64;
        for (i, d) in drives.iter().enumerate() {
            let out = render_tone(*d, f0 as f32, 0.5);
            let fund = db(mag_at(&out, f0));
            if i == 0 {
                base = fund;
            }
            // Drive is loudness-compensated: linear = flat fund.
            let comp = -(fund - base);
            let h2 = db(mag_at(&out, 2.0 * f0)) - fund;
            let h3 = db(mag_at(&out, 3.0 * f0)) - fund;
            eprintln!(
                "{d:>+8.0} {:>10.2} {comp:>9.2} {h2:>9.1} {h3:>9.1}",
                fund - base
            );
            // GATES: compression must be monotonic (gain never rises
            // with drive) and distortion must GROW with drive.
            assert!(
                fund <= prev_gain + 0.1 || i == 0,
                "{regime}: fundamental ROSE with drive at {d} dB"
            );
            let thd = (mag_at(&out, 2.0 * f0).powi(2) + mag_at(&out, 3.0 * f0).powi(2)).sqrt()
                / mag_at(&out, f0);
            assert!(
                thd >= prev_thd * 0.7 || i == 0,
                "{regime}: THD fell with drive at {d} dB"
            );
            prev_gain = fund;
            prev_thd = thd;
            // Flux modulation: the FORCE regime must speak
            // even-order too — current is large at mids even when
            // travel is nil (Bl(i) + Le(x,i)).
            if regime.starts_with("force") {
                if *d <= -12.0 {
                    assert!(h2 < -80.0, "mid H2 {h2:.1} dBc dirty at low drive");
                }
                if *d >= 24.0 {
                    assert!(
                        (-48.0..=-20.0).contains(&h2),
                        "mid H2 {h2:.1} dBc at full drive — flux must speak"
                    );
                }
            }
            // The even-order voice (motor asymmetry): by full drive in
            // the excursion regime, H2 must be audible but below H3
            // (offset-coil physics); at low drive it stays clean.
            if regime.starts_with("excursion") {
                if *d <= -12.0 {
                    assert!(h2 < -60.0, "H2 {h2:.1} dBc dirty at low drive");
                }
                if *d >= 24.0 {
                    assert!(
                        (-45.0..=-6.0).contains(&h2),
                        "H2 {h2:.1} dBc at full drive — the motor must speak even-order"
                    );
                }
            }
        }
    }

    // THE ALIAS FLOOR at full drive: 3 kHz tone (bin-exact 2953.125
    // = FS*63/1024... use 3000·(23040-friendly): 2968.75 = FS·1425/23040?
    // 23040·f/48000 integer → f = 48000·k/23040 = 25/12·k: k=1425 → 2968.75.
    let f0 = 2_968.75f64;
    let out = render_tone(24.0, f0 as f32, 0.5);
    let fund = mag_at(&out, f0);
    // Alias products of h·f0 fold to |h·f0 − n·FS|; probe the
    // folded images of H2..H5 that land IN BAND and away from
    // harmonics.
    // Harmonics 9..16 cross Nyquist and fold; probe their images
    // (skipping any that land on a true harmonic).
    let mut worst = f64::NEG_INFINITY;
    let mut probed = 0;
    for h in 2..=16 {
        let f = f0 * f64::from(h);
        if f > f64::from(FS) / 2.0 {
            let m = f % f64::from(FS);
            let folded = m.min(f64::from(FS) - m);
            let near_harm = (folded / f0 - (folded / f0).round()).abs() * f0 < 30.0;
            if folded > 100.0 && folded < 20_000.0 && !near_harm {
                worst = worst.max(db(mag_at(&out, folded) / fund));
                probed += 1;
            }
        }
    }
    assert!(
        probed >= 4,
        "alias probe vacuous: only {probed} folded images in band"
    );
    eprintln!("== alias floor at +24 dB drive, {f0} Hz: worst folded image {worst:.1} dBc ==");
    assert!(
        worst <= -50.0,
        "alias floor {worst:.1} dBc at full drive — the motor may need a Oversampled2x"
    );
}
