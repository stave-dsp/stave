//! Conformance: the saturation engines against their audible-band alias
//! and IMD limits, plus wet/dry time alignment.
//!
//! Limits are enforced on the AUDIBLE band (≤20 kHz) — see the metric-
//! library docs for why full-band figures over-count the half-band
//! decimator's deliberate transition-shoulder trade. Both numbers are
//! printed so the trade stays visible.

use stave_dsp::{
    EnvelopeFollower, EnvelopeTimes, Oversampler2xFir, Oversampler2xFirInner, SmoothSat, Triode,
};
use stave_measure::spectrum::goertzel;
use stave_measure::tone_test::{ToneTest, TwoToneTest};
use stave_multiband_comp_vst3::saturation::{BandSaturator, SaturationCharacter};

/// Triode + 1st-order LUT-ADAA inside a single 2× FIR half-band, swept
/// over bright tones and hot drives — the configuration whose old
/// (Butterworth) ceiling was −31…−41 dB *full-band*. The audible-band
/// residual is the ADAA machinery's own inner-rate fold, measured here.
#[test]
fn triode_2x_audible_alias_matrix() {
    let fs = 48_000.0;
    let n = 8192usize;
    let mut worst = -300.0f64;
    for (label, m) in [("~5 kHz", 853usize), ("~7 kHz", 1195), ("~9 kHz", 1536)] {
        // Grid volts per unit: hot (25 V) and extreme (110 V).
        for drive in [25.34, 110.4] {
            let mut t = Triode::new();
            // Rate contract: the triode ticks at 2× inside the oversampler.
            t.prepare(fs as f32 * 2.0);
            t.set_grid_drive(drive);
            let mut os = Oversampler2xFir::new();
            os.prepare(fs as f32);
            let r = ToneTest {
                bin: m,
                window: n,
                sample_rate: fs,
                amplitude: 0.5,
                warmup_windows: 1,
            }
            .alias(|x: f32| os.process_one(x, |s| t.process_sample(s)));
            println!(
                "triode 2×  {label} drive {drive:>5.1} V: audible {:>6.1} dB (full {:>6.1} dB)",
                r.audible_db, r.full_db
            );
            worst = worst.max(r.audible_db);
        }
    }
    // Measured worst −33.4 dB (~9 kHz @ 110 V drive) — the 1st-order-ADAA
    // @2× inner-rate residual (harmonics folding around the 96 kHz inner
    // Nyquist back into band), NOT the decimator: FIR and IIR measure the
    // same audible floor here. This is the honest single-2× ceiling; the
    // production BandSaturator path runs 4× (next test). Regression limit with
    // ~3 dB margin.
    assert!(
        worst < -30.0,
        "triode 2× audible alias regressed: {worst:.1} dB"
    );
}

/// The full production tube path: BandSaturator (nested 4× FIR + ADAA triode
/// + level makeup), hot and bright. This is the headline number.
#[test]
fn saturator_tube_4x_audible_alias() {
    let fs = 48_000.0;
    let n = 8192usize;
    // Two tiers: "hot but musical" (≤12 dB drive) and "extreme fuzz"
    // (20 dB — grid swings of ~100 V; the output is near-square, and a
    // 7–9 kHz near-square is the worst case any saturator faces).
    let mut worst_musical = -300.0f64;
    let mut worst_extreme = -300.0f64;
    for (label, m) in [("~5 kHz", 853usize), ("~7 kHz", 1195), ("~9 kHz", 1536)] {
        for drive in [12.0f32, 20.0] {
            let mut sat = BandSaturator::new();
            sat.prepare(fs as f32);
            sat.set_type(SaturationCharacter::Tube);
            sat.set_mix(1.0);
            sat.set_drive_db(drive);
            // Warm-up: 2 windows > the 80 ms makeup-envelope release and
            // the 99-sample latency, so the capture is steady-state.
            let r = ToneTest {
                bin: m,
                window: n,
                sample_rate: fs,
                amplitude: 0.5,
                warmup_windows: 2,
            }
            .alias(|x: f32| sat.process(x));
            println!(
                "saturator 4× {label} drive {drive:>2.0} dB: audible {:>6.1} dB (full {:>6.1} dB)",
                r.audible_db, r.full_db
            );
            if drive <= 12.0 {
                worst_musical = worst_musical.max(r.audible_db);
            } else {
                worst_extreme = worst_extreme.max(r.audible_db);
            }
        }
    }
    // Measured: musical −56.4 dB, extreme −46.8 dB (integrated audible
    // alias power — a stricter figure than any per-bin floor).
    assert!(
        worst_musical < -52.0,
        "saturator 4× musical-drive alias regressed: {worst_musical:.1} dB"
    );
    assert!(
        worst_extreme < -43.0,
        "saturator 4× extreme-drive alias regressed: {worst_extreme:.1} dB"
    );
}

/// Velvet's engine: SmoothSat through the same nested 4× FIR stack, at
/// its default-ish settings, on a bright tone.
#[test]
fn smoothsat_4x_audible_alias() {
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 1195usize; // ~7 kHz
    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    s.set_drive(1.6);
    s.set_smoothness(0.7);
    s.set_hf_smoothing(4.0);
    let mut outer = Oversampler2xFir::new();
    outer.prepare(fs as f32);
    let mut inner = Oversampler2xFirInner::new();
    inner.prepare(fs as f32 * 2.0);
    let r = ToneTest {
        bin: m,
        window: n,
        sample_rate: fs,
        amplitude: 0.7,
        warmup_windows: 2,
    }
    .alias(|x: f32| {
        let pre = s.process_pre(x);
        let shaped = outer.process_one(pre, |a| inner.process_one(a, |b| s.shape(b)));
        s.process_post(shaped)
    });
    println!(
        "smoothsat 4× ~7 kHz: audible {:.1} dB (full {:.1} dB)",
        r.audible_db, r.full_db
    );
    // Measured −56.6 dB. Note the metric: INTEGRATED alias power over all
    // ~2000 non-harmonic audible bins — roughly 30 dB stricter than the
    // per-bin midpoint floors the stave-dsp unit tests quote (−75 dB/bin
    // there and −57 dB integrated here describe the same spectrum).
    assert!(
        r.audible_db < -52.0,
        "smoothsat audible alias regressed: {:.1} dB",
        r.audible_db
    );
}

/// Wet/dry time alignment: the BandSaturator's dry-compensation delay must
/// place both legs of the mix at the same instant, and the early-out
/// (drive 0 / bypass) must take the SAME delay so latency is constant
/// across settings.
#[test]
fn saturator_wet_dry_and_bypass_are_time_aligned() {
    let fs = 48_000.0f32;
    let lat = {
        let mut sat = BandSaturator::new();
        sat.prepare(fs);
        sat.latency_samples() as usize
    };
    // Peak index of an impulse response for a given config.
    let peak_index = |mix: f32, drive: f32| -> usize {
        let mut sat = BandSaturator::new();
        sat.prepare(fs);
        sat.set_type(SaturationCharacter::Tape); // linear below its 0.7 knee
        sat.set_mix(mix);
        sat.set_drive_db(drive);
        let mut best = (0usize, 0.0f32);
        for i in 0..4 * lat {
            let x = if i == 0 { 0.5 } else { 0.0 };
            let y = sat.process(x);
            if y.abs() > best.1 {
                best = (i, y.abs());
            }
        }
        best.0
    };
    let dry_path = peak_index(0.0, 6.0); // mix 0 → early-out (delayed dry)
    let wet_path = peak_index(1.0, 0.01); // ~linear wet through the 4× stack
    let half_mix = peak_index(0.5, 0.01);
    println!("impulse peaks — dry {dry_path}, wet {wet_path}, 50/50 {half_mix}, reported {lat}");
    assert_eq!(dry_path, lat, "bypass/dry path latency != reported");
    assert_eq!(wet_path, lat, "wet path latency != reported");
    assert_eq!(half_mix, lat, "mixed path latency != reported");
}

/// IMD conformance. CCIF (19+20 kHz-style difference tone) is the audible
/// failure mode for bright material; SmoothSat's HF companding exists to
/// crush it — that claim gets a number and a limit here.
#[test]
fn smoothsat_companding_cuts_ccif_imd() {
    let fs = 48_000.0;
    let n = 8192usize;
    let (m1, m2) = (1707usize, 1878usize); // ~10.0 / ~11.0 kHz, Δ ≈ 1 kHz
    let imd_for = |hf: f32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.8);
        s.set_smoothness(0.6);
        s.set_hf_smoothing(hf);
        TwoToneTest {
            low_bin: m1,
            high_bin: m2,
            window: n,
            amplitude: 0.4,
            warmup_windows: 1,
        }
        .ccif_db(|x: f32| s.process(x))
    };
    let off = imd_for(0.0);
    let on = imd_for(10.0);
    println!(
        "CCIF 1 kHz difference tone: hf=0 {off:.1} dB | hf=10 {on:.1} dB (Δ {:+.1})",
        on - off
    );
    assert!(
        on < off - 12.0,
        "companding should cut CCIF IMD ≥12 dB: {off:.1} → {on:.1}"
    );
}

/// The flagship convergence pattern (Ember): a Chebyshev shaper whose
/// operating point is driven by the TubeBias memory model. The claim —
/// "it breathes" — must be measurable: with an ODD-only curve (no static
/// even weight), the 2nd harmonic can ONLY come from the memory-driven
/// bias, so under sustained level h2 must bloom from ~nothing, and the
/// model must recover in silence.
#[test]
fn memory_driven_bias_blooms_even_harmonics_and_recovers() {
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 171usize; // ~1 kHz, bin-centred
    let f0 = m as f64 * fs / n as f64;

    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    s.set_drive(2.0);
    s.set_even(0.0); // odd-only: any h2 is the breathing, not the recipe
    s.set_odd(0.25);
    s.set_smoothness(0.7);
    s.set_hf_smoothing(0.0);
    // TubeBias as data: EnvelopeFollower(0.35·τ, τ) + the coupling constants
    let mut mem = EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: 0.35 * 120.0,
        release_ms: 120.0,
        slow_release_ms: None,
    });
    mem.prepare(fs as f32);
    let depth = 2.0f32;

    let step = |x: f32, mem: &mut EnvelopeFollower, s: &mut SmoothSat| -> f32 {
        let st = (mem.tick(x.abs()) * depth).min(2.0);
        s.set_bias((st * 0.3).clamp(0.0, 0.75));
        s.process(x) * (1.0 - 0.33 * st).max(0.0)
    };
    let sig = |i: usize| {
        0.6 * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32
    };

    // The honest null: the same recipe with the model disabled. Odd-only
    // curve at bias 0 is symmetric ⇒ essentially no h2; anything the
    // settled breathing adds above that floor is unambiguously the model.
    let h2_of = |out: &[f64]| goertzel(out, (2.0 * f0) / fs) / goertzel(out, f0 / fs).max(1e-12);
    let h2_off = {
        let mut s0 = SmoothSat::new();
        s0.prepare(fs as f32);
        s0.set_drive(2.0);
        s0.set_even(0.0);
        s0.set_odd(0.25);
        s0.set_smoothness(0.7);
        s0.set_hf_smoothing(0.0);
        let out: Vec<f64> = (0..2 * n)
            .map(|i| s0.process(sig(i)) as f64)
            .skip(n)
            .collect();
        20.0 * h2_of(&out).log10()
    };
    // Settle the model (τ = 120 ms ⇒ 5 windows ≈ 0.85 s ≫ 5τ), capture.
    let mut settled = vec![0.0f64; n];
    for i in 0..6 * n {
        let y = step(sig(i), &mut mem, &mut s) as f64;
        if i >= 5 * n {
            settled[i - 5 * n] = y;
        }
    }
    let h2_on = 20.0 * h2_of(&settled).log10();
    println!("breathing h2/h1: breath off {h2_off:.1} dB → settled breath {h2_on:.1} dB");
    assert!(
        h2_off < -50.0,
        "odd-only recipe should have ~no static h2: {h2_off:.1} dB"
    );
    // Measured: −115 dB static → −35 dB settled (the slope-normalised
    // fundamental reference sits ~9 dB higher than pre-normalisation, so
    // the absolute figure moved; the RELATIVE bloom is the claim).
    assert!(
        h2_on > h2_off + 20.0 && h2_on > -42.0,
        "sustained level should bloom a clear 2nd harmonic: {h2_off:.1} → {h2_on:.1} dB"
    );

    // Recovery: in silence the model must come home (state → ~0 within a
    // few time constants) so quiet passages return to the dialed recipe.
    for _ in 0..(fs * 1.0) as usize {
        let _ = step(0.0, &mut mem, &mut s);
    }
    assert!(
        mem.state().abs() < 0.02,
        "memory model did not recover in silence: {}",
        mem.state()
    );
}

/// The smoothness floor (DECAY_MAX in SmoothSat), characterised honestly.
/// Fold-back (non-monotonic transfer) has TWO distinct sources:
///
///  - **The tail** (silk's domain): the old flat-ladder law folded ~0.25
///    across wide smoothness ranges. With the floor, moderate recipes
///    fold ≤ 0.12 at smoothness 0 and are monotonic from mid-knob up.
///  - **Strong even weight** (warmth's domain, silk-independent): a
///    −4 dB 2nd harmonic mathematically REQUIRES bending the negative
///    lobe back (rectification). Bounded, not eliminated — it is the
///    sound the setting asks for.
#[test]
fn smoothness_fold_back_is_bounded_and_vanishes() {
    // Total descent (cumulative fold-back) of the transfer over x ∈ [−2, 2].
    let fold_back = |even: f32, odd: f32, smooth: f32| -> f32 {
        let mut s = SmoothSat::new();
        s.prepare(48_000.0);
        s.set_even(even);
        s.set_odd(odd);
        s.set_smoothness(smooth);
        s.set_hf_smoothing(0.0);
        // Cumulative fold-back = sum of NEGATIVE increments (total
        // vertical descent), not per-sample valley depth.
        let mut last = f32::NEG_INFINITY;
        let mut total = 0.0f32;
        for i in 0..=1600 {
            let x = -2.0 + 4.0 * i as f32 / 1600.0;
            let y = s.shape(2.05 * x);
            if last.is_finite() && y < last {
                total += last - y;
            }
            last = y;
        }
        total
    };
    // Moderate recipes (defaults, balanced): the fold-back is the TAIL's
    // doing, so it must be bounded at low smoothness and VANISH by
    // mid-knob — this is what the DECAY_MAX floor bought (the old law
    // folded ~0.25 across wide ranges and never vanished).
    for &(even, odd) in &[(0.21f32, 0.12f32), (0.3, 0.3)] {
        let mut worst = 0.0f32;
        for step in 0..=10 {
            worst = worst.max(fold_back(even, odd, step as f32 * 0.099));
        }
        println!("fold-back (moderate): even {even} odd {odd} worst {worst:.3}");
        assert!(
            worst < 0.15,
            "moderate-recipe fold-back regressed: {worst:.3}"
        );
        let mid = fold_back(even, odd, 0.5);
        assert!(mid < 5e-3, "fold-back should vanish by mid-knob: {mid:.2e}");
        let top = fold_back(even, odd, 0.9);
        assert!(top < 2e-3, "top of knob must be monotonic: {top:.2e}");
    }
    // Extreme even weight folds REGARDLESS of smoothness — that is what
    // a −4 dB 2nd harmonic IS (d/du[u + 0.6·T₂] < 0 below u ≈ −0.42:
    // rectification). Not silk's doing and not new; bound it so the
    // shape stays a bend, not a runaway.
    let mut worst_even = 0.0f32;
    for step in 0..=10 {
        worst_even = worst_even.max(fold_back(0.6, 0.0, step as f32 * 0.099));
    }
    // Measured 1.41: warmth at maximum IS a wavefolder on the negative
    // lobe (the parabolic T₂ component dominates the linear term). The
    // bound catches runaway, not the character.
    println!("fold-back (pure even 0.6, inherent rectification): worst {worst_even:.3}");
    assert!(
        worst_even < 2.0,
        "pure-even fold-back runaway: {worst_even:.3}"
    );
    // Pure odd never folds (all-odd curves are monotone S-shapes).
    let mut worst_odd = 0.0f32;
    for step in 0..=10 {
        worst_odd = worst_odd.max(fold_back(0.0, 0.6, step as f32 * 0.099));
    }
    println!("fold-back (pure odd 0.6): worst {worst_odd:.2e}");
    assert!(
        worst_odd < 2e-3,
        "pure-odd recipe should be monotonic: {worst_odd:.2e}"
    );
}

/// SMPTE-style low+high twin tone through the production tube path.
/// A driven tube is SUPPOSED to intermodulate — a loud low tone swings
/// the operating point and amplitude-modulates everything above it
/// (measured: the 7 kHz carrier's sidebands reach ~+2 dB of the carrier
/// at drive 12 — that's the vari-mu physics, not a defect). What must
/// hold: IMD ~vanishes at zero drive (the stage is calibrated ~linear
/// there) and grows monotonically with drive.
#[test]
fn saturator_tube_smpte_imd_grows_with_drive_from_clean() {
    let fs = 48_000.0f32;
    let n = 8192usize;
    let (m1, m2) = (20usize, 1195usize); // ~117 Hz + ~7 kHz, 4:1
    let mut profile = vec![];
    for drive in [0.0f32, 6.0, 12.0] {
        let mut sat = BandSaturator::new();
        sat.prepare(fs);
        sat.set_type(SaturationCharacter::Tube);
        sat.set_mix(1.0);
        sat.set_drive_db(drive);
        let imd = TwoToneTest {
            low_bin: m1,
            high_bin: m2,
            window: n,
            amplitude: 0.5,
            warmup_windows: 2,
        }
        .smpte_db(|x: f32| sat.process(x));
        println!("tube 4× SMPTE IMD (117 Hz + 7 kHz, 4:1) drive {drive:>2.0}: {imd:.1} dB");
        profile.push(imd);
    }
    assert!(
        profile[0] < -30.0,
        "tube should be near-linear at zero drive: {:.1} dB",
        profile[0]
    );
    assert!(
        profile[0] < profile[1] && profile[1] < profile[2],
        "SMPTE IMD should grow with drive: {profile:?}"
    );
}

/// Step 6: the SmoothSat → CurveStage bridge is FAITHFUL — the tabulated
/// stage (ADAA off, so this isolates the table) matches the analytic
/// shape across the recipe matrix to interpolation precision. This is
/// the unification contract: one representation, no drift.
#[test]
fn smoothsat_curve_stage_bridge_is_faithful() {
    use stave_dsp::Stage as _;
    let mut worst = 0.0f64;
    for &(even, odd, smooth) in &[
        (0.15f32, 0.10f32, 0.7f32),
        (0.6, 0.0, 0.0),
        (0.0, 0.6, 0.9),
        (0.21, 0.12, 0.3),
    ] {
        let mut s = SmoothSat::new();
        s.prepare(48_000.0);
        s.set_even(even);
        s.set_odd(odd);
        s.set_smoothness(smooth);
        s.set_hf_smoothing(0.0);
        let mut cs = s.to_curve_stage();
        cs.set_adaa(false);
        cs.prepare(48_000.0);
        for i in 0..=600 {
            let x = -3.0 + 6.0 * i as f64 / 600.0;
            let want = s.shape(x as f32) as f64;
            let got = cs.tick(x as f32) as f64;
            worst = worst.max((got - want).abs());
        }
    }
    println!("bridge worst deviation: {worst:.2e}");
    assert!(
        worst < 5e-4,
        "tabulated stage drifted from analytic: {worst:.2e}"
    );
}
