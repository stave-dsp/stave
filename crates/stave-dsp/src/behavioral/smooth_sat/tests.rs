use super::*;

fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
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
    (re * re + im * im).sqrt() / (buf.len() as f64 * 0.5)
}

fn render<F: FnMut(f32) -> f32>(f0: f64, fs: f64, amp: f32, n: usize, mut f: F) -> Vec<f32> {
    for i in 0..n {
        let _ = f(amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32);
    }
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        out[i] = f(amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32);
    }
    out
}

#[test]
fn oversampled_alias_floor_high_input() {
    // The real harshness test: a high tone (7 kHz) whose harmonics fold.
    // Measure the non-harmonic (alias) floor as the plugin runs it —
    // inside the oversampler — at 2× vs 4×, default-ish settings.
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 1195usize; // ~7.00 kHz, exact bin
    let f0 = m as f64 * fs / n as f64;
    let bin_hz = |b: usize| b as f64 * fs / n as f64;

    let floor = |factor: u32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.6);
        s.set_smoothness(0.7);
        s.set_hf_smoothing(4.0);
        let mut os = Oversampler2x::new();
        os.prepare(fs as f32);
        let mut os2 = Oversampler2x::new();
        os2.prepare(fs as f32 * 2.0);
        // Correct topology: de-emphasis (base) → OS(memoryless shape) →
        // re-emphasis + DC (base).
        let samp = |x: f32, s: &mut SmoothSat, os: &mut Oversampler2x, os2: &mut Oversampler2x| {
            let pre = s.process_pre(x);
            let shaped = if factor == 4 {
                os.process_one(pre, |a| os2.process_one(a, |b| s.shape(b)))
            } else {
                os.process_one(pre, |a| s.shape(a))
            };
            s.process_post(shaped)
        };
        for i in 0..n {
            let _ = samp(
                0.7 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32,
                &mut s,
                &mut os,
                &mut os2,
            );
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = samp(
                0.7 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32,
                &mut s,
                &mut os,
                &mut os2,
            );
        }
        // Hann-windowed, measured between harmonics (no leakage).
        let _ = (m, bin_hz);
        let win: Vec<f32> = out
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let w = 0.5
                    - 0.5
                        * (2.0 * core::f64::consts::PI * i as f64 / (out.len() as f64 - 1.0)).cos();
                x * w as f32
            })
            .collect();
        let h1 = goertzel(&win, f0, fs).max(1e-12);
        let mut noise = 0.0f64;
        // midpoints between in-band harmonics of 7 kHz (3.5, 10.5, 17.5)
        // — NOT 21 kHz, which is the 3rd harmonic.
        for &fmid in &[3_500.0, 10_500.0, 17_500.0] {
            noise += goertzel(&win, fmid, fs).powi(2);
        }
        10.0 * (noise.sqrt() / h1).log10()
    };
    let two_x = floor(2);
    let four_x = floor(4);
    println!("SmoothSat 7 kHz alias floor (windowed): 2× {two_x:.1} dB | 4× {four_x:.1} dB");
    assert!(four_x < -65.0, "4× aliases at 7 kHz: {four_x}");
}

#[test]
fn hf_grain_stays_below_the_floor() {
    // Locate the "grainy HF on top": sweep drive / hf-smoothing /
    // smoothness through the full 4× chain at a 3.5 kHz tone and read
    // the non-harmonic (grain/alias) floor. Print-only.
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 597usize; // ~3.499 kHz exact bin
    let f0 = m as f64 * fs / n as f64;
    let bin_hz = |b: usize| b as f64 * fs / n as f64;
    let floor = |drive: f32, hf: f32, smooth: f32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(drive);
        s.set_hf_smoothing(hf);
        s.set_smoothness(smooth);
        let mut os = Oversampler2x::new();
        os.prepare(fs as f32);
        let mut os2 = Oversampler2x::new();
        os2.prepare(fs as f32 * 2.0);
        let step = |x: f32, s: &mut SmoothSat, os: &mut Oversampler2x, os2: &mut Oversampler2x| {
            let pre = s.process_pre(x);
            let sh = os.process_one(pre, |a| os2.process_one(a, |b| s.shape(b)));
            s.process_post(sh)
        };
        for i in 0..n {
            let _ = step(
                0.7 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32,
                &mut s,
                &mut os,
                &mut os2,
            );
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = step(
                0.7 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32,
                &mut s,
                &mut os,
                &mut os2,
            );
        }
        // Hann-window before measuring so the strong harmonics don't
        // leak into the grain bins; measure HALFWAY BETWEEN harmonics
        // (max distance from any harmonic ⇒ pure broadband grain).
        let win: Vec<f32> = out
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let w = 0.5
                    - 0.5
                        * (2.0 * core::f64::consts::PI * i as f64 / (out.len() as f64 - 1.0)).cos();
                x * w as f32
            })
            .collect();
        let h1 = goertzel(&win, f0, fs).max(1e-12);
        let mut noise = 0.0f64;
        // midpoints between harmonics k and k+1 (in Hz)
        for k in 1..=6 {
            let fmid = (k as f64 + 0.5) * f0;
            noise += goertzel(&win, fmid, fs).powi(2);
        }
        let _ = bin_hz;
        10.0 * (noise.sqrt() / h1).log10()
    };
    let mut worst = -300.0f64;
    for &drive in &[0.7f32, 1.5, 3.0] {
        for &hf in &[0.0f32, 8.0] {
            for &sm in &[0.3f32, 0.7, 0.95] {
                let g = floor(drive, hf, sm);
                println!("drive {drive:>3} hf {hf:>3} smooth {sm:>4}: grain floor {g:.1} dB");
                worst = worst.max(g);
            }
        }
    }
    // Regression guard: the soft-clip knee used to spike grain to
    // ~−58 dB at high drive; the low-curvature clip keeps it inaudible
    // (broadband HF grain between harmonics) across the whole matrix.
    assert!(worst < -68.0, "HF grain regressed: worst {worst:.1} dB");
}

#[test]
fn per_block_param_updates_dont_zipper() {
    // The plugin re-applies every setter each block. Re-applying the
    // SAME value must not disturb filter state — resetting the
    // companding shelves every buffer was a per-block discontinuity =
    // zipper on the top end. Render a steady tone while re-applying the
    // params every 256 samples and confirm no broadband grain appears.
    use crate::primitives::resample::Oversampler2x;
    let fs = 48_000.0;
    let n = 16_384usize;
    let m = 597usize;
    let f0 = m as f64 * fs / n as f64;
    let block = 256usize;
    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    let mut os = Oversampler2x::new();
    os.prepare(fs as f32);
    let mut os2 = Oversampler2x::new();
    os2.prepare(fs as f32 * 2.0);
    let apply = |s: &mut SmoothSat| {
        s.set_drive(1.6);
        s.set_even(0.15);
        s.set_odd(0.10);
        s.set_smoothness(0.7);
        s.set_hf_smoothing(6.0);
    };
    let step = |i: usize, s: &mut SmoothSat, os: &mut Oversampler2x, os2: &mut Oversampler2x| {
        if i % block == 0 {
            apply(s); // re-apply constant params every block, as the host does
        }
        let x = 0.7 * (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
        let pre = s.process_pre(x);
        let sh = os.process_one(pre, |a| os2.process_one(a, |b| s.shape(b)));
        s.process_post(sh)
    };
    for i in 0..n {
        let _ = step(i, &mut s, &mut os, &mut os2);
    }
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        out[i] = step(n + i, &mut s, &mut os, &mut os2);
    }
    let win: Vec<f32> = out
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let w = 0.5
                - 0.5 * (2.0 * core::f64::consts::PI * i as f64 / (out.len() as f64 - 1.0)).cos();
            x * w as f32
        })
        .collect();
    let h1 = goertzel(&win, f0, fs).max(1e-12);
    let mut noise = 0.0f64;
    for k in 1..=6 {
        noise += goertzel(&win, (k as f64 + 0.5) * f0, fs).powi(2);
    }
    let grain = 10.0 * (noise.sqrt() / h1).log10();
    println!("per-block-update grain: {grain:.1} dB");
    assert!(
        grain < -68.0,
        "per-block param updates zipper: {grain:.1} dB"
    );
}

#[test]
fn pushed_high_harmonics_decay() {
    // When pushed hard, the high-order harmonic tail must DECAY, not sit
    // on a fat shelf (that shelf was the audible "fizz" — caused by the
    // old hard flat-topping clip turning the signal near-square). The
    // smooth tanh bound keeps the tail rolling off. Measure h6..h10 at
    // drive 3 (smoothness at default) — all must be well below the
    // fundamental and below the low-order harmonics.
    let fs = 48_000.0;
    let f1 = 1_000.0;
    let n = 16_384;
    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    s.set_drive(3.0);
    s.set_smoothness(0.7);
    s.set_hf_smoothing(0.0);
    let out = render(f1, fs, 0.6, n, |x| s.process(x));
    let h1 = goertzel(&out, f1, fs).max(1e-12);
    let h = |k: usize| 20.0 * (goertzel(&out, k as f64 * f1, fs) / h1).log10();
    let tail = (6..=10).map(h).fold(f64::NEG_INFINITY, f64::max);
    let h2 = h(2);
    println!("pushed: h2 {h2:.0} dB, worst h6..10 {tail:.0} dB");
    assert!(h2 > -30.0, "should keep low-order warmth: h2 {h2}");
    assert!(
        tail < -42.0,
        "pushed high-harmonic tail must decay (no fizz shelf): {tail} dB"
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn stereo_width_report() {
    // Quantify how much dual-mono SmoothSat widens the image (side/mid
    // energy ratio, dB — higher = wider) vs a tanh reference, swept over
    // smoothness. Stereo test signal: mid = 1 kHz, side = 1.3 kHz, with
    // L = M+S, R = M-S.
    let fs = 48_000.0;
    let n = 16_384usize;
    let pi = core::f64::consts::PI;
    let mid = |i: usize| 0.5 * (2.0 * pi * 1000.0 * i as f64 / fs).sin() as f32;
    let side = |i: usize| 0.3 * (2.0 * pi * 1300.0 * i as f64 / fs).sin() as f32;

    let (mut ms, mut ss) = (0.0f64, 0.0f64);
    for i in n..2 * n {
        let (m, s) = (mid(i), side(i));
        let (l, r) = (m + s, m - s);
        ms += ((l + r) as f64 * 0.5).powi(2);
        ss += ((l - r) as f64 * 0.5).powi(2);
    }
    let in_w = 10.0 * (ss / ms).log10();

    let (mut ms, mut ss) = (0.0f64, 0.0f64);
    for i in 0..2 * n {
        let (m, s) = (mid(i), side(i));
        let l = (2.5 * (m + s)).tanh();
        let r = (2.5 * (m - s)).tanh();
        if i >= n {
            ms += ((l + r) as f64 * 0.5).powi(2);
            ss += ((l - r) as f64 * 0.5).powi(2);
        }
    }
    let tanh_w = 10.0 * (ss / ms).log10();

    print!("stereo side/mid (dB, higher=wider): input {in_w:.1}  tanh {tanh_w:.1}");
    for &sm in &[0.0f32, 0.5, 0.95] {
        let mut a = SmoothSat::new();
        a.prepare(fs as f32);
        a.set_drive(2.5);
        a.set_smoothness(sm);
        a.set_hf_smoothing(0.0);
        let mut b = SmoothSat::new();
        b.prepare(fs as f32);
        b.set_drive(2.5);
        b.set_smoothness(sm);
        b.set_hf_smoothing(0.0);
        let (mut ms, mut ss) = (0.0f64, 0.0f64);
        for i in 0..2 * n {
            let (m, s) = (mid(i), side(i));
            let l = a.process(m + s);
            let r = b.process(m - s);
            if i >= n {
                ms += ((l + r) as f64 * 0.5).powi(2);
                ss += ((l - r) as f64 * 0.5).powi(2);
            }
        }
        print!("  sm{sm} {:.1}", 10.0 * (ss / ms).log10());
    }
    println!();

    // Inter-channel CORRELATION with a shared center + independent sides
    // (closer to real material): corr drop = decorrelation = widening.
    let m = |i: usize| 0.5 * (2.0 * pi * 1000.0 * i as f64 / fs).sin() as f32;
    let sl = |i: usize| 0.3 * (2.0 * pi * 1700.0 * i as f64 / fs).sin() as f32;
    let sr = |i: usize| 0.3 * (2.0 * pi * 2300.0 * i as f64 / fs).sin() as f32;
    let corr = |kind: i32, drive: f32, sm: f32| -> f64 {
        let mut a = SmoothSat::new();
        let mut b = SmoothSat::new();
        for s in [&mut a, &mut b] {
            s.prepare(fs as f32);
            s.set_drive(drive);
            s.set_smoothness(sm);
            s.set_hf_smoothing(0.0);
        }
        let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..2 * n {
            let (li, ri) = (m(i) + sl(i), m(i) + sr(i));
            let (lo, ro) = match kind {
                0 => (li, ri),                                   // input
                1 => ((drive * li).tanh(), (drive * ri).tanh()), // tanh
                _ => (a.process(li), b.process(ri)),             // smoothsat
            };
            if i >= n {
                lr += lo as f64 * ro as f64;
                ll += (lo as f64).powi(2);
                rr += (ro as f64).powi(2);
            }
        }
        lr / (ll * rr).sqrt().max(1e-12)
    };
    println!(
        "L/R correlation (lower=wider): input {:.3}  tanh {:.3}  sm0 {:.3}  sm0.95 {:.3} | drive5 sm0 {:.3}",
        corr(0, 2.5, 0.0),
        corr(1, 2.5, 0.0),
        corr(2, 2.5, 0.0),
        corr(2, 2.5, 0.95),
        corr(2, 5.0, 0.0),
    );
}

#[test]
fn mid_side_keeps_center_better_than_dual_mono() {
    // The research fix: M/S with mid-favored drive keeps the phantom
    // center more correlated than dual-mono L/R. Signal = shared center
    // + independent sides.
    let fs = 48_000.0;
    let n = 16_384usize;
    let pi = core::f64::consts::PI;
    let m = |i: usize| 0.5 * (2.0 * pi * 1000.0 * i as f64 / fs).sin() as f32;
    let sl = |i: usize| 0.3 * (2.0 * pi * 1700.0 * i as f64 / fs).sin() as f32;
    let sr = |i: usize| 0.3 * (2.0 * pi * 2300.0 * i as f64 / fs).sin() as f32;
    let mk = |drive: f32| {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(drive);
        s.set_smoothness(0.3);
        s.set_hf_smoothing(0.0);
        s
    };
    let corr = |ls: &[f32], rs: &[f32]| -> f64 {
        let (mut lr, mut ll, mut rr) = (0.0f64, 0.0, 0.0);
        for i in 0..ls.len() {
            lr += ls[i] as f64 * rs[i] as f64;
            ll += (ls[i] as f64).powi(2);
            rr += (rs[i] as f64).powi(2);
        }
        lr / (ll * rr).sqrt().max(1e-12)
    };
    let drive = 3.0;
    // dual-mono L/R
    let (mut a, mut b) = (mk(drive), mk(drive));
    let (mut lo, mut ro) = (Vec::new(), Vec::new());
    for i in 0..2 * n {
        let (l, r) = (m(i) + sl(i), m(i) + sr(i));
        let (lp, rp) = (a.process(l), b.process(r));
        if i >= n {
            lo.push(lp);
            ro.push(rp);
        }
    }
    let lr_corr = corr(&lo, &ro);
    // M/S with mid-favored drive (sides at 0.3× the mid drive)
    let (mut mid_s, mut side_s) = (mk(drive), mk(drive * 0.3));
    let (mut lo2, mut ro2) = (Vec::new(), Vec::new());
    for i in 0..2 * n {
        let (l, r) = (m(i) + sl(i), m(i) + sr(i));
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r));
        let (mp, sp) = (mid_s.process(mid), side_s.process(side));
        if i >= n {
            lo2.push(mp + sp);
            ro2.push(mp - sp);
        }
    }
    let ms_corr = corr(&lo2, &ro2);
    println!("center correlation: L/R dual-mono {lr_corr:.3}, M/S mid-favored {ms_corr:.3}");
    assert!(
        ms_corr > lr_corr + 0.02,
        "M/S should keep the center more correlated ({lr_corr} vs {ms_corr})"
    );
}

#[test]
fn harmonic_profile_is_warm_and_smooth() {
    // At 1 kHz, full-ish level: 2nd/3rd present (warmth), high orders
    // steeply suppressed (no fizz). And smoothness must lower the high
    // orders without killing the low ones.
    let fs = 48_000.0;
    let f0 = 1_000.0;
    let n = 16_384;
    let profile = |smoothness: f32| -> [f64; 8] {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.0); // stays in range ⇒ pure Chebyshev profile
        s.set_hf_smoothing(0.0); // isolate the Chebyshev profile
        s.set_smoothness(smoothness);
        let out = render(f0, fs, 0.9, n, |x| s.process(x));
        let h1 = goertzel(&out, f0, fs).max(1e-12);
        let mut h = [0.0f64; 8];
        for k in 2..=8 {
            h[k - 1] = 20.0 * (goertzel(&out, k as f64 * f0, fs) / h1).log10();
        }
        h
    };
    let smooth = profile(0.8);
    let bright = profile(0.2);
    println!(
        "smooth(0.8) h2..h8 dB: {:?}",
        smooth
            .iter()
            .skip(1)
            .map(|v| (v * 10.0).round() / 10.0)
            .collect::<Vec<_>>()
    );
    println!(
        "bright(0.2) h2..h8 dB: {:?}",
        bright
            .iter()
            .skip(1)
            .map(|v| (v * 10.0).round() / 10.0)
            .collect::<Vec<_>>()
    );
    // Warmth: 2nd harmonic clearly present.
    assert!(
        smooth[1] > -45.0,
        "2nd harmonic should be present: {}",
        smooth[1]
    );
    // Smooth: high orders far below the 2nd.
    assert!(
        smooth[5] < smooth[1] - 12.0,
        "high orders not suppressed (h2 {}, h6 {})",
        smooth[1],
        smooth[5]
    );
    // Smoothness control works: more smoothness ⇒ lower high orders.
    // With the floored decay base (DECAY_MAX = 0.7) even "bright" has
    // a decaying tail, so the contrast is clearest at h4 (measured
    // ~15 dB) and shrinks with order (h6 ~5 dB).
    assert!(
        smooth[3] < bright[3] - 10.0,
        "smoothness should cut h4 ({} vs {})",
        smooth[3],
        bright[3]
    );
    assert!(
        smooth[5] < bright[5] - 4.0,
        "smoothness should cut h6 ({} vs {})",
        smooth[5],
        bright[5]
    );
}

#[test]
fn highs_distort_less_than_lows() {
    // Companding ⇒ a high tone generates less THD than a low tone at
    // the same level (without just sounding filtered — response is flat).
    let fs = 48_000.0;
    let n = 16_384;
    let thd_at = |f0: f64| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.6);
        s.set_hf_smoothing(8.0);
        let out = render(f0, fs, 0.6, n, |x| s.process(x));
        let h1 = goertzel(&out, f0, fs).max(1e-12);
        (2..=5)
            .map(|k| goertzel(&out, k as f64 * f0, fs))
            .map(|h| h * h)
            .sum::<f64>()
            .sqrt()
            / h1
    };
    let lo = thd_at(400.0);
    let hi = thd_at(5_000.0);
    println!("THD: 400 Hz {:.1}%, 5 kHz {:.1}%", lo * 100.0, hi * 100.0);
    assert!(hi < lo * 0.6, "highs should distort less ({lo} vs {hi})");
}

#[test]
fn companding_reduces_intermod() {
    // Two high tones (10 + 11 kHz). The audible IMD is the 1 kHz
    // difference tone. Companding must cut it hard.
    let fs = 48_000.0;
    let n = 8192usize;
    let b1 = 1707usize; // ~10.00 kHz (exact bin)
    let b2 = 1878usize; // ~11.00 kHz
    let f1 = b1 as f64 * fs / n as f64;
    let f2 = b2 as f64 * fs / n as f64;
    let diff = (b2 - b1) as f64 * fs / n as f64; // ~1.00 kHz IMD bin
    let imd_for = |hf: f32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.8);
        s.set_smoothness(0.6);
        s.set_hf_smoothing(hf);
        let two = |i: usize| {
            0.4 * ((2.0 * core::f64::consts::PI * f1 * i as f64 / fs).sin()
                + (2.0 * core::f64::consts::PI * f2 * i as f64 / fs).sin()) as f32
        };
        for i in 0..n {
            let _ = s.process(two(i));
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = s.process(two(i));
        }
        goertzel(&out, diff, fs)
    };
    // Reference: a plain tanh at comparable drive.
    let tanh_imd = {
        let two = |i: usize| {
            0.4 * ((2.0 * core::f64::consts::PI * f1 * i as f64 / fs).sin()
                + (2.0 * core::f64::consts::PI * f2 * i as f64 / fs).sin()) as f32
        };
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = (1.8 * two(i)).tanh();
        }
        goertzel(&out, diff, fs)
    };
    let _ = tanh_imd; // (tanh is purely odd → no even-order difference
                      // tone, so it's not a fair baseline for THIS product; kept only as
                      // a sanity reference in the print.)
    let off = imd_for(0.0);
    let on = imd_for(10.0);
    println!(
        "1 kHz IMD difference tone: hf=0 {off:.2e} | hf=10 {on:.2e} ({:+.1} dB) | (tanh ref {tanh_imd:.1e})",
        20.0 * (on / off.max(1e-12)).log10()
    );
    // Companding must dramatically cut the audible HF intermod product.
    assert!(
        on < off * 0.25,
        "companding should cut IMD ≥12 dB (hf0 {off}, hf10 {on})"
    );
}

#[test]
fn dual_mono_is_coherent() {
    // No AGC/dynamics ⇒ two independent instances given identical input
    // produce bit-identical output, so a centred (mono) signal stays
    // centred — nothing pumps the image. (Stereo content still gets
    // independent harmonics, which is normal for any dual-mono sat.)
    let fs = 48_000.0;
    let mk = || {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(2.0);
        s.set_hf_smoothing(6.0);
        s
    };
    let (mut l, mut r) = (mk(), mk());
    let mut max_diff = 0.0f32;
    for i in 0..8192 {
        let x = 0.7 * (2.0 * core::f64::consts::PI * 730.0 * i as f64 / fs).sin() as f32
            + 0.2 * (2.0 * core::f64::consts::PI * 3100.0 * i as f64 / fs).sin() as f32;
        let dl = l.process(x);
        let dr = r.process(x);
        max_diff = max_diff.max((dl - dr).abs());
    }
    assert!(
        max_diff < 1e-7,
        "channels diverge (mono would gain sides): {max_diff}"
    );
}

#[test]
fn bias_creates_even_harmonic_asymmetry() {
    // With an ODD-only curve (no even weight) the transfer is symmetric
    // ⇒ ~no 2nd harmonic at bias 0. A nonzero bias offsets the operating
    // point ⇒ asymmetry ⇒ the 2nd harmonic appears.
    let fs = 48_000.0;
    let f0 = 1_000.0;
    let n = 16_384;
    let h2_for = |bias: f32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(1.2);
        s.set_even(0.0); // odd-only → symmetric when bias = 0
        s.set_odd(0.3);
        s.set_smoothness(0.6);
        s.set_hf_smoothing(0.0);
        s.set_bias(bias);
        let out = render(f0, fs, 0.7, n, |x| s.process(x));
        let h1 = goertzel(&out, f0, fs).max(1e-12);
        20.0 * (goertzel(&out, 2.0 * f0, fs) / h1).log10()
    };
    let sym = h2_for(0.0);
    let biased = h2_for(0.5);
    println!("h2: bias 0 {sym:.1} dB, bias 0.5 {biased:.1} dB");
    assert!(
        sym < -50.0,
        "odd-only curve should be ~symmetric at bias 0: {sym}"
    );
    assert!(
        biased > sym + 20.0,
        "bias should add even harmonics ({sym} → {biased})"
    );
}

#[test]
fn static_bias_still_silences() {
    // A held bias offsets the curve, but silence must still settle to
    // silence (the output DC blocker removes the bias-induced DC).
    let fs = 48_000.0;
    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    s.set_bias(0.6);
    let mut tail = 0.0f32;
    let total = (fs * 1.5) as usize;
    let tail_start = (fs * 1.2) as usize;
    for i in 0..total {
        let y = s.process(0.0).abs();
        if i >= tail_start {
            tail = tail.max(y);
        }
    }
    println!("static-bias silence tail: {tail:.2e}");
    assert!(tail < 1e-3, "static bias left a DC offset: {tail}");
}

#[test]
fn stable_and_level_sane() {
    let fs = 48_000.0;
    let mut s = SmoothSat::new();
    s.prepare(fs as f32);
    s.set_drive(4.0);
    s.set_hf_smoothing(12.0);
    let out = render(220.0, fs, 0.95, 16_384, |x| s.process(x));
    assert!(out.iter().all(|x| x.is_finite()));
    let mx = out.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    assert!(mx < 6.0, "runaway: {mx}");
}
