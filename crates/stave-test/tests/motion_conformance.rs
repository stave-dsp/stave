//! Spectral-motion instruments:
//! calibration against constructions with known answers, the
//! impossibility floor for static chains, and first characterization of
//! the phenomena the existing engine already produces.

use stave_dsp::{shapes, CurveStage, EnvelopeFollower, EnvelopeTimes, Frame, Stage};
use stave_measure::modulation::{
    period_classify, sideband_report, subharmonic_ratio_db, PeriodClass, SidebandConfig,
    SubharmonicConfig,
};

const N: usize = 32_768;
const FS: f64 = 48_000.0;
/// Fundamental bin — even (coherent half-comb), 128 ⇒ f0 = 187.5 Hz,
/// period exactly 256 samples.
const M: usize = 128;

fn tone(i: usize, amp: f64) -> f64 {
    amp * (2.0 * std::f64::consts::PI * M as f64 * i as f64 / N as f64).sin()
}

/// Steady-state one-window capture of `render` under the standard tone.
fn capture<F: FnMut(f64) -> f64>(amp: f64, mut render: F) -> Vec<f64> {
    for i in 0..N {
        let _ = render(tone(i, amp));
    }
    (N..2 * N).map(|i| render(tone(i, amp))).collect()
}

// ---------------------------------------------------------------------------
// Calibration — the instruments must reproduce textbook numbers.
// ---------------------------------------------------------------------------

#[test]
fn sideband_instrument_matches_am_theory() {
    // g(t) = 1 + m·sin(2π·fm·t) on a pure carrier: two sidebands at
    // ±fm, each m/2 of the carrier ⇒ total sideband power 2·(m/2)².
    let m_am = 0.1f64;
    let fm_bin = 32usize; // 46.9 Hz, bin-centred
    let out: Vec<f64> = (0..N)
        .map(|i| {
            let g = 1.0
                + m_am * (2.0 * std::f64::consts::PI * fm_bin as f64 * i as f64 / N as f64).sin();
            g * tone(i, 0.5)
        })
        .collect();
    let rep = sideband_report(
        &out,
        &SidebandConfig {
            carrier_bin: M,
            skirt_bins: 64,
            harmonics: 1,
        },
    );
    let theory_total = 10.0 * (2.0 * (m_am / 2.0) * (m_am / 2.0)).log10();
    let theory_single = 20.0 * (m_am / 2.0).log10();
    println!(
        "AM m={m_am}: total {:.2} dB (theory {:.2}), worst {:?} (theory {:.2} at ±{fm_bin})",
        rep.total_db, theory_total, rep.worst, theory_single
    );
    assert!(
        (rep.total_db - theory_total).abs() < 0.5,
        "total {:.2} vs {:.2}",
        rep.total_db,
        theory_total
    );
    assert!(
        (rep.worst.1 - theory_single).abs() < 0.5,
        "single {:.2} vs {:.2}",
        rep.worst.1,
        theory_single
    );
    assert!(
        rep.worst.0.unsigned_abs() as usize == fm_bin,
        "offset {:?}",
        rep.worst
    );
}

#[test]
fn subharmonic_instrument_calibrates() {
    // Carrier + a −40 dB tone at exactly f0/2.
    let out: Vec<f64> = (0..N)
        .map(|i| {
            tone(i, 0.5)
                + 0.005 * (2.0 * std::f64::consts::PI * (M / 2) as f64 * i as f64 / N as f64).sin()
        })
        .collect();
    let r = subharmonic_ratio_db(
        &out,
        &SubharmonicConfig {
            carrier_bin: M,
            max_order: 8,
        },
    );
    println!("constructed −40 dB sub measures {r:.2} dB");
    assert!((r - (-40.0)).abs() < 0.3, "sub ratio {r:.2} vs −40");
}

#[test]
fn period_detector_calibrates_on_synthetic_period_two() {
    let period = (N / M) as f64; // exactly 256 samples
                                 // Pure sine → Period1.
    let clean: Vec<f64> = (0..N).map(|i| tone(i, 0.5)).collect();
    let r1 = period_classify(&clean, period);
    println!("sine: {:?} d1 {:.1} d2 {:.1}", r1.class, r1.d1_db, r1.d2_db);
    assert_eq!(r1.class, PeriodClass::Period1);
    // Alternate-cycle gain ±10% → Period2 with depth 20·log10(0.1).
    let p2: Vec<f64> = (0..N)
        .map(|i| {
            let cycle = i / (N / M);
            let g = if cycle % 2 == 0 { 1.1 } else { 0.9 };
            g * tone(i, 0.5)
        })
        .collect();
    let r2 = period_classify(&p2, period);
    println!(
        "alt ±10%: {:?} d1 {:.1} d2 {:.1} depth {:.1}",
        r2.class, r2.d1_db, r2.d2_db, r2.period2_depth_db
    );
    assert_eq!(r2.class, PeriodClass::Period2);
    assert!(
        (r2.period2_depth_db - (-20.0)).abs() < 1.0,
        "depth {:.2} vs −20",
        r2.period2_depth_db
    );
}

// ---------------------------------------------------------------------------
// The impossibility floor — a static curve emits NOTHING off the comb.
// ---------------------------------------------------------------------------

#[test]
fn static_curve_has_no_off_comb_energy() {
    let mut cs = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(2.0));
    cs.prepare(FS as f32);
    let out = capture(0.4, |x| cs.tick(x as f32) as f64);
    let sb = sideband_report(
        &out,
        &SidebandConfig {
            carrier_bin: M,
            skirt_bins: 48,
            harmonics: 6,
        },
    );
    let sub = subharmonic_ratio_db(
        &out,
        &SubharmonicConfig {
            carrier_bin: M,
            max_order: 8,
        },
    );
    let per = period_classify(&out, (N / M) as f64);
    println!(
        "static knee: sidebands {:.1} dB, subharmonics {:.1} dB, {:?}",
        sb.total_db, sub, per.class
    );
    assert!(
        sb.total_db < -80.0,
        "static chain grew sidebands: {:.1}",
        sb.total_db
    );
    assert!(sub < -80.0, "static chain grew subharmonics: {:.1}", sub);
    assert_eq!(per.class, PeriodClass::Period1);
}

// ---------------------------------------------------------------------------
// Characterization — what the existing engine already produces.
// ---------------------------------------------------------------------------

/// Mechanism 2 through the REAL Frame path: a sinusoidal pending-gain on
/// a CurveStage produces textbook AM sidebands around the carrier — and
/// proves per-sample frame commits modulate cleanly (no zipper spray).
#[test]
fn frame_gain_lfo_produces_textbook_sidebands() {
    let m_g = 0.1f64;
    let fm_bin = 32usize;
    let mut cs = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(2.0));
    cs.prepare(FS as f32);
    let mut i_abs = 0usize;
    // Small amplitude keeps the knee in its linear region so AM theory
    // applies directly.
    let out = capture(0.15, |x| {
        let g = 1.0
            + m_g * (2.0 * std::f64::consts::PI * fm_bin as f64 * i_abs as f64 / N as f64).sin();
        i_abs += 1;
        cs.set_pending(Frame {
            bias: 0.0,
            drive: 1.0,
            gain: g as f32,
        });
        cs.tick(x as f32) as f64
    });
    let rep = sideband_report(
        &out,
        &SidebandConfig {
            carrier_bin: M,
            skirt_bins: 48,
            harmonics: 1,
        },
    );
    let theory_total = 10.0 * (2.0 * (m_g / 2.0) * (m_g / 2.0)).log10();
    println!(
        "frame-gain LFO: H1 skirt {:.2} dB (theory {:.2}), worst {:?}",
        rep.total_db, theory_total, rep.worst
    );
    assert!(
        (rep.total_db - theory_total).abs() < 0.75,
        "frame modulation should match AM theory: {:.2} vs {:.2}",
        rep.total_db,
        theory_total
    );
    assert_eq!(
        rep.worst.0.unsigned_abs() as usize,
        fm_bin,
        "sidebands at ±f_lfo"
    );
}

/// The defining compressor behaviour, measured spectrally: envelope-
/// tracking counter-gain FLATTENS the program's amplitude modulation —
/// the first-order sidebands of an AM program shrink as depth rises.
#[test]
fn envelope_compression_flattens_program_sidebands() {
    let fm_bin = 5usize; // 7.3 Hz — envelope-rate program motion
    let band_db = |out: &[f64], b: usize| -> f64 {
        let n = out.len();
        let mut re = out.to_vec();
        let mut im = vec![0.0; n];
        stave_measure::spectrum::fft(&mut re, &mut im);
        let pow = |k: usize| re[k] * re[k] + im[k] * im[k];
        let carrier = pow(M).max(1e-300);
        10.0 * ((pow(b) + pow(2 * M - b)) / carrier).max(1e-30).log10()
    };
    let render = |depth: f64| -> Vec<f64> {
        let mut cs = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(2.0));
        cs.prepare(FS as f32);
        let mut env = EnvelopeFollower::new(EnvelopeTimes {
            attack_ms: 2.0,
            release_ms: 6.0,
            slow_release_ms: None,
        });
        env.prepare(FS as f32);
        let mut i_abs = 0usize;
        let mut f = move |_x: f64| {
            let am = 1.0
                + 0.5
                    * (2.0 * std::f64::consts::PI * fm_bin as f64 * i_abs as f64 / N as f64).cos();
            let x = am * tone(i_abs, 0.3);
            i_abs += 1;
            // Hot detector (input-gain into the sidechain) so the loop
            // has authority over the 50% program modulation.
            let e = env.tick((x.abs() * 3.0) as f32) as f64;
            let g = (1.0 - depth * e).max(0.05);
            cs.set_pending(Frame {
                bias: 0.0,
                drive: 1.0,
                gain: g as f32,
            });
            cs.tick(x as f32) as f64
        };
        for _ in 0..N {
            let _ = f(0.0);
        }
        (0..N).map(|_| f(0.0)).collect()
    };
    let quiet = band_db(&render(0.0), M + fm_bin);
    let deep = band_db(&render(0.6), M + fm_bin);
    println!("program ±fm sidebands: depth 0 → {quiet:.2} dB, depth 0.6 → {deep:.2} dB");
    assert!(
        deep < quiet - 2.0,
        "compression should flatten program AM: {quiet:.2} → {deep:.2}"
    );
}

/// Step 2: the LFO driver source — ghost sidebands as a route recipe.
/// Env-scaling gates them with program heat (ripple ∝ current draw).
#[test]
fn lfo_driver_gain_route_makes_gated_ghost_sidebands() {
    use stave_dsp::{Chain, Driver, ModMatrix, Route, StageKind, StageTarget, Target};
    let f_lfo = 32.0 * FS / N as f64; // bin-centred 46.875 Hz
    let render = |amount: f32, amp: f64| -> Vec<f64> {
        let mut ch = Chain::<2>::new();
        let id = ch
            .push(StageKind::Curve(CurveStage::from_fn(
                -4.0,
                4.0,
                0.0,
                shapes::knee(2.0),
            )))
            .unwrap();
        ch.prepare(FS as f32);
        let mut mm = ModMatrix::<1, 2>::new([Driver::lfo_env_scaled(f_lfo as f32, 5.0, 60.0)]);
        mm.prepare(FS as f32);
        mm.push_route(Route {
            source: 0,
            target: StageTarget {
                stage: id,
                target: Target::Gain,
            },
            amount,
        })
        .unwrap();
        let base = [(id, Frame::NEUTRAL)];
        capture(amp, |x| {
            mm.tick(&[x.abs() as f32], &mut ch, &base, &[]);
            ch.tick(x as f32) as f64
        })
    };
    let sbr = |out: &[f64]| {
        sideband_report(
            out,
            &SidebandConfig {
                carrier_bin: M,
                skirt_bins: 48,
                harmonics: 1,
            },
        )
    };
    let hot = sbr(&render(0.5, 0.4));
    let small = sbr(&render(0.05, 0.4));
    let quiet = sbr(&render(0.5, 0.04));
    println!(
        "ghost sidebands: hot {:.1} dB (worst {:?}), amount/10 {:.1} dB, program −20 dB {:.1} dB",
        hot.total_db, hot.worst, small.total_db, quiet.total_db
    );
    assert!(
        hot.total_db > -40.0,
        "LFO route should make audible sidebands: {:.1}",
        hot.total_db
    );
    assert_eq!(
        hot.worst.0.unsigned_abs(),
        32,
        "sidebands at ±f_lfo: {:?}",
        hot.worst
    );
    // Route amount is a clean dB axis…
    assert!(
        ((hot.total_db - small.total_db) - 20.0).abs() < 1.5,
        "10× amount should be +20 dB: {:.1} vs {:.1}",
        hot.total_db,
        small.total_db
    );
    // …and env-scaling gates with program level (−20 dB in → about −20 dB
    // of sideband ratio, since the envelope rides the input).
    assert!(
        quiet.total_db < hot.total_db - 15.0,
        "quiet program should gate the ripple: {:.1} vs {:.1}",
        quiet.total_db,
        hot.total_db
    );
}

/// Step 3: the SubOctave, both roles measured with Step 1's
/// instruments — mixed as a SIGNAL it puts the program on the half
/// comb (Period2 by construction); as a DRIVER on a gain route it puts
/// energy at 1.5·f0 / 2.5·f0, between the integer harmonics.
#[test]
fn sub_octave_populates_the_half_integer_comb() {
    use stave_dsp::{Chain, Driver, ModMatrix, Route, StageKind, StageTarget, SubOctave, Target};
    // --- Signal role: program + sub mix ---
    let mut sub = SubOctave::new();
    sub.prepare(FS as f32);
    let mixed = capture(0.4, |x| {
        let s = sub.tick(x as f32) as f64;
        x + 0.5 * s
    });
    let ratio = subharmonic_ratio_db(
        &mixed,
        &SubharmonicConfig {
            carrier_bin: M,
            max_order: 8,
        },
    );
    let per = period_classify(&mixed, (N / M) as f64);
    println!(
        "sub mix: half-comb {ratio:.1} dBc, {:?} (d1 {:.1}, d2 {:.1})",
        per.class, per.d1_db, per.d2_db
    );
    assert!(
        ratio > -12.0,
        "mixed sub should dominate the half comb: {ratio:.1}"
    );
    assert_eq!(
        per.class,
        PeriodClass::Period2,
        "octave-down mix IS period-2"
    );

    // --- Driver role: sub-rate gain modulation between the harmonics ---
    let mut ch = Chain::<2>::new();
    let id = ch
        .push(StageKind::Curve(CurveStage::from_fn(
            -4.0,
            4.0,
            0.0,
            shapes::knee(2.0),
        )))
        .unwrap();
    ch.prepare(FS as f32);
    let mut mm = ModMatrix::<1, 2>::new([Driver::sub()]);
    mm.prepare(FS as f32);
    mm.push_route(Route {
        source: 0,
        target: StageTarget {
            stage: id,
            target: Target::Gain,
        },
        amount: 0.6,
    })
    .unwrap();
    let base = [(id, Frame::NEUTRAL)];
    let out = capture(0.4, |x| {
        // Signed signal as the divider's detector.
        mm.tick(&[x as f32], &mut ch, &base, &[]);
        ch.tick(x as f32) as f64
    });
    let comb = subharmonic_ratio_db(
        &out,
        &SubharmonicConfig {
            carrier_bin: M,
            max_order: 8,
        },
    );
    println!("sub-driven gain route: half-comb {comb:.1} dBc");
    assert!(
        comb > -30.0,
        "sub-rate gain modulation should energize the half comb: {comb:.1}"
    );
}

/// First pass: the bifurcation scan. Does the NFB chain show
/// period multiplication anywhere before its measured oscillation wall
/// (β ≈ −0.6)? Also adds a reactive element (a slew stage in the loop)
/// which is the classic period-doubling ingredient. Findings printed;
/// the assertion is methodological (bounded, classified output in the
/// working range).
#[test]
fn feedback_chain_bifurcation_scan() {
    use stave_dsp::{Chain, Slew, StageKind};
    let scan = |reactive: bool| {
        println!(
            "-- loop {} --",
            if reactive {
                "with slew (reactive)"
            } else {
                "memoryless"
            }
        );
        for &beta in &[-0.30f32, -0.40, -0.45, -0.50, -0.55] {
            let mut ch = Chain::<3>::new();
            let mut a = CurveStage::from_fn(-8.0, 8.0, 0.0, shapes::knee(2.0));
            a.set_pending(Frame {
                bias: 0.05,
                drive: 3.0,
                gain: 1.0,
            });
            let id_a = ch.push(StageKind::Curve(a)).unwrap();
            if reactive {
                ch.push(StageKind::Slew(Slew::new(2.0, 2.0))).unwrap();
            }
            let mut b = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(4.0));
            b.set_pending(Frame {
                bias: 0.0,
                drive: 1.2,
                gain: 1.0,
            });
            let id_b = ch.push(StageKind::Curve(b)).unwrap();
            ch.set_feedback(beta, id_b, id_a, None);
            ch.prepare(FS as f32);
            let out = capture(0.7, |x| ch.tick(x as f32) as f64);
            let per = period_classify(&out, (N / M) as f64);
            let peak = out.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
            println!(
                "  β {beta:+.2}: {:?} (d1 {:.1} dB, d2 {:.1} dB, peak {peak:.2})",
                per.class, per.d1_db, per.d2_db
            );
            assert!(peak.is_finite() && peak < 8.0, "loop unbounded at β {beta}");
            assert_ne!(
                per.class,
                PeriodClass::Aperiodic,
                "working range must stay classified at β {beta}"
            );
        }
    };
    scan(false);
    scan(true);
}
