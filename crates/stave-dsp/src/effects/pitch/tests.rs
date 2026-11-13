use super::stretch::hops;
use super::*;
use crate::primitives::spectral::{Complex, Fft, Window};
use crate::units::{hz, semitones};
use crate::Stage;
use core::f64::consts::TAU;

const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];
const BOTH: [Reconstruction; 2] = [Reconstruction::Prusa, Reconstruction::LarocheDolson];

/// The Hann-windowed DFT of `x` at `f` Hz.
fn dft(x: &[f32], f: f64, fs: f64) -> Complex<f64> {
    let n = x.len();
    let step = Complex::new((TAU * f / fs).cos(), -(TAU * f / fs).sin());
    let mut rot = Complex::new(1.0, 0.0);
    let mut acc = Complex::new(0.0, 0.0);
    for (k, &v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (TAU * k as f64 / n as f64).cos();
        acc = acc + rot.scale(f64::from(v) * w);
        rot = rot * step;
        if k % 1024 == 0 {
            rot = rot.scale(1.0 / rot.norm());
        }
    }
    acc
}

/// The frequency of the strongest partial within 6 % of `near`: a
/// coarse scan of the windowed DFT, then the phase advance between the
/// two halves of `x`.
fn frequency(x: &[f32], fs: f64, near: f64) -> f64 {
    let n = x.len();
    let step = fs / (4.0 * n as f64);
    let (lo, hi) = (near * 0.94, near * 1.06);
    let mut best = (0.0, near);
    let mut f = lo;
    while f <= hi {
        let m = dft(x, f, fs).norm();
        if m > best.0 {
            best = (m, f);
        }
        f += step;
    }
    let fc = best.1;
    let half = n / 2;
    let a = dft(&x[..half], fc, fs).arg();
    let b = dft(&x[half..2 * half], fc, fs).arg();
    let lag = half as f64;
    let d = b - a - TAU * fc * lag / fs;
    let d = d - TAU * (d / TAU).round();
    fc + d * fs / (TAU * lag)
}

fn cents(got: f64, want: f64) -> f64 {
    1200.0 * (got / want).log2()
}

/// A power-of-two length of about `seconds` at `fs`.
fn about(seconds: f64, fs: f32) -> usize {
    1 << (seconds * f64::from(fs)).log2().round() as u32
}

#[test]
fn vocoder_shifts_pitch_to_the_cent() {
    let mut worst = 0.0f64;
    for rate in RATES {
        let fs = f64::from(rate);
        for reconstruction in BOTH {
            for shift in [12.0f32, -12.0, 7.0] {
                for f0 in [220.0f64, 1_500.0] {
                    let mut pv = PhaseVocoder::new(reconstruction, semitones(12.0));
                    pv.set_shift(semitones(shift));
                    pv.prepare(rate);
                    let start = pv.latency() as usize + 2 * pv.window_samples();
                    let len = about(0.34, rate);
                    let y: Vec<f32> = (0..start + len)
                        .map(|t| pv.tick(0.5 * (TAU * f0 * t as f64 / fs).sin() as f32))
                        .collect();
                    let want = f0 * f64::from(shift / 12.0).exp2();
                    let got = frequency(&y[start..], fs, want);
                    let c = cents(got, want);
                    worst = worst.max(c.abs());
                    assert!(c.abs() < 1.0, "{reconstruction:?} {rate} {shift} st {f0} Hz: {c} cents");
                }
            }
        }
    }
    println!("PhaseVocoder pitch: worst {worst:.4} cents");
}

/// The mean RMS time spread of each click's energy, ms, around where it
/// should land.
fn click_spread_ms(y: &[f32], at: impl Iterator<Item = usize>, half: usize, fs: f64) -> f64 {
    let mut spreads = Vec::new();
    for t in at {
        let seg = &y[t - half..t + half];
        let e: Vec<f64> = seg.iter().map(|v| f64::from(*v).powi(2)).collect();
        let total: f64 = e.iter().sum();
        let mean = e.iter().enumerate().map(|(k, v)| k as f64 * v).sum::<f64>() / total;
        let var = e.iter().enumerate().map(|(k, v)| (k as f64 - mean).powi(2) * v).sum::<f64>() / total;
        spreads.push(var.sqrt() * 1e3 / fs);
    }
    spreads.iter().sum::<f64>() / spreads.len() as f64
}

#[test]
fn vocoder_keeps_clicks_sharp() {
    let rate = 48_000.0;
    let fs = f64::from(rate);
    let period = 4_800;
    for shift in [12.0f32, -12.0] {
        let mut spread = [0.0; 2];
        for (i, reconstruction) in BOTH.into_iter().enumerate() {
            let mut pv = PhaseVocoder::new(reconstruction, semitones(12.0));
            pv.set_shift(semitones(shift));
            pv.prepare(rate);
            let latency = pv.latency() as usize;
            let y: Vec<f32> = (0..latency + 12 * period)
                .map(|t| pv.tick(if t % period == 0 { 1.0 } else { 0.0 }))
                .collect();
            spread[i] = click_spread_ms(&y, (3..11).map(|k| latency + k * period), period / 2, fs);
        }
        println!("clicks at {shift} st: RMS spread Prusa {:.3} ms, LarocheDolson {:.3} ms", spread[0], spread[1]);
        assert!(spread[0] < spread[1], "{shift}: Průša's integration keeps clicks sharper");
        assert!(spread[0] < 2.0, "{shift}: {} ms", spread[0]);
    }
}

#[test]
fn vocoder_latency_is_reported() {
    let rate = 48_000.0;
    let fs = f64::from(rate);
    // Identity locking smears an impulse across a frame when shifting,
    // so its centroid says little; it is checked unshifted.
    let cases = [
        (Reconstruction::Prusa, 0.0f32),
        (Reconstruction::Prusa, 12.0),
        (Reconstruction::Prusa, -12.0),
        (Reconstruction::LarocheDolson, 0.0),
    ];
    for (reconstruction, shift) in cases {
        {
            let mut pv = PhaseVocoder::new(reconstruction, semitones(12.0));
            pv.set_shift(semitones(shift));
            pv.prepare(rate);
            let latency = pv.latency() as usize;
            let at = 6_000;
            let y: Vec<f32> = (0..at + latency + 4_000)
                .map(|t| pv.tick(if t == at { 1.0 } else { 0.0 }))
                .collect();
            let seg = &y[at + latency - 2_000..at + latency + 2_000];
            let e: Vec<f64> = seg.iter().map(|v| f64::from(*v).powi(2)).collect();
            let centroid = e.iter().enumerate().map(|(k, v)| k as f64 * v).sum::<f64>() / e.iter().sum::<f64>();
            let off = centroid - 2_000.0;
            println!("{reconstruction:?} {shift} st: impulse centroid {off:.2} samples from the latency");
            assert!(off.abs() * 1e3 / fs < 0.5, "{reconstruction:?} {shift}: {off} samples");
        }
    }
}

/// Hann STFT magnitudes of `x` for frames centred at `n·hop`, `frames`
/// of them from `first`.
fn magnitudes(x: &[f32], size: usize, hop: usize, first: usize, frames: usize) -> Vec<Vec<f64>> {
    let fft = Fft::<f64>::new(size);
    let w = Window::Hann.periodic(size);
    let mut buf = vec![0.0f64; size];
    let mut s = vec![Complex::default(); size / 2 + 1];
    (first..first + frames)
        .map(|n| {
            let begin = (n * hop) as i64 - (size / 2) as i64;
            for (k, b) in buf.iter_mut().enumerate() {
                let i = begin + k as i64;
                *b = if (0..x.len() as i64).contains(&i) { f64::from(x[i as usize]) * w[k] } else { 0.0 };
            }
            fft.forward_real(&buf, &mut s);
            s.iter().map(|z| z.norm()).collect()
        })
        .collect()
}

/// Spectral convergence, dB: `‖|STFT_s(y)| − |STFT_a(x)|‖ / ‖|STFT_a(x)|‖`.
fn spectral_convergence(x: &[f32], ratio: f64, reconstruction: Reconstruction, size: usize) -> f64 {
    let y = time_stretch(x, ratio, reconstruction, size);
    let (aa, as_, _) = hops(ratio, size);
    let frames = ((x.len() - size) / aa).min((y.samples.len() - size) / as_) - 2;
    let target = magnitudes(x, size, aa, 2, frames);
    let got = magnitudes(&y.samples, size, as_, 2, frames);
    let (mut num, mut den) = (0.0, 0.0);
    for (t, g) in target.iter().zip(&got) {
        for (a, b) in t.iter().zip(g) {
            num += (a - b).powi(2);
            den += a * a;
        }
    }
    10.0 * (num / den).log10()
}

#[test]
fn stretch_spectral_convergence() {
    let fs = 22_050.0f64;
    let len = 33_075; // 1.5 s
    // Průša & Holighaus's figure 1: a sinusoid, an exponential chirp
    // and an impulse.
    let mix: Vec<f32> = (0..len)
        .map(|t| {
            let s = t as f64 / fs;
            let chirp = (TAU * 100.0 * (50f64.powf(s / 1.5) - 1.0) * 1.5 / 50f64.ln()).sin();
            let click = if t == len / 2 { 8.0 } else { 0.0 };
            (0.3 * (TAU * 1_000.0 * s).sin() + 0.3 * chirp + click) as f32
        })
        .collect();
    let clicks: Vec<f32> = (0..len).map(|t| if t % 2_205 == 0 { 1.0 } else { 0.0 }).collect();
    // A harmonic tone with vibrato.
    let tone: Vec<f32> = {
        let mut phase = 0.0f64;
        (0..len)
            .map(|t| {
                let f = 220.0 * (1.0 + 0.01 * (TAU * 5.0 * t as f64 / fs).sin());
                phase += TAU * f / fs;
                (1..12).map(|k| (k as f64 * phase).sin() / k as f64).sum::<f64>() as f32 * 0.3
            })
            .collect()
    };
    for (name, signal) in [("mix", &mix), ("clicks", &clicks), ("vibrato", &tone)] {
        for ratio in [0.75, 1.5, 2.0] {
            let p = spectral_convergence(signal, ratio, Reconstruction::Prusa, 1_024);
            let l = spectral_convergence(signal, ratio, Reconstruction::LarocheDolson, 1_024);
            println!("spectral convergence {name} ×{ratio}: Prusa {p:.2} dB, LarocheDolson {l:.2} dB");
            // Never worse than identity locking beyond 0.5 dB; on clicks,
            // where identity locking smears, better by a margin.
            if name == "vibrato" {
                // A stationary harmonic tone is identity locking's best
                // case (and PVDR's own caveat for voiced sound): both
                // stay well converged.
                assert!(p < -20.0 && l < -20.0, "{name} ×{ratio}: {p} / {l} dB");
                continue;
            }
            assert!(p < l + 0.5, "{name} ×{ratio}: Průša {p} dB against {l}");
            if name == "clicks" {
                assert!(p < l, "{name} ×{ratio}: Průša {p} dB against {l}");
            }
        }
    }
}

/// A band-limited sawtooth at `f0`.
fn saw(f0: f64, fs: f64, len: usize) -> Vec<f32> {
    let harmonics = (0.45 * fs / f0) as usize;
    (0..len)
        .map(|t| {
            let p = TAU * f0 * t as f64 / fs;
            (1..=harmonics).map(|k| (k as f64 * p).sin() / k as f64).sum::<f64>() as f32 * 0.3
        })
        .collect()
}

#[test]
fn psola_shifts_pitch_to_the_cent() {
    let mut worst = 0.0f64;
    for rate in RATES {
        let fs = f64::from(rate);
        for f0 in [98.0f64, 196.0, 392.0] {
            let len = about(0.3, rate);
            let mut psola = Psola::new(hz(60.0), hz(1_000.0));
            psola.prepare(rate);
            let start = psola.latency() as usize + (0.1 * fs) as usize;
            let x = saw(f0, fs, start + len);
            for shift in [12.0f32, -12.0, 7.0, -3.0, 0.0] {
                psola.set_shift(semitones(shift));
                psola.reset();
                let y: Vec<f32> = x.iter().map(|&v| psola.tick(v)).collect();
                let want = f0 * f64::from(shift / 12.0).exp2();
                let got = frequency(&y[start..], fs, want);
                let c = cents(got, want);
                worst = worst.max(c.abs());
                assert!(c.abs() < 3.0, "{rate} {f0} Hz {shift} st: {c} cents");
            }
        }
    }
    println!("Psola pitch: worst {worst:.3} cents");
}

/// The power-weighted mean frequency of `x` from `lo` to `hi` Hz.
fn centroid(x: &[f32], fs: f64, lo: f64, hi: f64) -> f64 {
    let n = x.len().next_power_of_two() / 2;
    let fft = Fft::<f64>::new(n);
    let w = Window::Hann.periodic(n);
    let buf: Vec<f64> = x[..n].iter().zip(&w).map(|(a, b)| f64::from(*a) * b).collect();
    let mut s = vec![Complex::default(); n / 2 + 1];
    fft.forward_real(&buf, &mut s);
    let (mut num, mut den) = (0.0, 0.0);
    for (k, z) in s.iter().enumerate() {
        let f = k as f64 * fs / n as f64;
        if (lo..=hi).contains(&f) {
            num += f * z.norm_sqr();
            den += z.norm_sqr();
        }
    }
    num / den
}

#[test]
fn psola_keeps_the_formant() {
    let rate = 48_000.0;
    let fs = f64::from(rate);
    let f0 = 150.0;
    // A pulse train through a 1 kHz resonance (Q 5), synthesised by
    // its harmonics.
    let resonance = |f: f64| {
        let r = f / 1_000.0;
        1.0 / ((1.0 - r * r).powi(2) + (r / 5.0).powi(2)).sqrt()
    };
    let mut psola = Psola::new(hz(60.0), hz(1_000.0));
    psola.prepare(rate);
    let start = psola.latency() as usize + 4_800;
    let len = start + 32_768;
    let x: Vec<f32> = (0..len)
        .map(|t| {
            let p = TAU * f0 * t as f64 / fs;
            (1..60).map(|k| resonance(k as f64 * f0) * (k as f64 * p).cos()).sum::<f64>() as f32 * 0.02
        })
        .collect();
    let input = centroid(&x[start..], fs, 400.0, 2_500.0);
    for shift in [5.0f32, -5.0] {
        psola.set_shift(semitones(shift));
        psola.reset();
        let y: Vec<f32> = x.iter().map(|&v| psola.tick(v)).collect();
        let out = centroid(&y[start..], fs, 400.0, 2_500.0);
        let moved = out / input - 1.0;
        println!("Psola {shift} st: formant centroid {input:.0} → {out:.0} Hz ({:+.1} %)", 100.0 * moved);
        assert!(moved.abs() < 0.05, "{shift}: the formant moved {moved}");
    }
}
