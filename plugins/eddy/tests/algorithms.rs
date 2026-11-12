//! The algorithm laws — one per promise each tab makes. Every
//! expected value here is DERIVED (from the feedback arithmetic, the
//! rotation group, or FM physics), never recorded from a run.
//!
//!  CIRCUIT: heat 0 is the classic law bit for bit; the cells cage
//!           the loop (regeneration never outruns the dialed
//!           feedback); the three voices are distinct.
//!  MOTOR:   under sustained load the delay sags toward the dialed
//!           stretch; unloaded it stands at the dialed time.
//!  SHIFT:   0 is the classic law bit for bit; every repeat is
//!           transposed by the dialed interval, compounding down
//!           the train; the transposer never adds loop gain.
//!  ORBIT:   the image rotates by the dialed angle per repeat and
//!           returns in 360/θ repeats; rotation adds no energy.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

/// Render stereo; `input` feeds the left lane, `right` (or a copy of
/// left) the right. Returns both output channels.
fn render_lr(set: &[(&str, f64)], left: &[f32], right: Option<&[f32]>) -> (Vec<f32>, Vec<f32>) {
    let mut plugin = stave_eddy_vst3::Eddy::new();
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
    let (mut outs_l, mut outs_r) = (Vec::new(), Vec::new());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for (ci, chunk) in left.chunks(BLOCK).enumerate() {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
        match right {
            Some(r) => {
                let start = ci * BLOCK;
                for (j, slot) in in_r.iter_mut().enumerate() {
                    *slot = r.get(start + j).copied().unwrap_or(0.0);
                }
            }
            None => in_r.copy_from_slice(&in_l),
        }
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
        outs_l.extend_from_slice(&out_l);
        outs_r.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (outs_l, outs_r)
}

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    render_lr(set, input, None).0
}

fn energy(out: &[f32], start: usize, len: usize) -> f64 {
    out[start.min(out.len())..(start + len).min(out.len())]
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum()
}

fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut lcg: u32 = 0x1234_5678;
    (0..n)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * amp
        })
        .collect()
}

fn sine(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * core::f32::consts::PI * hz * i as f32 / FS).sin() * amp)
        .collect()
}

/// Goertzel magnitude at `hz` over a window.
fn goertzel(buf: &[f32], hz: f64) -> f64 {
    let w = 2.0 * core::f64::consts::PI * hz / f64::from(FS);
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in buf {
        let s0 = f64::from(x) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    ((s1 * s1 + s2 * s2 - coeff * s1 * s2) / (buf.len() as f64 * buf.len() as f64)).sqrt()
}

// ---------------------------------------------------------------- CIRCUIT

/// HEAT = 0 bypasses the cell: every circuit voice renders the SAME
/// bits, and they match the default (classic-law) render exactly.
#[test]
fn heat_zero_is_the_classic_algorithm_bit_for_bit() {
    let input = noise(24_000, 0.5);
    let base: &[(&str, f64)] = &[
        ("time", 50.0),
        ("feedback", 0.9),
        ("wet", 0.7),
        ("algo", 0.0),
        ("heat", 0.0),
    ];
    let classic = render(&[("time", 50.0), ("feedback", 0.9), ("wet", 0.7)], &input);
    for kind in 0..3 {
        let mut set = base.to_vec();
        set.push(("circuit", f64::from(kind)));
        let out = render(&set, &input);
        for (i, (a, b)) in classic.iter().zip(out.iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "circuit {kind} at heat 0 moved a bit at sample {i}"
            );
        }
    }
}

/// The cage: at full HEAT every voice is small-signal unity and
/// compressive above, so the repeat PEAK never outruns the dialed
/// feedback. Peak, not energy, is the caged quantity — the stateful
/// voices REDISTRIBUTE energy inside the train (the screamer's cap
/// returns charge between repeats; the tape slew spreads a spike
/// wider each pass, so a segment's energy can transiently grow ~2×
/// while its peak falls — both measured), but the peak is what an
/// unstable loop blows up, and it stays ≤ fb every step — re-pinned
/// after the write-head + loop-filter topology (cut-only filters
/// can't add loop gain, so the cage survives the flavor).
#[test]
fn the_circuits_cage_the_loop() {
    let mut input = vec![0.0f32; 96_000];
    input[0] = 1.0;
    let n = (0.100 * FS) as usize;
    for kind in 0..3 {
        let out = render(
            &[
                ("time", 100.0),
                ("feedback", 0.9),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 0.0),
                ("circuit", f64::from(kind)),
                ("heat", 1.0),
            ],
            &input,
        );
        let pk = |k: usize| -> f32 {
            out[k * n - 200..k * n + 600]
                .iter()
                .fold(0.0f32, |a, b| a.max(b.abs()))
        };
        let peaks: Vec<f32> = (1..=8).map(pk).collect();
        assert!(
            peaks[0] > 1e-3,
            "circuit {kind}: first repeat inaudible ({:.2e})",
            peaks[0]
        );
        for k in 0..7 {
            let ratio = peaks[k + 1] / peaks[k].max(1e-30);
            assert!(
                ratio <= 0.9 + 0.01,
                "circuit {kind}: repeat {}→{} peak ratio {ratio:.3} outran fb=0.9",
                k + 1,
                k + 2
            );
        }
        // And the train is dying, not treading water: eight repeats
        // in, the peak has lost at least half its start.
        assert!(
            peaks[7] < 0.5 * peaks[0],
            "circuit {kind}: repeat 8 peak {:.3} vs repeat 1 {:.3} — not decaying",
            peaks[7],
            peaks[0]
        );
    }
}

/// Three voices, three sounds: at working heat the pairwise
/// difference between the circuits' echo trains is structural, not
/// rounding.
#[test]
fn the_circuits_are_distinct_voices() {
    let input = sine(24_000, 440.0, 0.4);
    let outs: Vec<Vec<f32>> = (0..3)
        .map(|kind| {
            render(
                &[
                    ("time", 120.0),
                    ("feedback", 0.6),
                    ("dry", 0.0),
                    ("wet", 1.0),
                    ("algo", 0.0),
                    ("circuit", f64::from(kind)),
                    ("heat", 0.8),
                ],
                &sine_padded(&input, 96_000),
            )
        })
        .collect();
    let names = ["TUBE", "SCREAMER", "TAPE"];
    for a in 0..3 {
        for b in (a + 1)..3 {
            let diff: f64 = outs[a]
                .iter()
                .zip(&outs[b])
                .map(|(x, y)| f64::from(x - y) * f64::from(x - y))
                .sum();
            let base = energy(&outs[a], 0, outs[a].len());
            let rel = (diff / base.max(1e-30)).sqrt();
            assert!(
                rel > 0.05,
                "{} vs {}: relative difference {rel:.4} — not distinct",
                names[a],
                names[b]
            );
        }
    }
}

fn sine_padded(burst: &[f32], total: usize) -> Vec<f32> {
    let mut v = burst.to_vec();
    v.resize(total, 0.0);
    v
}

/// A 50 ms musical burst (330 Hz) — the impulse's opposite: real
/// program with a sustained envelope, for the decay/loudness laws.
fn burst(total: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; total];
    for (i, x) in v.iter_mut().enumerate().take(2_400) {
        *x = (2.0 * core::f32::consts::PI * 330.0 * i as f32 / FS).sin() * 0.6;
    }
    v
}

fn repeat_peak(out: &[f32], n: usize, k: usize) -> f32 {
    out[k * n..(k * n + 2_600).min(out.len())]
        .iter()
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

/// The trains SURVIVE max feedback at full heat — the complaint this
/// topology fixed ("4-5 repeats"): TUBE and SCREAMER still speak a
/// dozen repeats in (measured 0.26/0.06 of repeat 1; floors with
/// margin); TAPE is domain-qualified to eight — its generation loss
/// really does eat the tail faster, that IS the voice.
#[test]
fn the_train_survives_max_feedback() {
    let n = (0.100 * FS) as usize;
    let input = burst(16 * n);
    for (kind, check_k, floor) in [(0, 12, 0.03), (1, 12, 0.02), (2, 8, 0.05)] {
        let out = render(
            &[
                ("time", 100.0),
                ("feedback", 0.95),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 0.0),
                ("circuit", f64::from(kind)),
                ("heat", 1.0),
            ],
            &input,
        );
        let (p1, pk) = (repeat_peak(&out, n, 1), repeat_peak(&out, n, check_k));
        assert!(
            pk > floor * p1,
            "circuit {kind}: repeat {check_k} at {:.4} vs first {:.4} — the train died early",
            pk,
            p1
        );
    }
}

/// FEEDBACK still owns the decay at full heat (the ceiling model's
/// whole point — the rejected AGC self-oscillated at every setting):
/// twelve repeats in, fb 0.95 carries clearly more than fb 0.8.
#[test]
fn feedback_still_dials_the_decay_under_heat() {
    let n = (0.100 * FS) as usize;
    let input = burst(16 * n);
    let p12 = |fb: f64| -> f32 {
        let out = render(
            &[
                ("time", 100.0),
                ("feedback", fb),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 0.0),
                ("circuit", 0.0),
                ("heat", 1.0),
            ],
            &input,
        );
        repeat_peak(&out, n, 12)
    };
    let (lo, hi) = (p12(0.8), p12(0.95));
    assert!(
        hi > 2.0 * lo,
        "repeat 12: fb 0.95 gives {hi:.4} vs fb 0.8's {lo:.4} — the knob went dead"
    );
}

/// Destroyed is LOUD: the heat-1 first repeat stays at program
/// loudness (the out-gain payback) — neither crushed to a whisper
/// (the old −20 dB makeup) nor blown up.
#[test]
fn heat_keeps_the_repeats_loud() {
    let n = (0.100 * FS) as usize;
    let input = burst(6 * n);
    let first = |kind: f64, heat: f64| -> f32 {
        let out = render(
            &[
                ("time", 100.0),
                ("feedback", 0.5),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 0.0),
                ("circuit", kind),
                ("heat", heat),
            ],
            &input,
        );
        repeat_peak(&out, n, 1)
    };
    for kind in 0..3 {
        let clean = first(f64::from(kind), 0.0);
        let hot = first(f64::from(kind), 1.0);
        assert!(
            hot > 0.45 * clean && hot < 2.0 * clean,
            "circuit {kind}: heat-1 repeat at {hot:.3} vs clean {clean:.3} — loudness lost"
        );
    }
}

/// Generation loss is AUDIBLE: with TAPE hot, the loop filter takes
/// top end on every lap, so a repeat's high band dies faster than
/// its low band — the darkening that reads as "heavy" (the user's
/// bar for HEAT). Derived from the lowpass-in-loop physics, not from
/// a run: highs pass the filter once more per repeat.
#[test]
fn heat_darkens_the_generations() {
    let mut input = vec![0.0f32; 48_000];
    input[0] = 1.0;
    let out = render(
        &[
            ("time", 100.0),
            ("feedback", 0.9),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 0.0),
            ("circuit", 2.0),
            ("heat", 0.8),
        ],
        &input,
    );
    let n = (0.100 * FS) as usize;
    let band = |k: usize, hz: f64| -> f64 { goertzel(&out[k * n - 100..k * n + 700], hz) };
    let hi_ratio = band(2, 5_000.0) / band(1, 5_000.0).max(1e-12);
    let lo_ratio = band(2, 300.0) / band(1, 300.0).max(1e-12);
    assert!(
        hi_ratio < 0.6 * lo_ratio,
        "repeat 2/1: high band ratio {hi_ratio:.3} vs low {lo_ratio:.3} — no generation loss"
    );
}

// ---------------------------------------------------------------- MOTOR

/// Under sustained load the motor saturates its droop and the delay
/// stands at base·(1+SAG_MAX) = 125 ms; unloaded (sag 0) it stands
/// at the dialed 100 ms. Measured by cross-correlating output
/// against input over the settled window.
#[test]
fn motor_sags_the_time_under_load() {
    let input = noise(192_000, 0.7);
    let dialed = (0.100 * FS) as usize; // 4800
    let lag_peak = |sag: f64| -> usize {
        let out = render(
            &[
                ("time", 100.0),
                ("feedback", 0.8),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 1.0),
                ("sag", sag),
                ("inertia", 0.3),
            ],
            &input,
        );
        // Settled window: well past the inertia time constant.
        let (from, len) = (120_000usize, 60_000usize);
        let mut best = (0usize, f64::MIN);
        for lag in 4_600..6_400 {
            let mut acc = 0.0f64;
            for i in from..from + len {
                acc += f64::from(out[i]) * f64::from(input[i - lag]);
            }
            if acc > best.1 {
                best = (lag, acc);
            }
        }
        best.0
    };
    let unloaded = lag_peak(0.0);
    assert!(
        (unloaded as i64 - dialed as i64).unsigned_abs() <= 8,
        "sag 0: delay at {unloaded}, dialed {dialed}"
    );
    let sagged = lag_peak(1.0);
    let full = (dialed as f32 * 1.25) as usize; // SAG_MAX = 0.25
    assert!(
        sagged > (dialed as f32 * 1.12) as usize && sagged <= full + 60,
        "sag 1 under saturating load: delay at {sagged}, expected near {full}"
    );
}

// ---------------------------------------------------------------- SHIFT

/// SHIFT = 0 bypasses the transposer: bit-identical to the classic
/// law (same promise heat 0 makes for CIRCUIT).
#[test]
fn shift_zero_is_the_classic_algorithm_bit_for_bit() {
    let input = noise(24_000, 0.5);
    let classic = render(&[("time", 50.0), ("feedback", 0.9), ("wet", 0.7)], &input);
    let out = render(
        &[
            ("time", 50.0),
            ("feedback", 0.9),
            ("wet", 0.7),
            ("algo", 2.0),
            ("shift", 0.0),
        ],
        &input,
    );
    for (i, (a, b)) in classic.iter().zip(out.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "shift 0 moved a bit at sample {i}"
        );
    }
}

/// +12 transposes the echo an octave: a 440 Hz program returns as
/// 880 Hz — the octave dominates the original by an order of
/// magnitude (the residue is crossfade sidebands and interpolation).
#[test]
fn shift_transposes_the_echo_an_octave() {
    let input = sine(96_000, 440.0, 0.4);
    let out = render(
        &[
            ("time", 80.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 2.0),
            ("shift", 12.0),
        ],
        &input,
    );
    let win = &out[48_000..96_000];
    let at_880 = goertzel(win, 880.0);
    let at_440 = goertzel(win, 440.0);
    assert!(at_880 > 1e-3, "octave inaudible: {at_880:.2e}");
    assert!(
        at_880 > 10.0 * at_440.max(1e-12),
        "octave {at_880:.2e} vs original {at_440:.2e} — not transposed"
    );
}

/// The interval COMPOUNDS per repeat: with the transposer in the
/// regeneration, repeat n sits n intervals up — a 440 Hz burst at
/// +12 returns at 880, then 1760.
#[test]
fn shift_compounds_per_repeat() {
    let mut input = sine(96_000, 440.0, 0.4);
    for x in input.iter_mut().skip(7_200) {
        *x = 0.0; // 150 ms burst, then silence — repeats stay separate
    }
    let out = render(
        &[
            ("time", 250.0),
            ("feedback", 0.7),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 2.0),
            ("shift", 12.0),
        ],
        &input,
    );
    let n = (0.250 * FS) as usize;
    // Repeat 1 window: the octave dominates. Repeat 2: two octaves.
    let w1 = &out[n..n + 7_200];
    let w2 = &out[2 * n..2 * n + 7_200];
    assert!(
        goertzel(w1, 880.0) > 3.0 * goertzel(w1, 440.0).max(1e-12),
        "repeat 1 is not the octave"
    );
    assert!(
        goertzel(w2, 1_760.0) > 3.0 * goertzel(w2, 880.0).max(1e-12),
        "repeat 2 is not two octaves"
    );
}

/// Envelope ripple: rectified, 30 Hz one-pole smoothed, then
/// (p95 − p5)/median over the settled window — the seam warble as
/// one number.
fn ripple(buf: &[f32]) -> f64 {
    let k = 1.0 - (-2.0 * core::f32::consts::PI * 30.0 / FS).exp();
    let mut env = 0.0f32;
    let mut envs: Vec<f32> = Vec::with_capacity(buf.len());
    for &x in buf {
        env += (x.abs() - env) * k;
        envs.push(env);
    }
    let mut s: Vec<f32> = envs[envs.len() / 4..].to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| f64::from(s[(q * (s.len() - 1) as f64) as usize]);
    (p(0.95) - p(0.05)) / p(0.5).max(1e-9)
}

/// SPLICE dials the seams at both ends. The dual-head crossfade is
/// a COMB between two copies offset by half a window — worst when
/// f0·w/2 lands on a half cycle (330 Hz at the default grain: the
/// heads cancel through every takeover, measured ripple 1.09 — the
/// "extra or missing cycles"). At SPLICE 1 the alignment search
/// re-phases each incoming head and the warble collapses (measured
/// 0.05, a 22× rescue); at SPLICE 0 the raw splice stays available
/// as a flavor. Both ends pinned.
#[test]
fn splice_dials_the_seam_from_warble_to_clean() {
    let input = sine(144_000, 330.0, 0.4);
    let seam = |splice: f64| -> f64 {
        let out = render(
            &[
                ("time", 80.0),
                ("feedback", 0.0),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 2.0),
                ("shift", 3.0),
                ("splice", splice),
            ],
            &input,
        );
        ripple(&out[48_000..144_000])
    };
    let raw = seam(0.0);
    let clean = seam(1.0);
    assert!(
        raw > 0.5,
        "the raw splice lost its warble ({raw:.3}) — the flavor end went missing"
    );
    assert!(clean < 0.15, "aligned seams still warble ({clean:.3})");
}

/// GRAIN changes the texture, never the pitch: the octave stays the
/// octave at both ends of the window range.
#[test]
fn grain_changes_texture_never_pitch() {
    let input = sine(96_000, 440.0, 0.4);
    for grain in [16.0, 160.0] {
        let out = render(
            &[
                ("time", 80.0),
                ("feedback", 0.0),
                ("dry", 0.0),
                ("wet", 1.0),
                ("algo", 2.0),
                ("shift", 12.0),
                ("grain", grain),
            ],
            &input,
        );
        let win = &out[48_000..96_000];
        assert!(
            goertzel(win, 880.0) > 10.0 * goertzel(win, 440.0).max(1e-12),
            "grain {grain}: the octave is not the octave"
        );
    }
}

/// The transposer never feeds the loop more than it took: the
/// crossfade gains sum to one, so at any interval the train still
/// dies at the dialed feedback.
#[test]
fn shift_never_runs_away() {
    let input = noise(9_600, 0.7); // a burst…
    let mut padded = input.clone();
    padded.resize(192_000, 0.0); // …then 3.8 s of silence
    let out = render(
        &[
            ("time", 150.0),
            ("feedback", 0.9),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 2.0),
            ("shift", 5.0),
        ],
        &padded,
    );
    // fb = 0.9 → six repeats on, energy must be well below onset:
    // 0.81^6 ≈ 0.28, asserted loose (transposer smear spreads it).
    let n = (0.150 * FS) as usize;
    let early = energy(&out, n, 2 * n);
    let late = energy(&out, 7 * n, 2 * n);
    assert!(early > 1e-6, "the train never started ({early:.2e})");
    assert!(
        late < 0.7 * early,
        "six repeats on, energy {late:.2e} vs onset {early:.2e} — not decaying"
    );
}

// ---------------------------------------------------------------- ORBIT

/// θ = 90°: an impulse in the LEFT ear orbits L → R → −L → −R and
/// returns to +L on repeat 5 (period 360/θ = 4). Positions and signs
/// are derived from the rotation group, not recorded.
#[test]
fn orbit_returns_the_image_in_the_dialed_period() {
    let n = (0.100 * FS) as usize;
    let mut left = vec![0.0f32; 8 * n];
    left[0] = 1.0;
    let right = vec![0.0f32; 8 * n];
    let (l, r) = render_lr(
        &[
            ("time", 100.0),
            ("feedback", 0.9),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 3.0),
            ("orbit", 90.0),
        ],
        &left,
        Some(&right),
    );
    // Signed peak (max |value|, sign kept) in a window per channel.
    let signed_peak = |buf: &[f32], k: usize| -> f32 {
        let (from, to) = (k * n - 20, k * n + 40);
        buf[from..to]
            .iter()
            .copied()
            .fold(0.0f32, |a, b| if b.abs() > a.abs() { b } else { a })
    };
    let dominant = |k: usize| -> (char, f32) {
        let (pl, pr) = (signed_peak(&l, k), signed_peak(&r, k));
        assert!(
            pl.abs().max(pr.abs()) > 10.0 * pl.abs().min(pr.abs()),
            "repeat {k}: image smeared (L {pl:.3}, R {pr:.3})"
        );
        if pl.abs() > pr.abs() {
            ('L', pl)
        } else {
            ('R', pr)
        }
    };
    let r1 = dominant(1);
    let r2 = dominant(2);
    let r3 = dominant(3);
    let r5 = dominant(5);
    assert_eq!(r1.0, 'L', "repeat 1 belongs to the written image");
    assert_eq!(r2.0, 'R', "repeat 2 rotated a quarter turn");
    assert_eq!(r3.0, 'L', "repeat 3 is the inverted center");
    assert!(r3.1 * r1.1 < 0.0, "repeat 3 carries the inverted sign");
    assert_eq!(r5.0, 'L', "repeat 5 completes the orbit");
    assert!(r5.1 * r1.1 > 0.0, "repeat 5 returns with repeat 1's sign");
}

/// Rotation is orthogonal — it moves energy around the image but
/// never adds any: at any angle the train still decays at the dialed
/// feedback.
#[test]
fn orbit_never_adds_energy() {
    let n = (0.100 * FS) as usize;
    let mut left = vec![0.0f32; 8 * n];
    left[0] = 1.0;
    let right = vec![0.0f32; 8 * n];
    let (l, r) = render_lr(
        &[
            ("time", 100.0),
            ("feedback", 0.8),
            ("dry", 0.0),
            ("wet", 1.0),
            ("algo", 3.0),
            ("orbit", 137.0),
        ],
        &left,
        Some(&right),
    );
    let e = |k: usize| -> f64 { energy(&l, k * n - 50, 200) + energy(&r, k * n - 50, 200) };
    for k in 1..6 {
        let ratio = e(k + 1) / e(k).max(1e-30);
        assert!(
            ratio <= 0.64 * 1.05,
            "repeat {}→{}: energy ratio {ratio:.3} outran fb²=0.64",
            k,
            k + 1
        );
    }
}
