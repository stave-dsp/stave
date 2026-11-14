use super::*;
use crate::FilterSpec;
use crate::units::{Db, Hz};

/// The drive positions these tests were written against — a 0–20 dB
/// knob on the anchored-exponential law the plugins use — as grid
/// volts per unit.
fn knob(drive_db: f32) -> f64 {
    let position = (f64::from(drive_db) / 20.0).clamp(0.0, 1.0);
    0.4 + 110.0 * (10f64.powf(1.5 * position) - 1.0) / (10f64.powf(1.5) - 1.0)
}

/// Self-biased through its cathode, bypass cap sized for an `rc_ms`
/// time constant.
fn self_biased(t: &mut Triode, rc_ms: f64) {
    let bypass_farads = rc_ms * 1e-3 / t.cathode_ohms();
    t.set_bias(Bias::Cathode { bypass_farads });
}

fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
    let n = buf.len();
    let w = 2.0 * core::f64::consts::PI * f / fs;
    let cw = w.cos();
    let coeff = 2.0 * cw;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in buf {
        let s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let re = s1 - s2 * cw;
    let im = s2 * w.sin();
    (re * re + im * im).sqrt() / (n as f64 * 0.5)
}
fn rms(buf: &[f32]) -> f64 {
    (buf.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / buf.len() as f64).sqrt()
}

// ===== ADAA tests =====

/// In-place forward FFT (f64) over split parts, through the crate's
/// [`Fft`](crate::Fft). N must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    use crate::primitives::spectral::Complex;
    let mut data: Vec<Complex<f64>> = re
        .iter()
        .zip(im.iter())
        .map(|(&r, &i)| Complex::new(r, i))
        .collect();
    crate::Fft::new(re.len()).forward(&mut data);
    for ((r, i), z) in re.iter_mut().zip(im.iter_mut()).zip(&data) {
        *r = z.re;
        *i = z.im;
    }
}

/// Alias-to-signal ratio in dB for a bin-centred sine through `run`.
/// Energy at exact harmonic bins (m·k) is "signal"; all other bins in
/// (0, N/2) are alias + noise. Lower dB = cleaner. `m` is the
/// fundamental's FFT bin (so the tone is exactly periodic — no leakage).
fn alias_to_signal_db<F: FnMut(f32) -> f32>(
    m: usize,
    n: usize,
    fs: f64,
    amp: f32,
    mut run: F,
) -> f64 {
    // warm up, then capture one periodic block
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        let _ = run(s);
    }
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        re[i] = run(s) as f64;
    }
    let _ = fs;
    fft(&mut re, &mut im);
    let (mut sig, mut alias) = (0.0f64, 0.0f64);
    for b in 1..n / 2 {
        let p = re[b] * re[b] + im[b] * im[b];
        if b % m == 0 {
            sig += p;
        } else {
            alias += p;
        }
    }
    10.0 * (alias / sig.max(1e-30)).log10()
}

/// Like `noise_floor_db` but classifying only bins at or below
/// `max_bin` as noise. Half-band decimators intentionally trade the
/// transition shoulder (which folds only onto the inaudible top,
/// 20 kHz–Nyquist) for a flat passband — so the enforceable claim is
/// the AUDIBLE-band floor, and a full-band figure over-counts by
/// design. Callers pass `max_bin` = 20 kHz's bin.
fn noise_floor_db_upto<F: FnMut(f32) -> f32>(
    m: usize,
    n: usize,
    amp: f32,
    max_bin: usize,
    mut run: F,
) -> f64 {
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        let _ = run(s);
    }
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        re[i] = run(s) as f64;
    }
    fft(&mut re, &mut im);
    let (mut sig, mut noise) = (0.0f64, 0.0f64);
    for b in 1..n / 2 {
        let p = re[b] * re[b] + im[b] * im[b];
        if b % m == 0 {
            sig += p;
        } else if b <= max_bin {
            noise += p;
        }
    }
    10.0 * (noise / sig.max(1e-30)).log10()
}

/// Noise floor (non-harmonic, non-DC energy ÷ fundamental) in dB at a
/// LOW fundamental, where a memoryless nonlinearity produces NO
/// aliasing — so any non-harmonic energy is artifact (crackle/noise)
/// injected by the ADAA machinery itself. `m` = fundamental bin.
fn noise_floor_db<F: FnMut(f32) -> f32>(m: usize, n: usize, amp: f32, mut run: F) -> f64 {
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        let _ = run(s);
    }
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    for i in 0..n {
        let s = amp * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
        re[i] = run(s) as f64;
    }
    fft(&mut re, &mut im);
    let (mut sig, mut noise) = (0.0f64, 0.0f64);
    for b in 1..n / 2 {
        let p = re[b] * re[b] + im[b] * im[b];
        if b % m == 0 {
            sig += p;
        } else {
            noise += p;
        }
    }
    10.0 * (noise / sig.max(1e-30)).log10()
}

#[test]
fn adaa_noise_floor_stays_inaudible() {
    // Regression guard for the medium-drive "crackle": at a low
    // fundamental there is no aliasing, so any non-harmonic energy is
    // ADAA artifact. The old 2nd-order LUT path injected a −16…−36 dB
    // floor here (clearly audible). 1st-order must keep it well below
    // audibility across the whole drive range.
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 38usize; // ~222 Hz
    let amp = 0.5f32;
    for &drv in &[0.0f32, 6.0, 12.0, 18.0, 24.0] {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(drv));
        let floor = noise_floor_db(m, n, amp, move |x| t.tick(x));
        println!("drive {drv:>4.0} dB | ADAA noise floor {floor:.1} dB");
        assert!(
            floor < -50.0,
            "ADAA injecting audible noise (crackle) at {drv} dB: floor {floor} dB"
        );
    }
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn decimator_vs_adaa_order_headroom() {
    // Which lever has more headroom? Compare, at the worst-case
    // (hot+bright) points, what each upgrade buys over the current
    // shipped path (1st-order ADAA + 2x biquad):
    //   A) ADAA order 1 → 2 (with the same 2x biquad)
    //   B) more oversampling, same biquad (2x → 4x) — proxy for "is the
    //      biquad transition band the wall?" If 4x≈2x, the FILTER is the
    //      limit and a steep decimator is the high-impact fix.
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 8192usize;
    let amp = 0.5f32;
    let asr = |order: AdaaOrder, factor: u8, m: usize, drv: f32| -> f64 {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(drv));
        t.set_adaa_order(order);
        let mut os1 = Oversampler2x::new();
        os1.prepare(fs as f32);
        let mut os2 = Oversampler2x::new();
        os2.prepare(fs as f32 * 2.0);
        alias_to_signal_db(m, n, fs, amp, move |x| {
            if factor == 4 {
                os1.tick_with(x, |a| os2.tick_with(a, |b| t.tick(b)))
            } else {
                os1.tick_with(x, |a| t.tick(a))
            }
        })
    };
    println!("\nWorst-case (hot+bright) alias-to-signal, dB (lower=better):");
    println!(
        "{:<10} {:>10} {:>10} {:>12} {:>12}",
        "tone/drv", "1st+2x", "2nd+2x", "1st+4xbiquad", "order gain / OS gain"
    );
    for (label, m) in [("~5k/20", 853usize), ("~7k/20", 1195), ("~9k/20", 1536)] {
        let base = asr(AdaaOrder::First, 2, m, 20.0);
        let ord2 = asr(AdaaOrder::Second, 2, m, 20.0);
        let os4 = asr(AdaaOrder::First, 4, m, 20.0);
        println!(
            "{:<10} {:>10.1} {:>10.1} {:>12.1}   2nd:{:+.1}  4x:{:+.1}",
            label,
            base,
            ord2,
            os4,
            base - ord2,
            base - os4
        );
    }
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn oversampler_net_verdict() {
    // Objective decision: does adding the 2x biquad oversampler to the
    // ADAA tube path help or hurt, aggregated over a dense freq×drive×
    // amp sweep? (Delta uses the SAME wanted-bin set for both configs,
    // so it's robust to any bin-coincidence in the absolute ASR.)
    // Run: cargo test -p stave-dsp oversampler_net_verdict -- --ignored --nocapture
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 8192usize;
    let run = |adaa: bool, os: bool, m: usize, amp: f32, drv: f32| -> f64 {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(drv));
        t.set_adaa(adaa);
        let mut osr = Oversampler2x::new();
        osr.prepare(fs as f32);
        alias_to_signal_db(m, n, fs, amp, move |x| {
            if os {
                osr.tick_with(x, |s| t.tick(s))
            } else {
                t.tick(x)
            }
        })
    };
    let mut deltas = vec![]; // (os2x - alone): positive ⇒ OS helps
    let (mut helps, mut hurts, mut worst_hurt, mut best_help) = (0, 0, 0.0f64, 0.0f64);
    let drvs = [6.0f32, 12.0, 18.0, 24.0];
    // per-drive accumulators: (sum delta, count, sum abs-alias-floor of best config)
    let mut per_drive: Vec<(f64, usize, f64)> = vec![(0.0, 0, 0.0); drvs.len()];
    for f0 in (2000..=11000).step_by(1000) {
        let m = ((f0 as f64) * n as f64 / fs).round() as usize;
        for &amp in &[0.25f32, 0.5] {
            for (di, &drv) in drvs.iter().enumerate() {
                let alone = run(true, false, m, amp, drv);
                let both = run(true, true, m, amp, drv);
                let d = alone - both; // both lower(better) ⇒ d>0 ⇒ OS helps
                deltas.push(d);
                if d.is_finite() {
                    per_drive[di].0 += d;
                    per_drive[di].1 += 1;
                    per_drive[di].2 += both.min(alone); // best achievable alias floor
                }
                if d > 0.5 {
                    helps += 1;
                    best_help = best_help.max(d);
                } else if d < -0.5 {
                    hurts += 1;
                    worst_hurt = worst_hurt.min(d);
                }
            }
        }
    }
    println!("\nPer-drive (mean OS benefit | mean best-achievable alias floor):");
    for (di, &drv) in drvs.iter().enumerate() {
        let (s, c, fl) = per_drive[di];
        println!(
            "  {:>2.0} dB drive: {:+5.1} dB OS benefit | floor {:6.1} dB",
            drv,
            s / c.max(1) as f64,
            fl / c.max(1) as f64
        );
    }
    let finite: Vec<f64> = deltas.iter().copied().filter(|d| d.is_finite()).collect();
    let mean = finite.iter().sum::<f64>() / finite.len().max(1) as f64;
    println!(
        "\nADAA+2x vs ADAA-alone over {} points ({} finite):",
        deltas.len(),
        finite.len()
    );
    println!("  mean OS benefit: {:+.2} dB (positive = OS cleaner)", mean);
    println!(
        "  OS helps (>0.5dB): {helps}   hurts (>0.5dB): {hurts}   tie: {}",
        deltas.len() - helps - hurts
    );
    println!(
        "  best help: {:+.1} dB   worst hurt: {:+.1} dB",
        best_help, worst_hurt
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn aliasing_verification_table() {
    // Full-spectrum alias-to-signal ratio for the four configurations,
    // swept over input frequency and drive. Run with:
    //   cargo test -p stave-dsp aliasing_verification_table -- --ignored --nocapture
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 8192usize; // bin spacing 5.859 Hz
    let amp = 0.5f32;
    // (label, fundamental bin) — chosen so no folded harmonic lands on
    // a harmonic bin (clean classification).
    let tones = [("~5 kHz", 853usize), ("~7 kHz", 1195), ("~9 kHz", 1536)];
    let drives = [12.0f32, 20.0];

    println!("\nAlias-to-signal ratio (dB, lower=cleaner) — amp {amp}");
    println!(
        "{:<8} {:>6} | {:>9} {:>9} {:>9} {:>9} | {:>10}",
        "tone", "drive", "raw", "2xOS", "ADAA", "ADAA+2x", "ADAA gain"
    );
    for (label, m) in tones {
        for &drv in &drives {
            let mk = |adaa: bool, os: bool| {
                let mut t = Triode::new();
                t.prepare(fs as f32);
                t.set_grid_drive(knob(drv));
                t.set_adaa(adaa);
                let mut osr = Oversampler2x::new();
                osr.prepare(fs as f32);
                alias_to_signal_db(m, n, fs, amp, move |x| {
                    if os {
                        osr.tick_with(x, |s| t.tick(s))
                    } else {
                        t.tick(x)
                    }
                })
            };
            let raw = mk(false, false);
            let os2 = mk(false, true);
            let adaa = mk(true, false);
            let both = mk(true, true);
            println!(
                "{:<8} {:>5.0}dB | {:>8.1} {:>9.1} {:>9.1} {:>9.1} | {:>+8.1} dB",
                label,
                drv,
                raw,
                os2,
                adaa,
                both,
                os2 - both
            );
        }
    }
}

#[test]
fn lut_matches_solver_and_antiderivatives_are_consistent() {
    // 1) The tabulated transfer must match the direct Newton solve.
    // 2) d/dx F1 ≈ f and d/dx F2 ≈ F1 (the tables really are successive
    //    antiderivatives), verified by central differences of the eval
    //    functions. 3) F1, F2 ≈ 0 at the operating point (input 0).
    let t = Triode::new();
    for &x in &[-1.5_f64, -0.7, -0.2, 0.0, 0.2, 0.7, 1.5] {
        let lut = t.f_eval(x);
        let direct = t.output_scale * t.core_transfer(t.vg_of(x));
        assert!(
            (lut - direct).abs() < 1e-3,
            "LUT vs solver at x={x}: {lut} vs {direct}"
        );
    }
    let h = 1e-3;
    for &x in &[-0.8_f64, -0.3, 0.1, 0.5, 1.0] {
        let df1 = (t.f1_eval(x + h) - t.f1_eval(x - h)) / (2.0 * h);
        assert!(
            (df1 - t.f_eval(x)).abs() < 5e-3,
            "dF1/dx != f at x={x}: {df1} vs {}",
            t.f_eval(x)
        );
        let df2 = (t.f2_eval(x + h) - t.f2_eval(x - h)) / (2.0 * h);
        assert!(
            (df2 - t.f1_eval(x)).abs() < 5e-3,
            "dF2/dx != F1 at x={x}: {df2} vs {}",
            t.f1_eval(x)
        );
    }
    assert!(
        t.f1_eval(0.0).abs() < 1e-6,
        "F1(0) should be ~0: {}",
        t.f1_eval(0.0)
    );
    assert!(
        t.f2_eval(0.0).abs() < 1e-6,
        "F2(0) should be ~0: {}",
        t.f2_eval(0.0)
    );
}

#[test]
fn adaa_matches_transfer_for_dc_and_slow_signals() {
    // For a DC or very slow input, ADAA must reproduce the static
    // transfer f(x) (no aliasing to suppress). Feed a constant and a
    // slow ramp and compare to direct evaluation.
    let mut t = Triode::new();
    t.set_grid_drive(knob(12.0));
    for &dc in &[-0.6_f64, -0.2, 0.3, 0.8] {
        // settle the ADAA state at this DC level
        for _ in 0..4 {
            let _ = t.tick(dc as f32);
        }
        let y = t.tick(dc as f32) as f64;
        let f = t.f_eval(dc);
        assert!((y - f).abs() < 1e-4, "ADAA DC mismatch at {dc}: {y} vs {f}");
    }
}

#[test]
fn adaa_ill_conditioned_branches_are_finite_and_correct() {
    // Exercise all three fallback branches: x0≈x2≈x1 (constant),
    // x0≈x2 but x1 differs (outer fallback), and generic tiny deltas.
    let mut t = Triode::new();
    t.set_grid_drive(knob(18.0));
    // Constant input → must equal f(const), no NaN.
    for _ in 0..8 {
        let _ = t.tick(0.4);
    }
    let y_const = t.tick(0.4);
    assert!(y_const.is_finite());
    assert!((y_const as f64 - t.f_eval(0.4)).abs() < 1e-4);
    // x0≈x2 with x1 different: feed A, B, A (period-2). All finite.
    let mut tr = Triode::new();
    tr.set_grid_drive(knob(18.0));
    let mut mx = 0.0f32;
    for i in 0..256 {
        let v = if i % 2 == 0 { 0.5 } else { -0.5 };
        let y = tr.tick(v);
        assert!(y.is_finite(), "non-finite in period-2 drive");
        mx = mx.max(y.abs());
    }
    assert!(mx < 8.0);
    // Sub-epsilon deltas around a point → ≈ f at that point, finite.
    let mut td = Triode::new();
    td.set_grid_drive(knob(6.0));
    for _ in 0..4 {
        let _ = td.tick(0.31);
    }
    let y = td.tick(0.31 + 1e-9);
    assert!(y.is_finite() && (y as f64 - td.f_eval(0.31)).abs() < 1e-3);
}

#[test]
fn adaa_reduces_aliasing() {
    // A 5 kHz tone driven hard: harmonics 5/10/15/20 kHz; alias products
    // land at non-harmonic bins (3 kHz is the 45 kHz image). ADAA must
    // hold that alias bin far below the aliased (ADAA-off) baseline.
    // Raw triode at base rate (no oversampler) isolates ADAA's effect.
    let fs = 48_000.0;
    let f0 = 5_000.0;
    let alias_bin = 3_000.0;
    let n = 16_384;

    let run = |adaa: bool| -> Vec<f32> {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(18.0));
        t.set_adaa(adaa);
        // warm-up
        for i in 0..n {
            let s = 0.5 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            let _ = t.tick(s);
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let s = 0.5 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            out[i] = t.tick(s);
        }
        out
    };
    let off = run(false);
    let on = run(true);
    let a_off = goertzel(&off, alias_bin, fs);
    let a_on = goertzel(&on, alias_bin, fs);
    println!(
        "alias@3k: ADAA-off {:.2e}  ADAA-on {:.2e}  ({:.1} dB lower)",
        a_off,
        a_on,
        20.0 * (a_off / a_on.max(1e-12)).log10()
    );
    assert!(
        a_on < a_off * 0.5,
        "ADAA should cut aliasing >=6 dB: off={a_off} on={a_on}"
    );
}

/// ADAA's in-band harmonics match the alias-free truth (the same
/// stage processed at 8× and measured there). The naive 48 kHz run
/// is no reference: folded images move its h4 by 2.4 dB.
#[test]
fn adaa_preserves_inband_harmonics() {
    let fs = 48_000.0;
    let f0 = 1_000.0;
    let n = 16_384;
    let run = |rate: f64, adaa: bool| -> Vec<f32> {
        let mut t = Triode::new();
        t.prepare(rate as f32);
        t.set_grid_drive(knob(12.0));
        t.set_adaa(adaa);
        let frames = (n as f64 * rate / fs) as usize;
        let sine =
            |i: usize| 0.4 * (2.0 * core::f64::consts::PI * f0 * i as f64 / rate).sin() as f32;
        for i in 0..frames {
            let _ = t.tick(sine(i));
        }
        (0..frames).map(|i| t.tick(sine(i))).collect()
    };
    let truth = run(fs * 8.0, false);
    let on = run(fs, true);
    for k in 2..=5 {
        let f = k as f64 * f0;
        let r_truth = goertzel(&truth, f, fs * 8.0) / goertzel(&truth, f0, fs * 8.0);
        let r_on = goertzel(&on, f, fs) / goertzel(&on, f0, fs);
        let ddb = 20.0 * (r_on / r_truth).log10();
        assert!(
            ddb.abs() < 1.0,
            "ADAA moved h{k} {ddb:+.2} dB from the truth"
        );
    }
}

// ===== Adversarial: solver convergence + cathode aliasing =====

#[test]
fn load_line_converges_over_full_build_range_all_tubes() {
    // The LUT is built across the whole grid-voltage range, seeded from
    // a fixed point with only 8 Newton iterations. Verify the load line
    // is actually solved (residual ≈ 0) everywhere it's sampled — for
    // every tube — or the tables are wrong.
    for &tt in &[
        Koren::T12AX7,
        Koren::T12AT7,
        Koren::T12AU7,
        Koren::T6DJ8,
        Koren::T6SN7,
    ] {
        let t = Triode::with_tube(tt);
        let n = 400;
        let mut worst = 0.0f64;
        let mut worst_vg = 0.0;
        for i in 0..=n {
            let vg_raw = VG_MIN + (VG_MAX - VG_MIN) * i as f64 / n as f64;
            let vgk = grid_volts_at(&t.p, vg_raw); // same path as core_transfer
            let vpk = t.solve_plate(vgk, t.vpk0); // same seed as build_luts
            let ip = t.plate_current_at(vgk, vpk);
            let resid = ((t.p.supply_v - vpk) / t.p.r_plate - ip).abs();
            if resid > worst {
                worst = resid;
                worst_vg = vgk;
            }
        }
        println!(
            "{}: worst load-line residual {:.2e} A @ Vgk {:.2}",
            tt.name, worst, worst_vg
        );
        assert!(
            worst < 1e-6,
            "{}: solver not converged ({worst:.2e} A @ {worst_vg})",
            tt.name
        );
    }
}

#[test]
fn cathode_bias_does_not_add_aliasing() {
    // The cathode bias is a slow operating-point modulation, so it must
    // not add aliasing BEYOND the base triode at any τ. (The base
    // triode's ADAA-only near-Nyquist residual on a 4 kHz tone is the
    // oversampler's job, tested separately — here we isolate the
    // cathode's contribution as the on-vs-off delta.)
    let fs = 48_000.0;
    let m = 683; // ~4 kHz, harmonics fold near Nyquist (worst case)
    let base = {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(14.0));
        noise_floor_db(m, 8192, 0.5, move |x| t.tick(x))
    };
    for &tau in &[0.05_f32, 1.0, 5.0, 30.0] {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(14.0));
        self_biased(&mut t, f64::from(tau));
        let on = noise_floor_db(m, 8192, 0.5, move |x| t.tick(x));
        println!(
            "4 kHz floor — base {base:.1} dB, cathode τ={tau:>4}ms {on:.1} dB (Δ {:+.1})",
            on - base
        );
        assert!(
            on < base + 2.0,
            "cathode τ={tau} added aliasing: base {base}, on {on}"
        );
    }
}

#[test]
fn full_path_high_freq_is_clean_with_cathode() {
    // The real signal path = triode (cathode on) INSIDE the 2× FIR
    // half-band oversampler. Confirm that at a near-Nyquist
    // fundamental the full path is clean — i.e. the oversampler
    // removes the base ADAA's residual and the cathode doesn't
    // reintroduce it. The triode is prepared at 2× — the rate it
    // ticks inside the oversampler (rate contract).
    use crate::kernel::resample::Oversampler2xFir;
    let fs = 48_000.0;
    let m = 683;
    let mut t = Triode::new();
    t.prepare(fs as f32 * 2.0);
    t.set_grid_drive(knob(14.0));
    self_biased(&mut t, 30.0);
    let mut os = Oversampler2xFir::new();
    os.prepare(fs as f32);
    // Audible-band (≤20 kHz) classification: the half-band's
    // transition shoulder folds only onto 20 kHz–Nyquist (inaudible
    // by design); what's left below 20 kHz is the ADAA machinery's own
    // inner-rate residual — measured −49.9 dB here, identical between
    // FIR and IIR decimators (the decimator can't touch it; more
    // oversampling can — a 4× saturation stack does).
    let max_bin = (20_000.0 * 8192.0 / fs) as usize;
    let floor = noise_floor_db_upto(m, 8192, 0.5, max_bin, move |x| {
        os.tick_with(x, |s| t.tick(s))
    });
    println!("4 kHz full-path audible-band floor (cathode on + FIR 2×): {floor:.1} dB");
    assert!(
        floor < -45.0,
        "full path aliases audibly at 4 kHz: {floor} dB"
    );
}

#[test]
fn companding_reduces_tube_hf_thd() {
    // The "smooth highs" mechanism applied to the TUBE: de-emphasise
    // highs into the triode, re-emphasise after. It reduces HF harmonic
    // generation (a high tone distorts less) — but only modestly,
    // because the tube clips hard (cutoff/conduction) and reducing the
    // input level doesn't stop the clip the way it does for a smooth
    // polynomial. This documents the HONEST benefit.
    use crate::primitives::filters::biquad::Biquad;
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 16_384usize;
    let f0 = 5_000.0;
    let thd = |hf: f32| -> f64 {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(8.0));
        let mut os = Oversampler2x::new();
        os.prepare(fs as f32);
        let mut pre = Biquad::new(FilterSpec::HighShelf {
            frequency: Hz::new(2_500.0),
            gain: Db::new(-hf),
        });
        pre.prepare(fs as f32);
        let mut post = Biquad::new(FilterSpec::HighShelf {
            frequency: Hz::new(2_500.0),
            gain: Db::new(hf),
        });
        post.prepare(fs as f32);
        let mut step = |x: f32| post.tick(os.tick_with(pre.tick(x), |s| t.tick(s)));
        let sig = |i: usize| 0.4 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        for i in 0..n {
            let _ = step(sig(i));
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = step(sig(i));
        }
        let h1 = goertzel(&out, f0, fs).max(1e-9);
        (2..=4)
            .map(|k| goertzel(&out, k as f64 * f0, fs))
            .map(|h| h * h)
            .sum::<f64>()
            .sqrt()
            / h1
    };
    let off = thd(0.0);
    let on = thd(8.0);
    println!(
        "tube 5 kHz THD: companding off {:.1}% | on {:.1}% ({:+.1} dB)",
        off * 100.0,
        on * 100.0,
        20.0 * (on / off.max(1e-9)).log10()
    );
    // Honest, modest: a few dB of HF THD reduction (the tube still
    // clips). Velvet's Chebyshev is the path to *dramatic* smoothing.
    assert!(
        on < off * 0.85,
        "companding should reduce tube HF THD somewhat (off {off}, on {on})"
    );
}

// ===== Generic tube types =====

#[test]
fn tube_types_are_sane_and_gain_orders_by_mu() {
    let fs = 48_000.0;
    let types = [
        Koren::T12AX7,
        Koren::T12AT7,
        Koren::T6DJ8,
        Koren::T12AU7,
        Koren::T6SN7,
    ];
    let mut gains = vec![];
    for &t in &types {
        let mut tube = Triode::with_tube(t);
        tube.prepare(fs as f32);
        let vpk0 = tube.operating_point();
        // Operating point must sit between the rails.
        assert!(vpk0 > 20.0 && vpk0 < 295.0, "{}: Vpk0 off: {vpk0}", t.name);
        // Datasheet-ish: plate current at the operating point is mA-range.
        let ip = (300.0 - vpk0) / 100e3;
        assert!(
            ip > 0.05e-3 && ip < 30e-3,
            "{}: Ip off: {} mA",
            t.name,
            ip * 1e3
        );
        // Small-signal gain |dVpk/dVgk| at the tube's bias.
        let bias = TriodeParams::for_tube(t).grid_bias;
        let dv = 1e-3;
        let g = ((tube.solve_plate(bias - dv, vpk0) - tube.solve_plate(bias + dv, vpk0))
            / (2.0 * dv))
            .abs();
        // Stable, finite, bounded when driven.
        tube.set_grid_drive(knob(10.0));
        let mut mx = 0.0f32;
        for i in 0..4096 {
            let s = 0.5 * (2.0 * core::f64::consts::PI * 1000.0 * i as f64 / fs).sin() as f32;
            let y = tube.tick(s);
            assert!(y.is_finite());
            mx = mx.max(y.abs());
        }
        assert!(mx < 8.0, "{}: runaway {mx}", t.name);
        println!(
            "{}: Vpk0 {:.0} V, Ip {:.2} mA, gain {:.0}",
            t.name,
            vpk0,
            ip * 1e3,
            g
        );
        gains.push((t, g));
    }
    // Gain must order by mu: 12AX7(100) > 12AT7(60) > 6DJ8(33) > 12AU7(21) ≳ 6SN7(20).
    for w in gains.windows(2) {
        assert!(
            w[0].1 >= w[1].1 - 2.0,
            "gain order wrong: {} ({:.0}) should be ≥ {} ({:.0})",
            w[0].0.name,
            w[0].1,
            w[1].0.name,
            w[1].1
        );
    }
    assert!(
        gains[0].1 > gains[3].1 * 1.5,
        "12AX7 gain should clearly exceed 12AU7"
    );
}

#[test]
fn set_tube_is_a_noop_when_unchanged() {
    // Selecting the same type repeatedly must not rebuild/alter state.
    let mut t = Triode::with_tube(Koren::T12AT7);
    t.prepare(48_000.0);
    let vpk0 = t.operating_point();
    for _ in 0..10 {
        t.set_tube(Koren::T12AT7);
    }
    assert_eq!(t.operating_point(), vpk0);
    assert_eq!(t.tube(), Koren::T12AT7);
}

// ===== Cathode self-bias (stateful tube dynamics) =====

fn run_sine(t: &mut Triode, f0: f64, fs: f64, amp: f32, samples: usize) {
    for i in 0..samples {
        let s = amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        let _ = t.tick(s);
    }
}

#[test]
fn cathode_off_is_static() {
    // depth 0 ⇒ no bias drift, no compression (pure static saturator).
    let fs = 48_000.0;
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_grid_drive(knob(10.0));
    run_sine(&mut t, 200.0, fs, 0.6, fs as usize / 2);
    assert!(
        t.gain_reduction().get().abs() < 0.1,
        "static stage drifted: {}",
        t.gain_reduction().get()
    );
    assert!(t.bias_shift().abs() < 1e-6);
}

/// Cathode self-bias compresses with program level once the grid
/// conducts: grid current joins the cathode current and drives the
/// stage colder (measured −0.03 dB at 0.05 in, −0.69 dB at 0.8 in,
/// 14 dB drive). Below conduction a 12AX7 at −1.5 V barely moves.
#[test]
fn cathode_bias_compresses_with_level() {
    let fs = 48_000.0;
    let gr_after = |amp: f32| -> f32 {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(14.0));
        self_biased(&mut t, 30.0);
        run_sine(&mut t, 200.0, fs, amp, fs as usize);
        t.gain_reduction().get()
    };
    let (quiet, loud) = (gr_after(0.05), gr_after(0.8));
    assert!(loud < quiet - 0.3, "quiet {quiet}, loud {loud}");
}

/// The cathode moves at the RC's pace and returns in silence: the
/// drift reaches 63 % of its settled size later for a larger bypass
/// cap, and vanishes once the signal stops (driven into grid
/// conduction, where the drift is clear: +0.33 V).
#[test]
fn cathode_responds_and_recovers_over_time() {
    let fs = 48_000.0;
    let drift = |rc_ms: f64| -> Vec<f64> {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(14.0));
        self_biased(&mut t, rc_ms);
        let loud = (0..fs as usize).map(|i| {
            let s = 0.8 * (2.0 * core::f64::consts::PI * 200.0 * i as f64 / fs).sin() as f32;
            let _ = t.tick(s);
            t.cathode_shift()
        });
        let mut out: Vec<f64> = loud.collect();
        out.extend((0..fs as usize).map(|_| {
            let _ = t.tick(0.0);
            t.cathode_shift()
        }));
        out
    };
    let rise_ms = |d: &[f64]| {
        let settled = d[fs as usize - 1].abs();
        let at = d.iter().position(|&v| v.abs() >= 0.63 * settled).unwrap();
        at as f64 / fs * 1000.0
    };
    let (fast, slow) = (drift(15.0), drift(120.0));
    let settled = fast[fs as usize - 1].abs();
    assert!(settled > 0.1, "no drift under program: {settled} V");
    assert!(
        rise_ms(&slow) > 3.0 * rise_ms(&fast),
        "{} vs {}",
        rise_ms(&fast),
        rise_ms(&slow)
    );
    let rested = fast.last().unwrap().abs();
    assert!(rested < 0.01 * settled, "did not recover: {rested}");
}

#[test]
fn cathode_stable_at_extreme() {
    let fs = 48_000.0;
    let f0 = 300.0;
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_grid_drive(knob(18.0));
    self_biased(&mut t, 20.0);
    let mut mx = 0.0f32;
    for i in 0..(fs as usize) {
        let s = 0.9 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        let y = t.tick(s);
        assert!(y.is_finite(), "non-finite with extreme cathode bias");
        mx = mx.max(y.abs());
    }
    assert!(mx < 8.0, "runaway with extreme cathode bias: {mx}");
}

#[test]
fn cathode_does_not_add_aliasing() {
    // The bias drift is slow, so it must not raise the noise floor.
    let fs = 48_000.0;
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_grid_drive(knob(12.0));
    self_biased(&mut t, 30.0);
    let floor = noise_floor_db(38, 8192, 0.5, move |x| t.tick(x));
    println!("cathode-on noise floor: {floor:.1} dB");
    assert!(floor < -50.0, "cathode bias added audible noise: {floor}");
}

#[test]
fn koren_matches_12ax7_datasheet() {
    // A 12AX7 at Vpk≈250 V, Vgk=−2 V draws ~1 mA; near cutoff
    // (Vgk≈−4 V) it's essentially off. Check the Koren law directly.
    let t = Triode::new();
    let ip_2v = t.plate_current_at(-2.0, 250.0);
    println!("Ip(Vgk=-2, Vpk=250) = {:.3} mA", ip_2v * 1e3);
    assert!(
        ip_2v > 0.4e-3 && ip_2v < 1.6e-3,
        "datasheet current off: {ip_2v}"
    );
    let ip_cut = t.plate_current_at(-4.5, 250.0);
    assert!(
        ip_cut < ip_2v * 0.1,
        "should be near cutoff at -4.5 V: {ip_cut}"
    );
    // Current must rise with grid voltage and with plate voltage.
    assert!(t.plate_current_at(-1.0, 250.0) > ip_2v);
    assert!(t.plate_current_at(-2.0, 300.0) > ip_2v);
}

#[test]
fn operating_point_is_realistic() {
    let t = Triode::new();
    let vpk0 = t.operating_point();
    println!("operating point Vpk0 = {:.1} V", vpk0);
    // Classic 12AX7 stage (300 V / 100 k / −1.5 V) sits ~180–230 V.
    assert!(vpk0 > 150.0 && vpk0 < 250.0, "operating point off: {vpk0}");
}

#[test]
fn load_line_residual_converges() {
    // After solving, the load-line equation must actually hold.
    let t = Triode::new();
    for &vgk in &[-3.0, -2.0, -1.5, -1.0, -0.5, 0.0] {
        let vpk = t.solve_plate(vgk, t.operating_point());
        let ip = t.plate_current_at(vgk, vpk);
        let load_i = (t.p.supply_v - vpk) / t.p.r_plate;
        let resid = (load_i - ip).abs();
        println!(
            "Vgk {:+.1} -> Vpk {:7.2}  residual {:.2e} A",
            vgk, vpk, resid
        );
        assert!(
            resid < 1e-6,
            "load line unsolved at Vgk={vgk}: resid={resid}"
        );
    }
}

#[test]
fn small_signal_gain_near_mu_rl_over_rl_plus_rp() {
    // Textbook: A = μ·RL/(RL+rp). 12AX7 rp≈62 kΩ ⇒ A≈61 with RL=100k.
    // We measure |dVpk/dVgk| at the operating point.
    let t = Triode::new();
    let v0 = t.operating_point();
    let dv = 1e-3;
    let a = ((t.solve_plate(-1.5 - dv, v0) - t.solve_plate(-1.5 + dv, v0)) / (2.0 * dv)).abs();
    println!("small-signal gain |dVpk/dVgk| = {:.1}", a);
    assert!(a > 40.0 && a < 80.0, "gain implausible for 12AX7: {a}");
}

/// THE POLICY INVARIANT — the reported gain IS the measured gain, under
/// both policies. Calibrated: unity at any drive (the historical
/// contract). Physical: gain FOLLOWS drive like a pot into a fixed
/// stage. Consumers compute makeup from the report — this invariant is
/// what makes fitted makeup ladders illegal.
#[test]
fn the_report_is_the_truth() {
    let fs = 48_000.0f64;
    let f0 = 1_000.0;
    let n = 16_384;
    let measure = |policy: LevelPolicy, drive: f32| -> (f64, f32) {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_level_policy(policy);
        t.set_grid_drive(knob(drive));
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            // −60 dB: far below any knee, pure small-signal.
            let s = 0.001 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            out[i] = t.tick(s);
        }
        let g = goertzel(&out[n / 2..], f0, fs) / 0.001;
        (20.0 * g.max(1e-12).log10(), t.small_signal_gain().get())
    };
    for &drive in &[0.0f32, 8.0, 16.0] {
        let (meas, rep) = measure(LevelPolicy::Calibrated, drive);
        assert!(
            meas.abs() < 1.5 && f64::from(rep).abs() < 0.5,
            "calibrated must stay unity: drive {drive} measured {meas:.2} reported {rep:.2}"
        );
        let (meas_p, rep_p) = measure(
            LevelPolicy::Physical {
                reference_drive: knob(0.0),
            },
            drive,
        );
        assert!(
            (meas_p - f64::from(rep_p)).abs() < 1.5,
            "physical report lies: drive {drive} measured {meas_p:.2} reported {rep_p:.2}"
        );
    }
    // And the physical gain actually climbs with drive.
    let (g0, _) = measure(
        LevelPolicy::Physical {
            reference_drive: knob(0.0),
        },
        0.0,
    );
    let (g16, _) = measure(
        LevelPolicy::Physical {
            reference_drive: knob(0.0),
        },
        16.0,
    );
    assert!(
        g16 - g0 > 20.0,
        "physical drive bought only {:.1} dB of gain",
        g16 - g0
    );
}

/// THE CEILING STAYS PUT — under Physical the knee's OUTPUT level
/// is drive-invariant (the real-amp shape); under Calibrated it
/// falls with drive (the effect-box shape). Both are contracts.
#[test]
fn the_ceiling_obeys_the_policy() {
    let fs = 48_000.0f64;
    let f0 = 1_000.0;
    let n = 16_384;
    let ceiling = |policy: LevelPolicy, drive: f32| -> f64 {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_level_policy(policy);
        t.set_grid_drive(knob(drive));
        let mut pk = 0.0f64;
        for i in 0..n {
            // 0 dBFS — far past any knee at any drive.
            let s = (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
            let y = t.tick(s);
            if i > n / 2 {
                pk = pk.max(f64::from(y.abs()));
            }
        }
        20.0 * pk.max(1e-12).log10()
    };
    let phys = ceiling(
        LevelPolicy::Physical {
            reference_drive: knob(0.0),
        },
        16.0,
    ) - ceiling(
        LevelPolicy::Physical {
            reference_drive: knob(0.0),
        },
        4.0,
    );
    let cal = ceiling(LevelPolicy::Calibrated, 16.0) - ceiling(LevelPolicy::Calibrated, 4.0);
    assert!(
        phys.abs() <= 2.0,
        "physical ceiling moved {phys:.1} dB across drive"
    );
    assert!(
        cal < -6.0,
        "calibrated ceiling should fall with drive (moved {cal:.1} dB)"
    );
    // And the REPORTED ceiling is the measured one (Physical).
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_level_policy(LevelPolicy::Physical {
        reference_drive: knob(0.0),
    });
    t.set_grid_drive(knob(16.0));
    let reported = f64::from(t.ceiling().get());
    let measured = ceiling(
        LevelPolicy::Physical {
            reference_drive: knob(0.0),
        },
        16.0,
    );
    assert!(
        (measured - reported).abs() < 3.0,
        "ceiling report lies: reported {reported:.1}, measured {measured:.1}"
    );
}

/// IDEMPOTENT SETTERS — re-binding unchanged values must not
/// perturb a settled operating point.
#[test]
fn rebind_is_a_no_op() {
    let fs = 48_000.0f64;
    let f0 = 220.0;
    let n = 8_192;
    let run = |rebind: bool| -> Vec<f32> {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(knob(12.0));
        self_biased(&mut t, 40.0);
        (0..n)
            .map(|i| {
                if rebind && i == n / 2 {
                    t.set_grid_drive(knob(12.0)); // same value, mid-stream
                }
                let s = 0.3 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
                t.tick(s)
            })
            .collect()
    };
    let a = run(false);
    let b = run(true);
    assert!(
        a.iter().zip(&b).all(|(x, y)| x == y),
        "re-binding an unchanged drive perturbed the stream"
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn drive_sweep_explore() {
    // THD vs knob position at a typical band level — verifies the taper
    // ramps saturation in early instead of bunching it at the top.
    // Run with `--ignored --nocapture`.
    let fs = 48_000.0;
    let f0 = 1_000.0;
    let n = 16_384;
    for &amp in &[0.3_f32, 0.1] {
        println!("--- amp {amp} (THD% by knob position) ---");
        let mut drive = 0.0f32;
        while drive <= 20.0 {
            let mut t = Triode::new();
            t.prepare(fs as f32);
            t.set_grid_drive(knob(drive));
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                let s = amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
                out[i] = t.tick(s);
            }
            let h1 = goertzel(&out, f0, fs);
            let thd: f64 = (2..=8)
                .map(|k| goertzel(&out, k as f64 * f0, fs))
                .map(|h| h * h)
                .sum::<f64>()
                .sqrt()
                / h1.max(1e-9);
            println!(
                "  knob {:>3.0}% | THD {:5.1}%",
                drive / 20.0 * 100.0,
                thd * 100.0
            );
            drive += 2.0;
        }
    }
}

#[test]
fn stable_and_tube_voiced() {
    let fs = 48_000.0_f64;
    let f0 = 1_000.0_f64;
    let n = 16_384usize;
    // Test at both a hot tone (0.5) and a realistic quiet band level
    // (0.1) — the latter is what a crossover band actually sees.
    for &amp in &[0.5_f32, 0.1] {
        println!("--- input amp {amp} ---");
        for &drive in &[0.0_f32, 6.0, 12.0, 18.0, 24.0] {
            let mut t = Triode::new();
            t.prepare(fs as f32);
            t.set_grid_drive(knob(drive));
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                let s = amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
                out[i] = t.tick(s);
            }
            assert!(
                out.iter().all(|x| x.is_finite()),
                "non-finite at {drive} dB"
            );
            let omax = out.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
            assert!(omax < 8.0, "runaway {omax} at {drive} dB");
            let h1 = goertzel(&out, f0, fs);
            let h2 = goertzel(&out, 2.0 * f0, fs);
            let h3 = goertzel(&out, 3.0 * f0, fs);
            let db = |a: f64| 20.0 * (a / h1).max(1e-9).log10();
            println!(
                "drive {:>4.0} dB | out_rms {:.3} | h2 {:+.1} dB  h3 {:+.1} dB",
                drive,
                rms(&out),
                db(h2),
                db(h3)
            );
        }
    }
    // A quiet band at high drive must still produce audible harmonics.
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_grid_drive(knob(20.0));
    let mut q = vec![0.0f32; n];
    for i in 0..n {
        let s = 0.1 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        q[i] = t.tick(s);
    }
    let qh1 = goertzel(&q, f0, fs);
    let qh2 = goertzel(&q, 2.0 * f0, fs);
    assert!(
        qh2 / qh1 > 0.03,
        "quiet band at high drive should clearly saturate, h2/h1={}",
        qh2 / qh1
    );
    // Warm-zone voicing: while the grid stays between cutoff and
    // conduction (here −2.4…−0.6 V around the −1.5 V bias) only the
    // plate law's curvature bends the wave, so the stage must be
    // 2nd-harmonic dominant (no tanh rails). Once both ends clip
    // (grid conduction on top, cutoff below) a real tube correctly
    // shifts toward odd/square — a different regime.
    let mut t = Triode::new();
    t.prepare(fs as f32);
    t.set_grid_drive(knob(8.0));
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let s = 0.08 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        out[i] = t.tick(s);
    }
    let h1 = goertzel(&out, f0, fs);
    let h2 = goertzel(&out, 2.0 * f0, fs);
    let h3 = goertzel(&out, 3.0 * f0, fs);
    assert!(
        h2 > h3,
        "warm-zone tube should be 2nd-harmonic dominant: h2={h2} h3={h3}"
    );
    assert!(h2 / h1 > 0.01, "expected audible 2nd harmonic: {}", h2 / h1);
}

/// The Hermite table is the solved stage: values to the table's O(h⁴)
/// between nodes, the tabulated slope the equilibrium's own (against a
/// central difference of the solve), the cathode current table the
/// solved plate-plus-grid current — and under `Bias::Cathode` the
/// circuit with the stage's Rk rests where the stage does: the plate's
/// load line from B+ to the plate (to ground) carries the tube's current
/// at its plate-to-cathode volts, and Rk carries it at the design bias.
#[test]
fn the_table_is_the_solved_stage() {
    for bias in [Bias::Fixed, Bias::Cathode { bypass_farads: 22e-6 }] {
        let mut t = Triode::new();
        t.set_bias(bias);
        t.prepare(48_000.0);
        let mut worst = (0.0f64, 0.0f64, 0.0f64);
        let mut at = 0.0;
        for i in 0..2000 {
            let vg = -6.0 + 7.0 * (f64::from(i) + 0.37) / 2000.0;
            let solved = t.solved(vg);
            let (f, slope) = t.curve.f_and_slope_at(vg);
            let h = 1e-5;
            let numeric = (t.core_transfer(vg + h) - t.core_transfer(vg - h)) / (2.0 * h);
            if (f - solved.swing.0).abs() > worst.0 {
                (worst.0, at) = ((f - solved.swing.0).abs(), vg);
            }
            worst.1 = worst.1.max((slope - numeric).abs() / numeric.abs().max(1.0));
            if let Bias::Cathode { .. } = bias {
                worst.2 = worst.2.max((t.cathode_amps_at(vg) - solved.cathode_amps.0).abs());
            }
        }
        println!("{bias:?}: table vs solve {:.2e} V (at {at:.3} V), slope {:.2e} rel, cathode current {:.2e} A", worst.0, worst.1, worst.2);
        assert!(worst.0 < 2e-5, "{bias:?}: table vs solve {:.2e} V", worst.0);
        assert!(worst.1 < 1e-4, "{bias:?}: slope {:.2e}", worst.1);
        assert!(worst.2 < 2e-10, "{bias:?}: cathode current {:.2e} A", worst.2);
    }
    let mut t = Triode::new();
    t.set_bias(Bias::Cathode { bypass_farads: 22e-6 });
    t.prepare(48_000.0);
    let (vk, plate) = (-t.p.grid_bias, t.operating_point());
    let vgk = grid_volts_at(&t.p, t.p.grid_bias);
    let ip = t.plate_current_at(vgk, plate - vk);
    let ig = GRID.grid_current(vgk).0;
    assert!(((t.p.supply_v - plate) / t.p.r_plate - ip).abs() < 1e-12, "the load line at rest");
    assert!((t.cathode_ohms() * (ip + ig) - vk).abs() < 1e-9, "the cathode at rest");
}
