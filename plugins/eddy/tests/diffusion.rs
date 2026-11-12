//! The FIELD laws — the promises the diffusion stage makes under
//! its three couplings (DIFFUSE = heard morph, BLEED = regeneration
//! morph, HOLD = the field's own ring). Every expected value is
//! DERIVED or carries its measured anchor; the probes that set the
//! anchors were deleted (bit-pin discipline), the numbers stay here
//! as the record.
//!
//!  * Both couplings at 0 is the classic law bit for bit.
//!  * THE SING: at a HALF-SECOND delay the field rings through the
//!    gaps — the wash finally has its own clock (the complaint this
//!    topology exists to fix).
//!  * The diagonal (bleed = diffuse, hold 0) IS the old in-loop
//!    compounding: PLATE's density still compounds, PRISM still
//!    chirps, its smear-0 end is still a bit-clean wire.
//!  * Ghost mode: DIFFUSE 0 / BLEED up keeps first arrivals
//!    bit-sharp while the loop dissolves behind them.
//!  * No coupling combination runs away: sustained resonant drive
//!    at max EVERYTHING reaches steady state (this law caught the
//!    unnormalized bleed product at 1e24 and the plate wander
//!    pumping the hold loop at 1.16× — both measured, both fixed).

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};
use stave_test::{assert_no_pump, resonant_tone, StabilityPolicy};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
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
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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

/// Stereo render with correlated (centered) input — for the image
/// laws.
fn render_lr(set: &[(&str, f64)], input: &[f32]) -> (Vec<f32>, Vec<f32>) {
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
    let (mut ol, mut or) = (Vec::new(), Vec::new());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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
        ol.extend_from_slice(&out_l);
        or.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (ol, or)
}

fn noise_burst(n: usize, total: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; total];
    let mut lcg: u32 = 99;
    for x in v.iter_mut().take(n) {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.4;
    }
    v
}

fn energy(o: &[f32], a: usize, len: usize) -> f64 {
    o[a.min(o.len())..(a + len).min(o.len())]
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum()
}

/// The per-voice max texture settings, on the DIAGONAL (bleed =
/// diffuse = 1, hold 0) — the coupling that reproduces the old
/// in-loop compounding topology.
fn diagonal_settings(kind: usize) -> Vec<(&'static str, f64)> {
    let mut set = vec![
        ("time", 150.0),
        ("feedback", 0.9),
        ("dry", 0.0),
        ("wet", 1.0),
        ("diffuse", 1.0),
        ("bleed", 1.0),
        ("hold", 0.0),
        ("diffusion", kind as f64),
    ];
    match kind {
        0 => set.push(("density", 1.0)),
        1 => set.push(("smear", 1.0)),
        2 => set.push(("spray", 1.0)),
        _ => set.push(("weave", 1.0)),
    }
    set
}

/// Both couplings at 0 bypass the stage: bit-identical to the
/// classic law for every voice.
#[test]
fn field_off_is_the_classic_algorithm_bit_for_bit() {
    let mut lcg: u32 = 0x1234_5678;
    let input: Vec<f32> = (0..24_000)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5
        })
        .collect();
    let classic = render(&[("time", 50.0), ("feedback", 0.9), ("wet", 0.7)], &input);
    for kind in 0..4 {
        let out = render(
            &[
                ("time", 50.0),
                ("feedback", 0.9),
                ("wet", 0.7),
                ("diffusion", f64::from(kind)),
                ("diffuse", 0.0),
                ("bleed", 0.0),
                ("hold", 0.8),
            ],
            &input,
        );
        for (i, (a, b)) in classic.iter().zip(out.iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "voice {kind} with field off moved bit {i}"
            );
        }
    }
}

/// THE SING — the law this topology exists for: at a HALF-SECOND
/// delay time, with HOLD up and BLEED at zero, the field rings
/// through the silence between repeats while the repeats stay
/// articulate. PLATE and MESH are true spaces; PRISM's ring is a
/// sparse chirp train. DUST is domain-qualified OUT of the deep-gap
/// clause: its taps shed energy every internal lap and its hold
/// ceiling is a pump limit (the doctrine) — dust RINGS BRIEFLY and
/// settles, and that brief ring is asserted instead.
#[test]
fn the_field_sings_between_distant_repeats() {
    let n = (0.500 * FS) as usize;
    let mut imp = vec![0.0f32; 6 * n];
    imp[0] = 1.0;
    let ring = |kind: usize| -> Vec<f32> {
        render(
            &[
                ("time", 500.0),
                ("feedback", 0.7),
                ("dry", 0.0),
                ("wet", 1.0),
                ("diffusion", kind as f64),
                ("diffuse", 1.0),
                ("hold", 1.0),
                ("bleed", 0.0),
            ],
            &imp,
        )
    };
    for (kind, floor) in [(0usize, 0.1), (1, 0.005), (3, 0.1)] {
        let out = ring(kind);
        let gap = energy(&out, n + n / 3, n / 3);
        let rep = energy(&out, n - 400, 4_000);
        assert!(
            gap > floor * rep,
            "voice {kind}: gap/repeat {:.5} under floor {floor} — the field went silent",
            gap / rep.max(1e-30)
        );
    }
    // DUST: a brief settle after each repeat, quiet by mid-gap.
    let out = ring(2);
    let settle = energy(&out, n + 2_000, 5_000);
    let rep = energy(&out, n - 400, 4_000);
    assert!(
        settle > 1e-4 * rep,
        "dust never rang at all ({:.2e} of the repeat)",
        settle / rep.max(1e-30)
    );
}

/// Ghost mode: DIFFUSE 0 / BLEED up — every FIRST arrival is
/// bit-identical to the classic delay (the ear path is untouched
/// until regenerated content returns), while the loop dissolves
/// behind it.
#[test]
fn ghost_mode_keeps_first_arrivals_sharp() {
    let n = (0.150 * FS) as usize;
    let mut imp = vec![0.0f32; 5 * n];
    imp[0] = 1.0;
    let classic = render(
        &[
            ("time", 150.0),
            ("feedback", 0.9),
            ("dry", 0.0),
            ("wet", 1.0),
        ],
        &imp,
    );
    let ghost = render(
        &[
            ("time", 150.0),
            ("feedback", 0.9),
            ("dry", 0.0),
            ("wet", 1.0),
            ("diffusion", 0.0),
            ("diffuse", 0.0),
            ("bleed", 1.0),
            ("hold", 0.7),
        ],
        &imp,
    );
    for i in 0..(2 * n - 200) {
        assert_eq!(
            classic[i].to_bits(),
            ghost[i].to_bits(),
            "ghost mode blurred the first arrival at sample {i}"
        );
    }
    let diverged = (2 * n..4 * n).any(|i| classic[i].to_bits() != ghost[i].to_bits());
    assert!(diverged, "the ghost never dissolved — bleed is dead");
}

/// No coupling combination adds energy, under either torture:
/// * impulse train at fb 0.9 on the diagonal — the tail DECAYS;
/// * a sustained resonant tone at fb 0.95 with EVERYTHING maxed
///   (diffuse, hold, bleed, textures) — steady state, never growth.
///   This law's probe history: the mesh's equal-angle rotation
///   eigenvalue rode to 115× (golden detune), the unnormalized
///   bleed product hit 1e24 (the loop field is (1 − hg)-normalized
///   now), and the plate wander pumped the hold loop 1.16× (wander
///   shrinks with hold).
#[test]
fn diffusion_redistributes_never_adds() {
    let n = (0.150 * FS) as usize;
    let mut imp = vec![0.0f32; 12 * n];
    imp[0] = 1.0;
    for kind in 0..4 {
        let out = render(&diagonal_settings(kind), &imp);
        let early: f64 = (1..=4).map(|k| energy(&out, k * n, n)).sum();
        let late: f64 = (7..=10).map(|k| energy(&out, k * n, n)).sum();
        assert!(
            late < 0.6 * early,
            "voice {kind}: impulse train not decaying (late {late:.3e} vs early {early:.3e})"
        );
    }
    // The trend doctrine (windows, spacing, bound, and why) lives in
    // stave_test::stability — it was carved from THIS law's
    // incident history (the 1e24 bleed product, the 115× mesh
    // eigenvalue, the plate wander pump, DUST's "1.4×" that was
    // response wander, not growth).
    let policy = StabilityPolicy::default();
    let tone = resonant_tone(FS, &policy);
    for kind in 0..4 {
        let mut set = diagonal_settings(kind);
        for slot in set.iter_mut() {
            match slot.0 {
                "feedback" => slot.1 = 0.95,
                "hold" => slot.1 = 1.0,
                _ => {}
            }
        }
        let out = render(&set, &tone);
        assert_no_pump(&out, FS, &policy, &format!("voice {kind}"));
    }
}

/// PLATE's density COMPOUNDS on the diagonal: the temporal spread
/// (second moment) of repeat 3 exceeds repeat 1's. With the tank,
/// the FIRST pass is already wide (measured 1384 samples vs the old
/// lattice's 865), so the relative growth is smaller (→ 1572,
/// +14%) — both the width and the growth are asserted.
#[test]
fn plate_compounds_the_density() {
    let n = (0.150 * FS) as usize;
    let mut imp = vec![0.0f32; 8 * n];
    imp[0] = 1.0;
    let out = render(&diagonal_settings(0), &imp);
    let spread = |k: usize| -> f64 {
        let w = &out[k * n - 400..k * n + 5_000];
        let (mut e, mut te, mut t2e) = (0.0f64, 0.0f64, 0.0f64);
        for (i, x) in w.iter().enumerate() {
            let p = f64::from(*x) * f64::from(*x);
            e += p;
            te += p * i as f64;
            t2e += p * (i as f64) * (i as f64);
        }
        let c = te / e.max(1e-30);
        (t2e / e.max(1e-30) - c * c).sqrt()
    };
    let (s1, s3) = (spread(1), spread(3));
    assert!(
        s1 > 1_000.0,
        "first pass spread {s1:.0} — the tank forgot to diffuse"
    );
    assert!(
        s3 > 1.06 * s1,
        "repeat 3 spread {s3:.0} vs repeat 1 {s1:.0} — density is not compounding"
    );
}

/// PRISM chirps the LOWS late — the only direction first-order
/// dispersion permits — and by an amount SMEAR dials: at full smear
/// the low band's arrival centroid trails the high band's by
/// hundreds of samples per repeat; at smear 0 the line is a wire
/// (bit-identical to the classic law even at full DIFFUSE).
#[test]
fn prism_chirps_the_lows_late() {
    let n = (0.150 * FS) as usize;
    let mut imp = vec![0.0f32; 8 * n];
    imp[0] = 1.0;
    let mut set = diagonal_settings(1);
    set.push(("bend", 0.5));
    let out = render(&set, &imp);
    let w = &out[2 * n - 400..2 * n + 6_000];
    let k = 1.0 - (-2.0 * core::f32::consts::PI * 1_500.0 / FS).exp();
    let mut lp = 0.0f32;
    let (mut le, mut lte, mut he, mut hte) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (i, x) in w.iter().enumerate() {
        lp += (x - lp) * k;
        let hp = x - lp;
        let (pl, ph) = (f64::from(lp) * f64::from(lp), f64::from(hp) * f64::from(hp));
        le += pl;
        lte += pl * i as f64;
        he += ph;
        hte += ph * i as f64;
    }
    let lag = lte / le.max(1e-30) - hte / he.max(1e-30);
    assert!(
        lag > 300.0,
        "low-band lag {lag:.0} samples — the chirp went missing"
    );

    // smear 0 is a wire: bit-identical to classic at full DIFFUSE.
    let mut lcg: u32 = 0x1234_5678;
    let input: Vec<f32> = (0..24_000)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5
        })
        .collect();
    let classic = render(&[("time", 50.0), ("feedback", 0.9), ("wet", 0.7)], &input);
    let wire = render(
        &[
            ("time", 50.0),
            ("feedback", 0.9),
            ("wet", 0.7),
            ("diffusion", 1.0),
            ("diffuse", 1.0),
            ("bleed", 1.0),
            ("hold", 0.0),
            ("smear", 0.0),
        ],
        &input,
    );
    for (i, (a, b)) in classic.iter().zip(wire.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "smear-0 prism moved bit {i}");
    }
}

// ------------------------------------------------- PRISM conformance

/// PRISM passes energy UNTOUCHED in one pass: a 96-stage allpass
/// line is exactly lossless (measured ratio 1.0000) — its "faster
/// decay" in early probes was time-smearing across measurement
/// windows, never loss.
#[test]
fn prism_passes_energy_untouched() {
    let burst = noise_burst(24_000, 96_000);
    let base: &[(&str, f64)] = &[
        ("time", 50.0),
        ("feedback", 0.0),
        ("dry", 0.0),
        ("wet", 1.0),
        ("diffusion", 1.0),
        ("diffuse", 1.0),
        ("hold", 0.0),
        ("bleed", 0.0),
    ];
    let mut wire = base.to_vec();
    wire.push(("smear", 0.0));
    let mut full = base.to_vec();
    full.push(("smear", 1.0));
    full.push(("bend", 0.3));
    let e_wire = energy(&render(&wire, &burst), 0, usize::MAX);
    let e_full = energy(&render(&full, &burst), 0, usize::MAX);
    let ratio = e_full / e_wire.max(1e-30);
    assert!(
        (0.99..=1.01).contains(&ratio),
        "dispersion changed the energy ({ratio:.4}) — an allpass line must not"
    );
}

/// BEND moves the chirp where it says: a lower bend concentrates
/// the dispersion lower, so the low band lags MORE (measured 1035
/// samples at bend 0.25 vs 671 at 0.75).
#[test]
fn bend_moves_the_chirp() {
    let lag = |bend: f64| -> f64 {
        let mut imp = vec![0.0f32; 96_000];
        imp[0] = 1.0;
        let out = render(
            &[
                ("time", 50.0),
                ("feedback", 0.0),
                ("dry", 0.0),
                ("wet", 1.0),
                ("diffusion", 1.0),
                ("diffuse", 1.0),
                ("hold", 0.0),
                ("bleed", 0.0),
                ("smear", 1.0),
                ("bend", bend),
            ],
            &imp,
        );
        let w = &out[2_000..60_000];
        let k = 1.0 - (-2.0 * core::f32::consts::PI * 1_200.0 / FS).exp();
        let mut lp = 0.0f32;
        let (mut le, mut lte, mut he, mut hte) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (i, x) in w.iter().enumerate() {
            lp += (x - lp) * k;
            let hp = x - lp;
            le += f64::from(lp) * f64::from(lp);
            lte += f64::from(lp) * f64::from(lp) * i as f64;
            he += f64::from(hp) * f64::from(hp);
            hte += f64::from(hp) * f64::from(hp) * i as f64;
        }
        lte / le.max(1e-30) - hte / he.max(1e-30)
    };
    let (low, high) = (lag(0.25), lag(0.75));
    assert!(
        low > high + 200.0,
        "bend 0.25 lags {low:.0} vs 0.75's {high:.0} — the knob lost its meaning"
    );
}

/// CHIRPS OF CHIRPS, measured: under HOLD the field re-disperses
/// every lap, so the ring turns progressively bassier — the highs
/// arrive early and the lows keep receding (measured hp/lp energy
/// 0.055 early → 0.004 late, a 14× tilt).
#[test]
fn chirps_compound_in_the_field() {
    let mut imp = vec![0.0f32; 144_000];
    imp[0] = 1.0;
    let ring = render(
        &[
            ("time", 20.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("diffusion", 1.0),
            ("diffuse", 1.0),
            ("hold", 0.9),
            ("bleed", 0.0),
            ("smear", 1.0),
            ("bend", 0.3),
        ],
        &imp,
    );
    let hp_lp = |w: &[f32]| -> f64 {
        let k = 1.0 - (-2.0 * core::f32::consts::PI * 1_200.0 / FS).exp();
        let mut lp = 0.0f32;
        let (mut le, mut he) = (0.0f64, 0.0f64);
        for x in w {
            lp += (x - lp) * k;
            let hp = x - lp;
            le += f64::from(lp) * f64::from(lp);
            he += f64::from(hp) * f64::from(hp);
        }
        he / le.max(1e-30)
    };
    let early = hp_lp(&ring[5_000..30_000]);
    let late = hp_lp(&ring[60_000..130_000]);
    assert!(
        early > 5.0 * late,
        "ring tilt early {early:.3} vs late {late:.3} — the chirps stopped compounding"
    );
}

// -------------------------------------------------- DUST conformance

/// SPRAY 0 is the COHERENT echo: coincident sum-to-one taps
/// reconstruct the signal to rounding (measured 9e-15 relative).
#[test]
fn dust_at_zero_spray_is_the_coherent_echo() {
    let burst = noise_burst(24_000, 96_000);
    let wire = render(
        &[
            ("time", 50.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
        ],
        &burst,
    );
    let dust = render(
        &[
            ("time", 50.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("diffusion", 2.0),
            ("diffuse", 1.0),
            ("hold", 0.0),
            ("bleed", 0.0),
            ("spray", 0.0),
        ],
        &burst,
    );
    let diff: f64 = wire
        .iter()
        .zip(&dust)
        .map(|(a, b)| f64::from(a - b) * f64::from(a - b))
        .sum();
    let rel = diff / energy(&wire, 0, usize::MAX).max(1e-30);
    assert!(
        rel < 1e-10,
        "coincident cloud diverged from the echo ({rel:.2e})"
    );
}

/// SPRAY widens the cloud as dialed: the scattered echo's temporal
/// spread grows with the knob (measured 115 samples at 0.2 vs 590
/// at 1.0 — and this law is why the cloud is SEEDED OPEN at
/// prepare: the probe caught freshly-loaded walks all at zero, a
/// closed cloud with no scatter for the first second).
#[test]
fn spray_widens_the_cloud() {
    let spread = |spray: f64| -> f64 {
        let mut imp = vec![0.0f32; 24_000];
        imp[0] = 1.0;
        let out = render(
            &[
                ("time", 50.0),
                ("feedback", 0.0),
                ("dry", 0.0),
                ("wet", 1.0),
                ("diffusion", 2.0),
                ("diffuse", 1.0),
                ("hold", 0.0),
                ("bleed", 0.0),
                ("spray", spray),
                ("motes", 1.0),
            ],
            &imp,
        );
        let w = &out[2_000..8_000];
        let (mut e, mut te, mut t2e) = (0.0f64, 0.0f64, 0.0f64);
        for (i, x) in w.iter().enumerate() {
            let p = f64::from(*x) * f64::from(*x);
            e += p;
            te += p * i as f64;
            t2e += p * (i as f64) * (i as f64);
        }
        let c = te / e.max(1e-30);
        (t2e / e.max(1e-30) - c * c).sqrt()
    };
    let (tight, wide) = (spread(0.2), spread(1.0));
    assert!(
        wide > 2.0 * tight && tight > 20.0,
        "cloud spread {tight:.0} → {wide:.0} — spray lost its meaning"
    );
}

// -------------------------------------------- image safety (both)

/// PRISM and DUST never split a centered image — identical cascades
/// and lane-shared cloud positions make L ≡ R a BIT identity for
/// centered program, at any coupling. (PLATE and MESH decorrelate
/// the image ON PURPOSE — their laws live in tail.rs.)
#[test]
fn the_line_and_the_cloud_never_split_the_image() {
    let burst = noise_burst(24_000, 96_000);
    for (kind, knob, val) in [(1usize, "smear", 1.0f64), (2, "spray", 1.0)] {
        let (l, r) = render_lr(
            &[
                ("time", 80.0),
                ("feedback", 0.6),
                ("dry", 0.0),
                ("wet", 1.0),
                ("diffusion", kind as f64),
                ("diffuse", 1.0),
                ("hold", 0.7),
                ("bleed", 0.5),
                ("spread", 0.0),
                ("depth", 0.0),
                (knob, val),
            ],
            &burst,
        );
        for (i, (a, b)) in l.iter().zip(r.iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "voice {kind} split the image at sample {i}"
            );
        }
    }
}

/// DUST never starves the echo train — the regression the user's
/// ears caught: the (1 − hg) loop normalization assumes a field
/// that SUSTAINS, and dust's lossy cloud doesn't, so full bleed
/// replaced healthy repeats with a quarter-scale field and the
/// train died within two repeats (measured −81 dB where classic
/// reads −46). Post-fix anchors (impulse, fb 0.9, time 400 ms,
/// per-second RMS): bleed 0 decays like classic (~9 dB over 4 s);
/// full-everything starts ≥ −70 dB and keeps its first two seconds
/// within 22 dB.
#[test]
fn dust_never_starves_the_train() {
    let mut imp = vec![0.0f32; 6 * 48_000];
    imp[0] = 1.0;
    let seg_db = |out: &[f32], sec: usize| -> f64 {
        let w = &out[sec * 48_000..(sec + 1) * 48_000];
        10.0 * (w.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / w.len() as f64)
            .max(1e-24)
            .log10()
    };
    let base: &[(&str, f64)] = &[
        ("time", 400.0),
        ("feedback", 0.9),
        ("dry", 0.0),
        ("wet", 1.0),
        ("diffusion", 2.0),
        ("spray", 1.0),
        ("motes", 1.0),
    ];
    // Scatter-only (bleed 0): the train decays like the classic law.
    let mut set = base.to_vec();
    set.extend_from_slice(&[("diffuse", 1.0), ("hold", 1.0), ("bleed", 0.0)]);
    let out = render(&set, &imp);
    let drop = seg_db(&out, 0) - seg_db(&out, 4);
    assert!(
        drop < 14.0,
        "dust at bleed 0: {drop:.1} dB over 4 s — the scatter is eating the train"
    );
    // Full everything: quieter and shorter BY DESIGN (the crumble
    // trade), but audible and decaying, never dead.
    let mut set = base.to_vec();
    set.extend_from_slice(&[("diffuse", 1.0), ("hold", 1.0), ("bleed", 1.0)]);
    let out = render(&set, &imp);
    let (s0, s2) = (seg_db(&out, 0), seg_db(&out, 2));
    assert!(
        s0 > -70.0,
        "full-bleed dust starts at {s0:.1} dB — the train starved"
    );
    assert!(
        s2 > s0 - 22.0,
        "full-bleed dust falls {:.1} dB in two seconds — starving again",
        s0 - s2
    );
}

/// SMEAR 0 means NO DISPERSION — including no dispersive FIELD: an
/// ungated hold loop around the identity cascade degenerated into a
/// frame-rate one-pole (+20 dB resonant bass below ~750 Hz,
/// measured). A low tone through smear-0 prism at full HOLD must
/// pass at unity.
#[test]
fn smear_zero_holds_no_resonator() {
    let tone: Vec<f32> = (0..96_000)
        .map(|i| (2.0 * core::f32::consts::PI * 90.0 * i as f32 / 48_000.0).sin() * 0.25)
        .collect();
    let out = render(
        &[
            ("time", 50.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("diffusion", 1.0),
            ("diffuse", 1.0),
            ("hold", 1.0),
            ("smear", 0.0),
        ],
        &tone,
    );
    let e_out: f64 = out[48_000..96_000]
        .iter()
        .map(|v| f64::from(*v) * f64::from(*v))
        .sum();
    let e_in: f64 = tone[48_000..96_000]
        .iter()
        .map(|v| f64::from(*v) * f64::from(*v))
        .sum();
    let gain_db = 10.0 * (e_out / e_in.max(1e-30)).log10();
    assert!(
        gain_db.abs() < 2.0,
        "smear-0 prism at full hold gains {gain_db:.1} dB — the resonator is back"
    );
}

/// Every algorithm HEARS the field: DIFFUSE up changes the wet
/// render on every tab. This law exists because the wide frame
/// signatures let SHIFT accept the sends and drop them on the floor
/// — the DIFFUSE knob was dead on that tab (found by the FrameCtx
/// refactor's bit-pin, 2026-07-06). Silent decoupling can't come
/// back.
#[test]
fn every_algorithm_hears_the_field() {
    let total = 2 * FS as usize;
    let input = noise_burst((0.1 * FS) as usize, total);
    for algo in 0..4 {
        let base = vec![
            ("algo", f64::from(algo)),
            ("time", 120.0),
            ("feedback", 0.6),
            ("wet", 1.0),
            ("dry", 0.0),
            ("hold", 0.6),
            ("bleed", 0.4),
        ];
        let mut off = base.clone();
        off.push(("diffuse", 0.0));
        let mut on = base.clone();
        on.push(("diffuse", 0.9));
        let a = render(&off, &input);
        let b = render(&on, &input);
        let diff: f64 = a
            .iter()
            .zip(&b)
            .map(|(x, y)| f64::from(x - y).powi(2))
            .sum();
        let e: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum();
        assert!(
            diff > e * 1e-4,
            "algo {algo}: DIFFUSE is dead (diff {diff:.3e} vs energy {e:.3e})"
        );
    }
}

/// SHIFT's field bleed is a NEW recirculation (the field's output is
/// re-transposed every lap), so it gets the same sustained-drive
/// trend torture as the diagonal: everything maxed, a resonant tone,
/// energy TREND over 5 s windows 20 s apart must be flat. Both
/// shimmer (+12) and the downward spiral (−12) — the two ends move
/// energy through the spectrum in opposite directions.
#[test]
fn shift_shimmer_field_never_pumps() {
    let policy = StabilityPolicy::default();
    let tone = resonant_tone(FS, &policy);
    for (semis, kind) in [(12.0, 0), (-12.0, 2)] {
        let set = vec![
            ("algo", 2.0),
            ("shift", semis),
            ("splice", 0.5),
            ("time", 120.0),
            ("feedback", 0.95),
            ("wet", 1.0),
            ("dry", 0.0),
            ("diffusion", f64::from(kind)),
            ("diffuse", 1.0),
            ("bleed", 1.0),
            ("hold", 1.0),
        ];
        let out = render(&set, &tone);
        assert_no_pump(&out, FS, &policy, &format!("shift {semis}"));
    }
}
