use super::*;
use crate::primitives::random::SplitMix64;
use stave_measure::spectrum::{goertzel, welch};

/// A busy signal: full-band noise well inside full scale.
fn busy(n: usize, seed: u64) -> Vec<f64> {
    let mut r = SplitMix64::new(seed);
    (0..n).map(|_| 0.05 * r.next_normal()).collect()
}

/// The total error `y − x` and its power, in units of `Δ²`.
fn error_power(q: &mut Quantiser, x: &[f64]) -> (Vec<f64>, f64, f64) {
    let step = q.step();
    let e: Vec<f64> = x.iter().map(|&x| q.quantise(x) - x).collect();
    let mean = e.iter().sum::<f64>() / e.len() as f64;
    let power = e.iter().map(|e| e * e).sum::<f64>() / e.len() as f64;
    (e, mean / step, power / (step * step))
}

#[test]
fn error_powers_match_theory() {
    let x = busy(1 << 18, 3);
    for (dither, expect) in [
        (Dither::None, 1.0 / 12.0),
        (Dither::Rectangular, 2.0 / 12.0),
        (Dither::Triangular, 0.25),
        (Dither::HighpassTriangular, 0.25),
    ] {
        let mut q = Quantiser::new(12).with_dither(dither, 9);
        let (_, mean, power) = error_power(&mut q, &x);
        assert!(mean.abs() < 0.01, "{dither:?}: mean {mean} Δ");
        assert!((power / expect - 1.0).abs() < 0.01, "{dither:?}: {power} Δ² against {expect}");
    }
    // 16-bit TPDF: Δ/2 rms is −96.33 dB re full scale.
    let mut q = Quantiser::new(16).with_dither(Dither::Triangular, 2);
    let (_, _, power) = error_power(&mut q, &x);
    let floor_db = 10.0 * (power * q.step() * q.step()).log10();
    assert!((floor_db + 96.33).abs() < 0.05, "{floor_db}");
}

#[test]
fn full_scale_sines_reach_the_textbook_snr() {
    // Undithered rounding of a busy full-scale sine: SNR 6.02·N + 1.76 dB.
    let n = 1 << 17;
    for bits in [8u32, 12, 16] {
        let mut q = Quantiser::new(bits);
        let a = 1.0 - q.step();
        let w = std::f64::consts::TAU * 0.123_456_789;
        let (mut signal, mut noise) = (0.0, 0.0);
        for i in 0..n {
            let x = a * (w * i as f64).sin();
            let e = q.quantise(x) - x;
            signal += x * x;
            noise += e * e;
        }
        let snr = 10.0 * (signal / noise).log10();
        let expect = 6.02 * f64::from(bits) + 1.76;
        assert!((snr - expect).abs() < 0.3, "{bits} bits: {snr:.2} dB against {expect:.2}");
    }
}

#[test]
fn tpdf_error_moments_do_not_depend_on_the_signal() {
    // A DC input swept across one step: TPDF keeps the error's mean at 0
    // and its power at Δ²/4 everywhere; RPDF's swings from 0 (on a
    // code) to Δ²/4 (halfway) (Wannamaker et al. 2000).
    let n = 1 << 18;
    let mut rpdf_range = (f64::MAX, f64::MIN);
    for i in 0..16 {
        let step = Quantiser::new(12).step();
        let offset = 0.1 + step * i as f64 / 16.0;
        let x = vec![offset; n];
        let mut q = Quantiser::new(12).with_dither(Dither::Triangular, 40 + i);
        let (_, mean, power) = error_power(&mut q, &x);
        assert!(mean.abs() < 0.006, "offset {i}/16: mean {mean} Δ");
        assert!((power / 0.25 - 1.0).abs() < 0.015, "offset {i}/16: {power} Δ²");
        let mut q = Quantiser::new(12).with_dither(Dither::Rectangular, 40 + i);
        let (_, _, power) = error_power(&mut q, &x);
        rpdf_range = (rpdf_range.0.min(power), rpdf_range.1.max(power));
    }
    assert!(rpdf_range.0 < 0.1 && rpdf_range.1 > 0.2, "RPDF modulates: {rpdf_range:?}");
}

/// Mean PSD over bins `lo..hi`.
fn band(psd: &[f64], lo: usize, hi: usize) -> f64 {
    psd[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
}

#[test]
fn shaped_error_spectra_follow_the_noise_transfer_function() {
    let fs = 48_000.0;
    let segment = 1024;
    let x = busy(1 << 18, 5);
    for shaping in [Shaping::NONE, Shaping::FIRST_ORDER, Shaping::SECOND_ORDER] {
        let mut q = Quantiser::new(16)
            .with_dither(Dither::Triangular, 11)
            .with_shaping(shaping)
            .unwrap();
        let step = q.step();
        let (e, _, _) = error_power(&mut q, &x);
        let psd = welch(&e, segment, fs);
        // White total error Δ²/4 over fs/2, through |1 − H|².
        let white = step * step / 4.0 / (fs / 2.0);
        let mut worst: f64 = 0.0;
        for lo in (32..512).step_by(32) {
            let hi = lo + 32;
            let measured = band(&psd, lo, hi);
            let expect = (lo..hi)
                .map(|k| white * shaping.power_gain(std::f64::consts::TAU * k as f64 / segment as f64))
                .sum::<f64>()
                / 32.0;
            worst = worst.max((10.0 * (measured / expect).log10()).abs());
        }
        assert!(worst < 0.3, "{shaping:?}: worst band {worst} dB");
    }
    // Highpass TPDF: Δ²/12 white plus (Δ²/12)·|1 − z⁻¹|².
    let mut q = Quantiser::new(16).with_dither(Dither::HighpassTriangular, 13);
    let step = q.step();
    let (e, _, _) = error_power(&mut q, &x);
    let psd = welch(&e, segment, fs);
    for lo in (32..512).step_by(96) {
        let measured = band(&psd, lo, lo + 32);
        let expect = (lo..lo + 32)
            .map(|k| {
                let w = std::f64::consts::TAU * k as f64 / segment as f64;
                step * step / 12.0 * (1.0 + Shaping::FIRST_ORDER.power_gain(w)) / (fs / 2.0)
            })
            .sum::<f64>()
            / 32.0;
        let db = 10.0 * (measured / expect).log10();
        assert!(db.abs() < 0.3, "bins {lo}: {db} dB");
    }
}

#[test]
fn undithered_low_level_tones_distort_and_dithered_ones_do_not() {
    // A tone of 1.3 steps' amplitude on a coherent bin: undithered, its
    // third harmonic stands ~ −20 dBc; TPDF leaves the harmonic at the
    // noise floor's level in that bin.
    let n = 1 << 16;
    let cycles = 331.0;
    for (dither, low) in [(Dither::None, false), (Dither::Triangular, true)] {
        let mut q = Quantiser::new(12).with_dither(dither, 17);
        let a = 1.3 * q.step();
        let y: Vec<f64> = (0..n)
            .map(|i| q.quantise(a * (std::f64::consts::TAU * cycles * i as f64 / n as f64).sin()))
            .collect();
        let h1 = goertzel(&y, cycles / n as f64);
        let h3 = goertzel(&y, 3.0 * cycles / n as f64);
        let dbc = 20.0 * (h3 / h1).log10();
        if low {
            // The floor in one bin: Δ/2 rms over n/2 bins.
            let floor = 20.0 * ((q.step() / 2.0) * (2.0 / n as f64).sqrt() * 2.0 / h1).log10();
            assert!(dbc < floor + 12.0, "TPDF H3 {dbc} dBc, floor {floor}");
        } else {
            assert!(dbc > -30.0, "undithered H3 {dbc} dBc");
        }
    }
}

#[test]
fn saturation_and_truncation_follow_the_word() {
    let q = Quantiser::new(12);
    assert_eq!(q.code(10.0), 2047);
    assert_eq!(q.code(-10.0), -2048);
    assert_eq!(q.code(f64::NAN), 0);
    let t = Quantiser::new(12).with_rounding(Rounding::Truncate);
    let step = t.step();
    assert_eq!(t.code(0.99 * step), 0);
    assert_eq!(t.code(-0.01 * step), -1);
    assert_eq!(q.code(0.5 * step), 1);
    assert_eq!(q.code(0.49 * step), 0);
    assert!(Quantiser::new(12).with_shaping(Shaping(&[0.0; 17])).is_err());
    // Overload never destabilises the shaper.
    let mut s = Quantiser::new(8).with_shaping(Shaping::SECOND_ORDER).unwrap();
    for i in 0..10_000 {
        let y = s.quantise(if i % 1000 < 500 { 5.0 } else { -5.0 });
        assert!(y.abs() <= 1.0);
    }
    assert!(s.quantise(0.0).abs() < 4.0 * s.step());
}

#[test]
fn seeded_dither_reproduces() {
    let x = busy(4096, 1);
    let mut a = Quantiser::new(16).with_dither(Dither::Triangular, 5);
    let mut b = Quantiser::new(16).with_dither(Dither::Triangular, 5);
    let ya: Vec<f64> = x.iter().map(|&x| a.quantise(x)).collect();
    let yb: Vec<f64> = x.iter().map(|&x| b.quantise(x)).collect();
    assert_eq!(ya, yb);
    a.reset();
    let again: Vec<f64> = x.iter().map(|&x| a.quantise(x)).collect();
    assert_eq!(ya, again);
}
