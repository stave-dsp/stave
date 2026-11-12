//! THE ACOUSTICS SCOREBOARD — quality as numbers, not adjectives.
//! Vault renders deterministic impulse (and loud-burst) responses
//! and the harness computes the room-acoustics literature's metric
//! suite on them, per REVERB CLASS (rooms of three sizes, plate,
//! spring) — so "dimension", "size cues" and "character" become
//! measurable columns:
//! * T30 / EDT + the Schroeder curve's linearity — decay honesty
//!   and NATURALNESS (a kinked or front-loaded curve is the
//!   "forced envelope" the field heard);
//! * NED(t), normalized echo density (Abel–Huang) — diffusion
//!   quality over time: real rooms rise smoothly to ~1 by ~100 ms,
//!   a plate is ~instant, "bouncy delays" plateau low;
//! * C80 / D50 / Ts — the early/mid/late energy structure;
//! * IACC early/late — the field's dimension (real spaces
//!   decorrelate over time);
//! * band T30s → bass ratio — warmth (aspirational until per-band
//!   decay ships; reported, not asserted);
//! * spectral RIDGE of the late tail — the metallic detector;
//! * FLUTTER — envelope periodicity (a spring's signature; a
//!   room's defect).
//!
//! NONLINEARITY IS AN EXCITATION OPTION: a nonlinear machine has
//! no single IR, so every class can also be measured under a loud
//! 120 ms burst (drive engages at performance levels only —
//! measured on the drive law). The scoreboard prints both worlds;
//! the laws pin what must hold today and ratchet as slices land.
//! The SPRING class is REPORT-ONLY until the spring topology
//! exists (no dispersion chirp yet — recorded, not pretended).

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
    let (mut left, mut right) = (Vec::new(), Vec::new());
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
        left.extend_from_slice(&out_l);
        right.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (left, right)
}

#[derive(Clone, Copy, PartialEq)]
enum Excite {
    Impulse,
    /// 120 ms of noise at 0.45 — performance level, where the
    /// nonlinear path actually engages.
    Burst,
}

fn excitation(e: Excite) -> (Vec<f32>, usize) {
    match e {
        Excite::Impulse => {
            let mut v = vec![0.0f32; 256];
            v[0] = 0.8;
            (v, 1)
        }
        Excite::Burst => {
            let n = (0.12 * FS) as usize;
            let mut rng = 0xACE5_7A1Eu32;
            let v: Vec<f32> = (0..n)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.45
                })
                .collect();
            (v, n)
        }
    }
}

/// Backward-integrated energy decay (Schroeder), dB re total,
/// starting at `from`.
fn schroeder_db(l: &[f32], r: &[f32], from: usize) -> Vec<f64> {
    let e: Vec<f64> = (from..l.len())
        .map(|i| f64::from(l[i]) * f64::from(l[i]) + f64::from(r[i]) * f64::from(r[i]))
        .collect();
    let total: f64 = e.iter().sum();
    let mut acc = 0.0f64;
    let mut out = vec![0.0f64; e.len()];
    for (i, x) in e.iter().enumerate().rev() {
        acc += x;
        out[i] = 10.0 * (acc / total.max(1e-30)).log10();
    }
    out
}

/// First index at or below the level.
fn cross(sch: &[f64], db: f64) -> Option<usize> {
    sch.iter().position(|&x| x <= db)
}

/// Decay time extrapolated to −60 dB from the [hi, lo] region.
fn decay_time(sch: &[f64], hi: f64, lo: f64) -> Option<f64> {
    let a = cross(sch, hi)?;
    let b = cross(sch, lo)?;
    if b <= a {
        return None;
    }
    Some((b - a) as f64 / f64::from(FS) * 60.0 / (hi - lo).abs())
}

/// Max |residual| in dB of a straight-line fit over [hi, lo] —
/// the decay-NATURALNESS number (real rooms are straight).
fn linearity_residual(sch: &[f64], hi: f64, lo: f64) -> f64 {
    let (Some(a), Some(b)) = (cross(sch, hi), cross(sch, lo)) else {
        return f64::NAN;
    };
    if b <= a + 8 {
        return f64::NAN;
    }
    let n = (b - a) as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for (k, &y) in sch[a..b].iter().enumerate() {
        let x = k as f64;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx).max(1e-12);
    let icpt = (sy - slope * sx) / n;
    sch[a..b]
        .iter()
        .enumerate()
        .map(|(k, &y)| (y - (icpt + slope * k as f64)).abs())
        .fold(0.0f64, f64::max)
}

/// Normalized echo density (Abel–Huang): fraction of samples
/// beyond one std in a sliding window, normalized by the Gaussian
/// expectation (erfc(1/√2) ≈ 0.3173). Returns the time (s after
/// `from`) it first sustains ≥ 0.9, and its value at 25 ms.
fn ned_profile(l: &[f32], from: usize) -> (f64, f64) {
    let win = (0.020 * FS) as usize;
    let hop = (0.005 * FS) as usize;
    let mut t90 = f64::NAN;
    let mut at25 = f64::NAN;
    let mut streak = 0;
    let mut k = 0;
    while from + k * hop + win < l.len() && k < 400 {
        let w = &l[from + k * hop..from + k * hop + win];
        let mean = w.iter().map(|x| f64::from(*x)).sum::<f64>() / win as f64;
        let var = w
            .iter()
            .map(|x| (f64::from(*x) - mean).powi(2))
            .sum::<f64>()
            / win as f64;
        let sd = var.sqrt().max(1e-15);
        let frac = w
            .iter()
            .filter(|x| (f64::from(**x) - mean).abs() > sd)
            .count() as f64
            / win as f64;
        let ned = frac / 0.3173;
        let t = (k * hop) as f64 / f64::from(FS);
        if (t - 0.025).abs() < 0.004 && at25.is_nan() {
            at25 = ned;
        }
        if ned >= 0.9 {
            streak += 1;
            if streak >= 2 && t90.is_nan() {
                t90 = ((k - 1) * hop) as f64 / f64::from(FS);
            }
        } else {
            streak = 0;
        }
        k += 1;
    }
    (t90, at25)
}

/// Clarity C80 (dB), definition D50, center time Ts (ms).
fn clarity(l: &[f32], r: &[f32], from: usize) -> (f64, f64, f64) {
    let e = |i: usize| f64::from(l[i]).powi(2) + f64::from(r[i]).powi(2);
    let n80 = from + (0.080 * FS) as usize;
    let n50 = from + (0.050 * FS) as usize;
    let early80: f64 = (from..n80.min(l.len())).map(e).sum();
    let late80: f64 = (n80.min(l.len())..l.len()).map(e).sum();
    let early50: f64 = (from..n50.min(l.len())).map(e).sum();
    let total: f64 = (from..l.len()).map(e).sum();
    let ts: f64 = (from..l.len())
        .map(|i| (i - from) as f64 * e(i))
        .sum::<f64>()
        / total.max(1e-30)
        / f64::from(FS)
        * 1_000.0;
    (
        10.0 * (early80 / late80.max(1e-30)).log10(),
        early50 / total.max(1e-30),
        ts,
    )
}

/// Interaural cross-correlation: max normalized correlation over
/// ±1 ms lags in [a, b).
fn iacc(l: &[f32], r: &[f32], a: usize, b: usize) -> f64 {
    let b = b.min(l.len());
    if b <= a + 100 {
        return f64::NAN;
    }
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

/// Spectral ridge of the late tail: max/mean of correlation
/// magnitudes over a log grid — the METALLIC detector (a comb's
/// surviving modes tower over the mean).
fn ridge(l: &[f32], a: usize, b: usize) -> f64 {
    let b = b.min(l.len());
    if b <= a + 4_800 {
        return f64::NAN;
    }
    let w = &l[a..b];
    let mut mags = Vec::with_capacity(96);
    for k in 0..96 {
        let hz = 300.0 * (5_000.0f64 / 300.0).powf(k as f64 / 95.0);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in w.iter().enumerate() {
            let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
            re += f64::from(x) * ph.cos();
            im += f64::from(x) * ph.sin();
        }
        mags.push((re * re + im * im).sqrt());
    }
    let mean: f64 = mags.iter().sum::<f64>() / mags.len() as f64;
    mags.iter().fold(0.0f64, |m, &x| m.max(x)) / mean.max(1e-30)
}

/// Envelope periodicity in 15–120 ms lags — a spring's flutter
/// signature (and a room's defect).
fn flutter(l: &[f32], from: usize) -> f64 {
    let n = ((0.8 * FS) as usize).min(l.len() - from);
    let k = 1.0 - (-core::f64::consts::TAU * 80.0 / f64::from(FS)).exp();
    let mut z = 0.0f64;
    let env: Vec<f64> = l[from..from + n]
        .iter()
        .map(|x| {
            z += k * (f64::from(x.abs()) - z);
            z
        })
        .collect();
    // DETREND: subtract a slow baseline (6 Hz one-pole) — a
    // decaying envelope autocorrelates at every lag through its
    // trend alone (measured 0.8+ on every class, meaningless).
    let kb = 1.0 - (-core::f64::consts::TAU * 6.0 / f64::from(FS)).exp();
    let mut zb = 0.0f64;
    let e: Vec<f64> = env
        .iter()
        .map(|x| {
            zb += kb * (x - zb);
            x - zb
        })
        .collect();
    let acf0: f64 = e.iter().map(|x| x * x).sum();
    let mut best = 0.0f64;
    let mut lag = (0.015 * FS) as usize;
    while lag < (0.120 * FS) as usize {
        let acc: f64 = (0..n - lag).map(|i| e[i] * e[i + lag]).sum();
        best = best.max(acc / acf0.max(1e-30));
        lag += 24;
    }
    best
}

struct Metrics {
    t30: f64,
    edt: f64,
    residual: f64,
    ned_t90: f64,
    ned_at25: f64,
    c80: f64,
    d50: f64,
    ts: f64,
    iacc_early: f64,
    iacc_late: f64,
    ridge: f64,
    flutter: f64,
    peak: f64,
    tail_end: f64,
}

fn measure(set: &[(&str, f64)], excite: Excite, seconds: f64) -> Metrics {
    let (input, from) = excitation(excite);
    let blocks = (seconds * f64::from(FS)) as usize / BLOCK;
    let (l, r) = render_stereo(set, &input, blocks);
    let sch = schroeder_db(&l, &r, from);
    let n80 = from + (0.080 * FS) as usize;
    let late_a = from + (0.30 * FS) as usize;
    let late_b = from + (0.80 * FS) as usize;
    Metrics {
        t30: decay_time(&sch, -5.0, -35.0).unwrap_or(f64::NAN),
        edt: decay_time(&sch, -0.5, -10.0).unwrap_or(f64::NAN),
        residual: linearity_residual(&sch, -5.0, -35.0),
        ned_t90: ned_profile(&l, from).0,
        ned_at25: ned_profile(&l, from).1,
        c80: clarity(&l, &r, from).0,
        d50: clarity(&l, &r, from).1,
        ts: clarity(&l, &r, from).2,
        iacc_early: iacc(&l, &r, from, n80),
        iacc_late: iacc(&l, &r, late_a, late_b),
        ridge: ridge(&l, late_a, late_b),
        flutter: flutter(&l, from),
        peak: l.iter().fold(0.0f64, |m, x| m.max(f64::from(x.abs()))),
        tail_end: {
            let a = l.len().saturating_sub((0.3 * FS) as usize);
            (l[a..].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / (l.len() - a) as f64)
                .sqrt()
        },
    }
}

/// The class table: name, dialed T60 (s), the param set, render
/// seconds. SPRING is report-only (no spring topology yet — the
/// flutter and dispersion its class demands do not exist in the
/// engine; the row records the gap).
/// (name, t60, settings, seconds)
type Class = (&'static str, f64, Vec<(&'static str, f64)>, f64);

fn classes() -> Vec<Class> {
    vec![
        (
            "ROOM-S",
            0.6,
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("size", 0.45),
                ("decay", 600.0),
            ],
            3.0,
        ),
        (
            "CHAMBER",
            1.5,
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("size", 1.0),
                ("decay", 1_500.0),
            ],
            4.0,
        ),
        (
            "HALL",
            2.8,
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("size", 2.4),
                ("decay", 2_800.0),
            ],
            6.0,
        ),
        (
            "PLATE",
            2.2,
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("size", 0.85),
                ("decay", 2_200.0),
                ("topology", 1.0),
                ("high energy", 0.2),
                ("whip", 0.6),
            ],
            5.0,
        ),
        (
            "SPRING?",
            1.8,
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("size", 0.32),
                ("decay", 1_800.0),
                ("high energy", 0.55),
                ("early", 0.3),
            ],
            4.0,
        ),
    ]
}

/// The scoreboard, printed (cargo test -- --nocapture). Reference
/// bands from the room-acoustics literature sit in the header for
/// the eye; the LAWS below assert today's regression pins.
#[test]
fn the_scoreboard() {
    println!(
        "\n  class     ex     T30    EDT/T30  resid  NEDt90  NED@25  C80    D50    Ts     IACCe  IACCl  ridge  flut   peak"
    );
    println!(
        "  (real rooms: EDT/T30≈1, resid<3dB, NEDt90<0.15s, IACCl<0.4; plate: NEDt90<0.03; spring: flut>0.3)"
    );
    println!(
        "  (IACCe note: literature values ~0.7-0.95 INCLUDE the direct sound; these are WET-ONLY — not comparable)"
    );
    println!(
        "  (flut note: tank-only classes read SLOW ENVELOPE BEATING (finite mode density), not repetition — probe showed monotone acf, no line-period peak; ER masks it in room classes; cure = mode density, on the roadmap)"
    );
    for (name, _t60, set, secs) in classes() {
        for ex in [Excite::Impulse, Excite::Burst] {
            let m = measure(&set, ex, secs);
            println!(
                "  {:<9} {:<5} {:>6.2} {:>7.2} {:>7.2} {:>7.3} {:>7.2} {:>6.1} {:>6.2} {:>6.0} {:>6.2} {:>6.2} {:>6.1} {:>6.2} {:>6.2}",
                name,
                if ex == Excite::Impulse { "imp" } else { "burst" },
                m.t30,
                m.edt / m.t30,
                m.residual,
                m.ned_t90,
                m.ned_at25,
                m.c80,
                m.d50,
                m.ts,
                m.iacc_early,
                m.iacc_late,
                m.ridge,
                m.flutter,
                m.peak,
            );
        }
    }
}

/// The ROOM classes hold their shapes (regression pins — tighten
/// as slices land; the aspirational bands live in the scoreboard
/// header).
#[test]
fn the_room_classes_hold_their_shapes() {
    // T30 baselines are MEASURED regression pins (2026-07-19), not
    // the dial: default DAMP absorbs broadband energy beyond the
    // T60 gain, so bigger rooms legitimately read below their dial
    // until per-band decay (V3) makes the promise damp-aware. The
    // dial-honesty law lives in properties.rs at damp 0. ROOM-S measured
    // 0.61 vs 0.60 dialed — the loop math itself is honest.
    // Re-pinned 2026-07-19 after the second in-loop diffuser (the
    // loop legitimately lengthened; the t60 law re-calibrated the
    // AP compensation at damp 0, classes run the default damp).
    // Re-pinned 2026-07-19 with the modal low end: low modes ring
    // past the mid decay (bass-ratio physics), so broadband T30
    // legitimately lengthened.
    let t30_pin = [("ROOM-S", 0.74), ("CHAMBER", 1.43), ("HALL", 1.63)];
    for (name, _t60, set, secs) in classes() {
        if name == "SPRING?" || name == "PLATE" {
            continue;
        }
        let m = measure(&set, Excite::Impulse, secs);
        let pin = t30_pin.iter().find(|(n, _)| *n == name).unwrap().1;
        assert!(
            (m.t30 / pin - 1.0).abs() < 0.30,
            "{name}: T30 {:.2}s vs pinned baseline {pin}s",
            m.t30
        );
        assert!(
            m.residual < 8.0,
            "{name}: Schroeder residual {:.2} dB",
            m.residual
        );
        assert!(
            m.ned_t90 < 0.25,
            "{name}: diffusion too slow, NED t90 {:.3}s",
            m.ned_t90
        );
        assert!(m.ridge < 30.0, "{name}: metallic ridge {:.1}", m.ridge);
        assert!(
            m.iacc_late < 0.85,
            "{name}: late field too correlated: {:.2}",
            m.iacc_late
        );
    }
}

/// THE CATHEDRAL HOLDS (worklist 6 guard): the stochastic
/// order-3+ taps extend the skeleton (their law lives in-crate,
/// room.rs — field-access isolation); the FULL render at size 2.7
/// must stay inside its class shape. Measured at the slice:
/// NED@25 0.483, flutter 0.630 (tank-only 0.656 — the flutter IS
/// the tank's line periodicity at this scale; cure = mode
/// density, on the roadmap).
#[test]
fn the_cathedral_holds_its_shape() {
    let set = vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("size", 2.7),
        ("decay", 4_000.0),
    ];
    let m = measure(&set, Excite::Impulse, 7.0);
    println!(
        "  CATHEDRAL: NED@25 {:.3}  flutter {:.3}  ned_t90 {:.3}  C80 {:.1}",
        m.ned_at25, m.flutter, m.ned_t90, m.c80
    );
    assert!(
        m.flutter < 0.72,
        "cathedral beats: flutter {:.2}",
        m.flutter
    );
    assert!(
        m.ned_t90 < 0.16,
        "cathedral density too slow: t90 {:.3}",
        m.ned_t90
    );
    assert!(m.residual < 8.0, "cathedral residual {:.2} dB", m.residual);
}

/// The PLATE is instant density with no metal.
#[test]
fn the_plate_is_instant_density() {
    let (_, t60, set, secs) = classes().into_iter().find(|(n, ..)| *n == "PLATE").unwrap();
    let _ = t60;
    let m = measure(&set, Excite::Impulse, secs);
    // The DEDICATED plate topology (the topologies arc): density
    // is instant — 0.005 s measured vs 0.075 s when "PLATE" was
    // the room's FDN with the ER muted.
    assert!(
        m.ned_t90 < 0.03,
        "plate density too slow: NED t90 {:.3}s",
        m.ned_t90
    );
    // Measured baseline pin (damp 0.2 shortens below the dial).
    assert!(
        (m.t30 / 2.0 - 1.0).abs() < 0.30,
        "plate T30 {:.2}s vs pinned 2.0",
        m.t30
    );
    assert!(m.ridge < 30.0, "plate rings metallic: ridge {:.1}", m.ridge);
}

/// NONLINEARITY IS AN OPTION, NOT A BREAK: under loud-burst
/// excitation with drive dialed, every class stays bounded and
/// still falls silent.
#[test]
fn nonlinearity_is_an_option_not_a_break() {
    for (name, _t60, set, secs) in classes() {
        let m = measure(&set, Excite::Burst, secs + 1.0);
        assert!(m.peak < 2.5, "{name}+drive: peak {:.2}", m.peak);
        assert!(
            m.tail_end < 1e-3,
            "{name}+drive: never dies ({:.2e})",
            m.tail_end
        );
    }
}

/// Probe: WHERE is the plate's envelope periodicity? (Two
/// dispersion attempts moved flutter 0.77→0.74 — the model of
/// what's fluttering must be wrong. Find the dominant lag.)
#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_plate_flutter_lag() {
    let set = vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("size", 0.85),
        ("decay", 2_200.0),
        ("early", 0.0),
        ("high energy", 0.2),
        ("diffuse", 0.95),
        ("whip", 0.6),
    ];
    let (input, from) = excitation(Excite::Impulse);
    let (l, _r) = render_stereo(&set, &input, (5.0 * f64::from(FS)) as usize / BLOCK);
    let n = ((0.8 * FS) as usize).min(l.len() - from);
    let k = 1.0 - (-core::f64::consts::TAU * 80.0 / f64::from(FS)).exp();
    let mut z = 0.0f64;
    let env: Vec<f64> = l[from..from + n]
        .iter()
        .map(|x| {
            z += k * (f64::from(x.abs()) - z);
            z
        })
        .collect();
    let kb = 1.0 - (-core::f64::consts::TAU * 6.0 / f64::from(FS)).exp();
    let mut zb = 0.0f64;
    let e: Vec<f64> = env
        .iter()
        .map(|x| {
            zb += kb * (x - zb);
            x - zb
        })
        .collect();
    let acf0: f64 = e.iter().map(|x| x * x).sum();
    let mut lag = (0.010 * FS) as usize;
    let mut peaks: Vec<(f64, f64)> = Vec::new();
    while lag < (0.150 * FS) as usize {
        let acc: f64 = (0..n - lag).map(|i| e[i] * e[i + lag]).sum();
        peaks.push((lag as f64 / f64::from(FS) * 1_000.0, acc / acf0));
        lag += 24;
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (ms, v) in peaks.iter().take(8) {
        println!("lag {ms:.1} ms: acf {v:.3}");
    }
    // Also with motion 0 — is the chorus the driver?
    let set2: Vec<(&str, f64)> = set
        .iter()
        .map(|&(n, v)| (n, if n == "motion" { 0.0 } else { v }))
        .collect();
    let (l2, _) = render_stereo(&set2, &input, (5.0 * f64::from(FS)) as usize / BLOCK);
    let mut z = 0.0f64;
    let env2: Vec<f64> = l2[from..from + n]
        .iter()
        .map(|x| {
            z += k * (f64::from(x.abs()) - z);
            z
        })
        .collect();
    let mut zb = 0.0f64;
    let e2: Vec<f64> = env2
        .iter()
        .map(|x| {
            zb += kb * (x - zb);
            x - zb
        })
        .collect();
    let acf0b: f64 = e2.iter().map(|x| x * x).sum();
    let mut best = (0.0f64, 0.0f64);
    let mut lag = (0.010 * FS) as usize;
    while lag < (0.150 * FS) as usize {
        let acc: f64 = (0..n - lag).map(|i| e2[i] * e2[i + lag]).sum();
        if acc / acf0b > best.1 {
            best = (lag as f64 / f64::from(FS) * 1_000.0, acc / acf0b);
        }
        lag += 24;
    }
    println!("motion 0: best lag {:.1} ms acf {:.3}", best.0, best.1);
}

/// Probe: echo density at CATHEDRAL scale (field: "sounds like a
/// multitap delay" on long/large settings).
#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_cathedral_density() {
    for size in [1.0, 1.8, 2.7] {
        let set = vec![
            ("mix", 1.0),
            ("predelay", 0.0),
            ("size", size),
            ("decay", 6_000.0),
            ("early", 1.0),
        ];
        let m = measure(&set, Excite::Impulse, 6.0);
        println!(
            "size {size}: NED t90 {:.3}s NED@25 {:.2} C80 {:.1} Ts {:.0} flut {:.2}",
            m.ned_t90, m.ned_at25, m.c80, m.ts, m.flutter
        );
    }
}
