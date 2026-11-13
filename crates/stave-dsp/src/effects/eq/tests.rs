use super::*;
use crate::primitives::spectral::{Complex, Fft, Fir};
use crate::units::{db, hz, ms, Ratio};
use crate::Stage;
use stave_measure::zipper::{zipper_report, ZipperConfig};

const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];

/// Five bands across the range: the set every EQ test runs.
fn five_bands() -> [Band; 6] {
    [
        Band::LowCut { frequency: hz(30.0), q: 0.707 },
        Band::LowShelf { frequency: hz(120.0), gain: db(4.0) },
        Band::Bell { frequency: hz(400.0), gain: db(-6.0), q: 1.5 },
        Band::Bell { frequency: hz(3_000.0), gain: db(5.0), q: 3.0 },
        Band::HighShelf { frequency: hz(10_000.0), gain: db(-3.0) },
        Band::HighCut { frequency: hz(18_000.0), q: 0.707 },
    ]
}

/// The cookbook cascade's gain at `f`.
fn cookbook_db(bands: &[Band], f: f64, rate: f32) -> f64 {
    bands.iter().map(|b| b.gain_db(f, rate)).sum()
}

/// Log-spaced frequencies from `lo` to `hi`, 24 per octave.
fn log_grid(lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let n = ((hi / lo).log2() * 24.0).ceil() as usize;
    (0..=n).map(move |k| lo * (hi / lo).powf(k as f64 / n as f64))
}

/// `stage`'s impulse response, `len` samples.
fn impulse(stage: &mut dyn Stage, len: usize) -> Vec<f32> {
    (0..len).map(|n| stage.tick(if n == 0 { 1.0 } else { 0.0 })).collect()
}

/// Gain in dB at bin `k` of an impulse response zero-padded to `n`.
fn spectrum_db(h: &[f32], n: usize) -> Vec<f64> {
    let fft = Fft::<f64>::new(n);
    let mut x = vec![0.0f64; n];
    for (a, &b) in x.iter_mut().zip(h) {
        *a = f64::from(b);
    }
    let mut s = vec![Complex::default(); n / 2 + 1];
    fft.forward_real(&x, &mut s);
    s.iter().map(|z| 20.0 * z.norm().max(1e-300).log10()).collect()
}

#[test]
fn minimum_phase_is_the_biquads() {
    let bands = five_bands();
    for rate in RATES {
        let mut eq = MinimumPhase::new(&bands);
        eq.prepare(rate);
        assert_eq!(eq.latency(), 0);
        let n = 1 << 17;
        let db = spectrum_db(&impulse(&mut eq, n), n);
        let mut worst = 0.0f64;
        for k in 1..n / 2 {
            let f = k as f64 * f64::from(rate) / n as f64;
            if !(20.0..=20_000.0).contains(&f) {
                continue;
            }
            worst = worst.max((db[k] - cookbook_db(&bands, f, rate)).abs());
        }
        assert!(worst < 1e-3, "{rate}: {worst} dB from the cookbook");
    }
}

#[test]
fn linear_phase_matches_the_biquads() {
    let bands = five_bands();
    for rate in RATES {
        let mut eq = LinearPhase::new(&bands);
        eq.prepare(rate);
        let fir = Fir::from_taps(eq.impulse_response().iter().map(|&h| f64::from(h)).collect());
        let fs = f64::from(rate);
        let (mut above_40, mut above_20) = (0.0f64, 0.0f64);
        for f in log_grid(20.0, 20_000.0) {
            let err = (fir.gain_db(f / fs) - cookbook_db(&bands, f, rate)).abs();
            above_20 = above_20.max(err);
            if f >= 40.0 {
                above_40 = above_40.max(err);
            }
        }
        println!("LinearPhase {rate}: {} taps, worst {above_40:.4} dB from 40 Hz, {above_20:.4} from 20 Hz", fir.len());
        assert!(above_40 < 0.05, "{rate}: {above_40} dB from 40 Hz");
        assert!(above_20 < 0.1, "{rate}: {above_20} dB from 20 Hz");
        // The convolver runs exactly these taps, at the latency.
        let latency = eq.latency() as usize;
        let taps = eq.impulse_response().to_vec();
        let y = impulse(&mut eq, latency + taps.len());
        let lead = latency - (taps.len() - 1) / 2;
        let off = y[lead..].iter().zip(&taps).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(off < 1e-5, "{rate}: the convolver is {off} off the design");
    }
}

#[test]
fn linear_phase_is_exactly_linear() {
    for rate in RATES {
        let mut eq = LinearPhase::new(&five_bands());
        eq.prepare(rate);
        let latency = eq.latency() as usize;
        let y = impulse(&mut eq, 2 * latency + 1);
        let peak = y.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let centre = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
        assert_eq!(centre, latency, "{rate}: the impulse lands at the latency");
        let lead = latency - (eq.taps() - 1) / 2;
        let asym = (1..latency - lead)
            .map(|k| (y[latency + k] - y[latency - k]).abs())
            .fold(0.0f32, f32::max);
        assert!(asym <= 1e-7 * peak, "{rate}: asymmetry {asym}");
        // Group delay: Re(Σ n·h[n]·e^{−iωn} / Σ h[n]·e^{−iωn}).
        let h = &y[lead..];
        let fs = f64::from(rate);
        let mut worst = 0.0f64;
        for f in log_grid(20.0, 20_000.0) {
            let w = core::f64::consts::TAU * f / fs;
            let (mut a, mut b) = (Complex::new(0.0, 0.0), Complex::new(0.0, 0.0));
            for (n, &v) in h.iter().enumerate() {
                let z = Complex::new((w * n as f64).cos(), -(w * n as f64).sin()).scale(f64::from(v));
                a = a + z;
                b = b + z.scale(n as f64);
            }
            if a.norm() < 0.01 * f64::from(peak) {
                continue; // below −40 dB re the peak tap's scale
            }
            let ratio = b * a.conj();
            let tau = ratio.re / a.norm_sqr();
            worst = worst.max((tau - (latency - lead) as f64).abs());
        }
        assert!(worst < 0.01, "{rate}: group delay off by {worst} samples");
    }
}

/// How long the response stays above −60 dB re its peak ahead of the
/// main tap: the pre-ringing the rustdoc quotes.
fn pre_ringing_ms(band: Band, rate: f32) -> f64 {
    let mut eq = LinearPhase::new(&[band]);
    eq.prepare(rate);
    let taps = eq.impulse_response().to_vec();
    let m = (taps.len() - 1) / 2;
    // The ringing, without the impulse a bell or shelf passes.
    let ring: Vec<f32> = taps.iter().enumerate().map(|(i, &h)| if i == m { 0.0 } else { h }).collect();
    let peak = taps.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    let first = ring.iter().position(|h| h.abs() > 1e-3 * peak).unwrap_or(m);
    (m - first) as f64 * 1e3 / f64::from(rate)
}

#[test]
fn linear_phase_pre_ringing_is_as_documented() {
    let rate = 48_000.0;
    let q1 = pre_ringing_ms(Band::Bell { frequency: hz(1_000.0), gain: db(6.0), q: 1.0 }, rate);
    let q8 = pre_ringing_ms(Band::Bell { frequency: hz(1_000.0), gain: db(6.0), q: 8.0 }, rate);
    let cut = pre_ringing_ms(Band::LowCut { frequency: hz(100.0), q: 0.707 }, rate);
    println!("pre-ringing above −60 dB: bell Q1 {q1:.2} ms, Q8 {q8:.2} ms, low cut 100 Hz {cut:.2} ms");
    assert!(q1 < q8, "a narrower bell rings longer");
    assert!(q1 < 2.0 && q8 < 7.0 && cut < 5.0, "{q1} {q8} {cut}");
}

/// A coherent 1 kHz-ish tone of `n` samples on bin `bin`.
fn tone(n: usize, bin: usize, amp: impl Fn(usize) -> f32) -> Vec<f32> {
    (0..n)
        .map(|t| amp(t) * (core::f32::consts::TAU * (bin * t % n) as f32 / n as f32).sin())
        .collect()
}

#[test]
fn linear_phase_band_changes_do_not_click() {
    let rate = 48_000.0;
    let n = 1 << 15;
    let bin = 683; // 1000.5 Hz
    let f = bin as f32 * rate / n as f32;
    let mut eq = LinearPhase::new(&[Band::Bell { frequency: hz(f), gain: db(0.0), q: 1.0 }]);
    eq.prepare(rate);
    let latency = eq.latency() as usize;
    let x = tone(n + latency, bin, |_| 0.5);
    let mut y = Vec::with_capacity(n);
    for (t, &v) in x.iter().enumerate() {
        // Automation: −1 dB every 1024 samples once the output starts.
        if t >= latency && (t - latency) % 1_024 == 0 {
            let step = ((t - latency) / 1_024).min(12) as f32;
            eq.set_band(0, Band::Bell { frequency: hz(f), gain: db(-step), q: 1.0 });
        }
        let out = eq.tick(v);
        if t >= latency {
            y.push(f64::from(out));
        }
    }
    let report = zipper_report(
        &y,
        &ZipperConfig {
            carrier_bin: bin,
            sample_rate: f64::from(rate),
            guard_hz: 400.0,
            low_cut_hz: 20.0,
        },
    );
    println!("LinearPhase automation spray {:.1} dB, worst {:?}", report.spray_db, report.worst);
    assert!(report.spray_db < -60.0, "spray {} dB", report.spray_db);
}

/// The dynamic bands' static twins: never past threshold.
fn resting(bands: &[Band], phase: Phase) -> Vec<DynamicBand> {
    let asleep = Dynamics {
        threshold: db(100.0),
        ratio: Ratio::new(4.0),
        knee: db(0.0),
        range: db(-12.0),
        attack: ms(1.0),
        release: ms(50.0),
        detector: Detector::Peak,
    };
    bands.iter().map(|&b| DynamicBand::dynamic(b, phase, asleep)).collect()
}

#[test]
fn dynamic_bands_match_the_biquads_at_rest() {
    let bands = five_bands();
    for rate in [44_100.0, 96_000.0] {
        for phase in [Phase::Minimum, Phase::Linear] {
            let mut eq = Dynamic::new(&resting(&bands, phase));
            eq.prepare(rate);
            let latency = eq.latency() as usize;
            let n = 1 << 17;
            let h = impulse(&mut eq, n + latency);
            let lead = if latency > 0 { linear_phase::BLOCK } else { 0 };
            let db = spectrum_db(&h[lead..], n);
            let mut worst = 0.0f64;
            for k in 1..n / 2 {
                let f = k as f64 * f64::from(rate) / n as f64;
                if (40.0..=20_000.0).contains(&f) {
                    worst = worst.max((db[k] - cookbook_db(&bands, f, rate)).abs());
                }
            }
            println!("Dynamic {phase:?} {rate}: {worst:.5} dB at rest");
            let budget = if phase == Phase::Minimum { 1e-4 } else { 0.05 };
            assert!(worst < budget, "{phase:?} {rate}: {worst} dB");
        }
    }
    // A minimum-phase section at fixed gains is its biquad.
    for gain in [-15.0f32, -3.0, 0.0, 2.5, 12.0] {
        for band in five_bands() {
            let band = band.with_gain(db(gain));
            let rate = 48_000.0;
            let mut s = section::Section::new(band, rate);
            let n = 1 << 16;
            let h: Vec<f32> = (0..n).map(|t| s.tick(if t == 0 { 1.0 } else { 0.0 })).collect();
            let got = spectrum_db(&h, n);
            for k in (28..n / 2).step_by(97) {
                let f = k as f64 * f64::from(rate) / n as f64;
                if f > 20_000.0 {
                    break;
                }
                let want = band.gain_db(f, rate);
                assert!((got[k] - want).abs() < 1e-4, "{band:?} at {f}: {} vs {want}", got[k]);
            }
        }
    }
}

/// The level after `stage` of a held tone at `f`, dB, measured over the
/// last quarter of `len` samples.
fn held_gain_db(stage: &mut dyn Stage, f: f32, amp: f32, rate: f32, len: usize) -> f64 {
    let w = core::f64::consts::TAU * f64::from(f) / f64::from(rate);
    let y: Vec<f32> = (0..len).map(|t| stage.tick(amp * (w * t as f64).sin() as f32)).collect();
    let tail = &y[3 * len / 4..];
    let rms = (tail.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
    20.0 * (rms * 2f64.sqrt() / f64::from(amp)).log10()
}

#[test]
fn dynamic_bands_reach_their_static_curve() {
    let rate = 48_000.0;
    let curve = Dynamics {
        threshold: db(-30.0),
        ratio: Ratio::new(3.0),
        knee: db(0.0),
        range: db(-15.0),
        attack: ms(2.0),
        release: ms(20.0),
        detector: Detector::Rms(ms(20.0)),
    };
    for phase in [Phase::Minimum, Phase::Linear] {
        for (level_db, static_db) in [(-40.0f32, 0.0f32), (-18.0, 0.0), (-12.0, 3.0), (-6.0, -2.0)] {
            let band = Band::Bell { frequency: hz(1_000.0), gain: db(static_db), q: 2.0 };
            let mut eq = Dynamic::new(&[DynamicBand::dynamic(band, phase, curve)]);
            eq.prepare(rate);
            let amp = 10f32.powf(level_db / 20.0);
            let got = held_gain_db(&mut eq, 1_000.0, amp, rate, 48_000);
            // The key is the band-passed tone: its RMS is the tone's.
            let rms_db = level_db - 10.0 * 2f32.log10();
            let computer = crate::effects::dynamics::GainComputer {
                threshold_db: -30.0,
                ratio: 3.0,
                knee_db: 0.0,
            };
            let want = f64::from(static_db + computer.gain_db(rms_db).max(-15.0));
            println!("{phase:?} at {level_db} dB, static {static_db}: {got:.3} dB, want {want:.3}");
            assert!((got - want).abs() < 0.1, "{phase:?} {level_db}: {got} vs {want}");
        }
    }
}

#[test]
fn dynamic_bands_do_not_click() {
    let rate = 48_000.0;
    let n = 1 << 15;
    let bin = 683;
    let f = bin as f32 * rate / n as f32;
    let fast = Dynamics {
        threshold: db(-30.0),
        ratio: Ratio::new(8.0),
        knee: db(0.0),
        range: db(-12.0),
        attack: ms(3.0),
        release: ms(40.0),
        // A peak detector's ripple at twice the tone modulates the gain
        // at audio rate (distortion, not a click): the RMS window does not.
        detector: Detector::Rms(ms(10.0)),
    };
    // Bursts: −40 to −6 dBFS and back every 2048 samples, 5 ms ramps.
    let envelope = |t: usize| {
        let phase = (t % 4_096) as f32;
        let ramp = 240.0;
        let x = if phase < 2_048.0 { (phase / ramp).min(1.0) } else { 1.0 - ((phase - 2_048.0) / ramp).min(1.0) };
        let s = 0.5 - 0.5 * (core::f32::consts::PI * x).cos();
        0.01 + (0.5 - 0.01) * s
    };
    let config = ZipperConfig {
        carrier_bin: bin,
        sample_rate: f64::from(rate),
        guard_hz: 1_000.0,
        low_cut_hz: 20.0,
    };
    for phase in [Phase::Minimum, Phase::Linear] {
        let band = Band::Bell { frequency: hz(f), gain: db(0.0), q: 2.0 };
        let mut eq = Dynamic::new(&[DynamicBand::dynamic(band, phase, fast)]);
        eq.prepare(rate);
        let latency = eq.latency() as usize;
        let x = tone(n + latency, bin, envelope);
        let y: Vec<f64> = x.iter().map(|&v| f64::from(eq.tick(v))).skip(latency).collect();
        let dry: Vec<f64> = x[..n].iter().map(|&v| f64::from(v)).collect();
        let wet = zipper_report(&y, &config);
        let reference = zipper_report(&dry, &config);
        println!(
            "Dynamic {phase:?}: spray {:.1} dB (input's own {:.1}), worst {:?}",
            wet.spray_db, reference.spray_db, wet.worst
        );
        assert!(wet.spray_db < -60.0, "{phase:?}: {} dB", wet.spray_db);
        assert!(eq.gain_offset(0).get() < 0.0 || phase == Phase::Linear);
    }
}
