//! THE DEPTH SUITE — front-to-back and enveloping-ness as
//! numbers (the field's brief: "jumping out of the speakers",
//! "maximum depth"). Four instruments:
//! * CONTRIBUTION PROFILES — the field's own idea: isolate stages
//!   (render difference) and watch energy + brightness over time;
//!   depth done right is an ordered handoff, bright/sparse →
//!   dark/dense, with no seam;
//! * THE RECESSION TRAJECTORY — the tail's spectral centroid must
//!   FALL over time (the reverb walks away), faster with DAMP;
//! * THE COHERENCE MATRIX — IACC per band per time window: lows
//!   anchored (the fold), highs diffusing as the tail evolves —
//!   the "true stereo depth" fingerprint;
//! * NATURAL COMPRESSION — a reverberant field sums hundreds of
//!   delayed copies: crest falls through the wet with ZERO
//!   nonlinearity (the physics discussion, measured).

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
    let (mut l, mut r) = (Vec::new(), Vec::new());
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
        l.extend_from_slice(&out_l);
        r.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (l, r)
}

fn render(set: &[(&str, f64)], input: &[f32], extra: usize) -> Vec<f32> {
    render_stereo(set, input, extra).0
}

fn impulse() -> Vec<f32> {
    let mut v = vec![0.0f32; 256];
    v[0] = 0.8;
    v
}

fn rms(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}

fn centroid(w: &[f32]) -> f64 {
    let n = w.len();
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for k in 0..40 {
        let hz = 150.0 * (8_000.0f64 / 150.0).powf(k as f64 / 39.0);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in w.iter().enumerate() {
            let hann = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
            let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
            re += f64::from(x) * hann * ph.cos();
            im += f64::from(x) * hann * ph.sin();
        }
        let m = (re * re + im * im).sqrt();
        num += hz * m;
        den += m;
    }
    num / den.max(1e-30)
}

fn band(w: &[f32], lo: f32, hi: f32) -> Vec<f32> {
    let kh = 1.0 - (-core::f32::consts::TAU * lo / FS).exp();
    let kl = 1.0 - (-core::f32::consts::TAU * hi / FS).exp();
    let mut zh = [0.0f32; 2];
    let mut zl = [0.0f32; 2];
    w.iter()
        .map(|x| {
            let mut v = *x;
            for z in zh.iter_mut() {
                *z += kh * (v - *z);
                v -= *z;
            }
            for z in zl.iter_mut() {
                *z += kl * (v - *z);
                v = *z;
            }
            v
        })
        .collect()
}

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

/// THE DEPTH REPORT: contribution profiles (ER isolated vs late),
/// recession trajectory, and the coherence matrix, printed.
#[test]
fn the_depth_report() {
    let base = [("mix", 1.0), ("predelay", 0.0), ("decay", 2_500.0)];
    let er_on: Vec<(&str, f64)> = base.iter().copied().chain([("early", 1.0)]).collect();
    let er_off: Vec<(&str, f64)> = base.iter().copied().chain([("early", 0.0)]).collect();
    let a = render(&er_on, &impulse(), 500);
    let b = render(&er_off, &impulse(), 500);
    let er: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
    println!("\n  CONTRIBUTIONS (energy dB / centroid Hz per 60 ms window):");
    for (name, w) in [("ER   ", &er), ("late ", &b)] {
        let mut row = format!("  {name}");
        for k in 0..8 {
            let s = (k * 2_880) + 144;
            let win = &w[s..s + 2_880];
            row += &format!(
                " {:>5.0}/{:>4.0}",
                20.0 * rms(win).max(1e-9).log10(),
                centroid(win)
            );
        }
        println!("{row}");
    }
    println!("  RECESSION (full-wet centroid per 150 ms window):");
    let mut row = String::from("  ");
    for k in 0..8 {
        let s = (0.1 * FS) as usize + k * 7_200;
        row += &format!(" {:>5.0}", centroid(&a[s..s + 7_200]));
    }
    println!("{row}");
    println!("  COHERENCE MATRIX (IACC band x window):");
    let (l, r) = render_stereo(&er_on, &impulse(), 500);
    for (lo, hi, label) in [
        (89.0, 250.0, "low "),
        (707.0, 2_828.0, "mid "),
        (2_828.0, 11_314.0, "high"),
    ] {
        let bl = band(&l, lo, hi);
        let br = band(&r, lo, hi);
        let e = iacc(&bl, &br, 144, (0.080 * FS) as usize);
        let m = iacc(&bl, &br, (0.15 * FS) as usize, (0.40 * FS) as usize);
        let lt = iacc(&bl, &br, (0.5 * FS) as usize, (1.0 * FS) as usize);
        println!("  {label}: early {e:.2}  mid {m:.2}  late {lt:.2}");
    }
}

/// THE RECESSION LAW: the tail's brightness falls over time (the
/// reverb walks away), and DAMP steepens the walk.
#[test]
fn the_tail_recedes() {
    let traj = |damp: f64| {
        let set = [
            ("mix", 1.0),
            ("predelay", 0.0),
            ("decay", 2_500.0),
            ("high energy", damp),
        ];
        let out = render(&set, &impulse(), 500);
        let early = centroid(&out[(0.10 * FS) as usize..(0.30 * FS) as usize]);
        let late = centroid(&out[(0.9 * FS) as usize..(1.4 * FS) as usize]);
        late / early
    };
    let open = traj(0.15);
    let damped = traj(0.7);
    assert!(
        open < 0.97,
        "the tail must recede even when open: ratio {open:.3}"
    );
    assert!(
        damped < open - 0.08,
        "damp must steepen the recession: open {open:.3} vs damped {damped:.3}"
    );
}

/// THE COHERENCE FINGERPRINT: lows anchored by the fold, highs
/// genuinely diffuse late — the enveloping-field contract.
#[test]
fn the_field_wraps_around() {
    let set = [("mix", 1.0), ("predelay", 0.0), ("decay", 2_500.0)];
    let (l, r) = render_stereo(&set, &impulse(), 500);
    let lo_l = band(&l, 89.0, 250.0);
    let lo_r = band(&r, 89.0, 250.0);
    let hi_l = band(&l, 2_828.0, 11_314.0);
    let hi_r = band(&r, 2_828.0, 11_314.0);
    let low_late = iacc(&lo_l, &lo_r, (0.5 * FS) as usize, (1.0 * FS) as usize);
    let high_late = iacc(&hi_l, &hi_r, (0.5 * FS) as usize, (1.0 * FS) as usize);
    // 0.78: two SEQUENTIAL mild one-pole folds (380 + 220 Hz,
    // independent states) measure 0.84 — corners sit ABOVE the
    // band so the phase-rotated region falls outside it (depth
    // alone saturated at 0.77; a 2-pole subtraction measured
    // WORSE, 0.56). Real rooms ~0.9.
    assert!(
        low_late > 0.78,
        "the lows must stay anchored: IACC {low_late:.2}"
    );
    assert!(
        high_late < 0.5,
        "the highs must diffuse: IACC {high_late:.2}"
    );
    assert!(
        low_late > high_late + 0.3,
        "coherence must fall with frequency: low {low_late:.2} vs high {high_late:.2}"
    );
}

/// THE ATTACK LOCALIZES, THE TAIL ENVELOPS: mid/high coherence
/// must EVOLVE — high early (the ER fold presents a placed image
/// that "jumps out"), low late (the tank wraps around). A field
/// that is equally diffuse from window one has no front-to-back.
#[test]
fn the_attack_localizes_then_envelops() {
    let set = [("mix", 1.0), ("predelay", 0.0), ("decay", 2_500.0)];
    let (l, r) = render_stereo(&set, &impulse(), 500);
    for (lo, hi, label) in [(707.0, 2_828.0, "mid"), (2_828.0, 11_314.0, "high")] {
        let bl = band(&l, lo, hi);
        let br = band(&r, lo, hi);
        let e = iacc(&bl, &br, 144, (0.080 * FS) as usize);
        let lt = iacc(&bl, &br, (0.5 * FS) as usize, (1.0 * FS) as usize);
        // Measured 0.60/0.57 early, 0.07-0.16 late.
        assert!(
            e >= 0.4,
            "{label}: the attack must localize: early IACC {e:.2}"
        );
        assert!(
            lt <= 0.3,
            "{label}: the tail must envelop: late IACC {lt:.2}"
        );
        assert!(
            e > lt + 0.2,
            "{label}: coherence must EVOLVE: {e:.2} -> {lt:.2}"
        );
    }
    // Above WIDTH 1 the ER fold opens — measured as early-window
    // side/mid ENERGY (IACC takes |corr| over ±1 ms, so a
    // hyper-wide near-antiphase attack reads as "coherent"; energy
    // ratio is the monotone instrument for "the fold opened").
    let side_mid = |l: &[f32], r: &[f32]| {
        let a = 144;
        let b = (0.080 * FS) as usize;
        let (mut es, mut em) = (0.0f64, 0.0f64);
        for i in a..b {
            let m = 0.5 * (f64::from(l[i]) + f64::from(r[i]));
            let sd = 0.5 * (f64::from(l[i]) - f64::from(r[i]));
            em += m * m;
            es += sd * sd;
        }
        es / em.max(1e-30)
    };
    let wide: Vec<(&str, f64)> = set.iter().copied().chain([("width", 2.0)]).collect();
    let (l2, r2) = render_stereo(&wide, &impulse(), 500);
    let nat = side_mid(&l, &r);
    let opened = side_mid(&l2, &r2);
    assert!(
        opened > 2.0 * nat,
        "hyper-width must reopen the ER's side: {nat:.3} -> {opened:.3}"
    );
}

/// NATURAL COMPRESSION, measured: the wet's crest is LOWER than
/// the dry burst train's — linear temporal integration, the
/// physics-honest 'glue' (no nonlinearity involved; the linearity
/// laws prove that separately).
#[test]
fn the_field_glues_linearly() {
    let bursts: Vec<f32> = {
        let mut rng = 0x61_0e5u32;
        (0..96_000)
            .map(|i| {
                if i % 12_000 < 480 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5
                } else {
                    0.0
                }
            })
            .collect()
    };
    let crest = |w: &[f32]| {
        let peak = w.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        f64::from(peak) / rms(w).max(1e-15)
    };
    let dry_crest = crest(&bursts);
    let wet = render(&[("mix", 1.0), ("predelay", 0.0)], &bursts, 100);
    let wet_crest = crest(&wet[..96_000]);
    // Measured 0.77 (8.7 → 6.8) after the early-coherence slice
    // (was 0.83): the folded ER sums denser, per-burst peaks
    // integrate sooner. 0.6 stays the aspiration (a later slice's
    // pursuit). Pin: a glue-less wet reads ~1.
    println!(
        "  GLUE: dry crest {dry_crest:.2} wet {wet_crest:.2} ratio {:.2}",
        wet_crest / dry_crest
    );
    assert!(
        wet_crest < 0.85 * dry_crest,
        "the field must glue: dry crest {dry_crest:.1} vs wet {wet_crest:.1}"
    );
}
