//! The Fourier tools the other metrics stand on: an in-place FFT (the
//! shared [`stave_dsp::Fft`]) and a single-bin Goertzel (f64,
//! bin-centred tones — see the crate docs for why measurement here never windows a stationary capture).

use stave_dsp::primitives::spectral::Complex;
use stave_dsp::Fft;

/// In-place forward FFT (f64) over split real and imaginary parts,
/// `re.len()` a power of two (at least 2), `im` the same length. Plans
/// [`stave_dsp::Fft`], the workspace's one transform, per call:
/// measurement is offline, so the plan is not kept.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two(), "fft length must be a power of two");
    assert_eq!(im.len(), n, "fft: re and im differ in length");
    if n < 2 {
        return;
    }
    let mut data: Vec<Complex<f64>> = re
        .iter()
        .zip(im.iter())
        .map(|(&r, &i)| Complex::new(r, i))
        .collect();
    Fft::new(n).forward(&mut data);
    for ((r, i), z) in re.iter_mut().zip(im.iter_mut()).zip(&data) {
        *r = z.re;
        *i = z.im;
    }
}

/// Amplitude of the component at `cycles_per_sample` (frequency ÷
/// sample rate). Exact when that component completes whole cycles
/// over `buf`.
pub fn goertzel<T: Copy + Into<f64>>(buf: &[T], cycles_per_sample: f64) -> f64 {
    let n = buf.len();
    if n == 0 {
        return 0.0;
    }
    let w = std::f64::consts::TAU * cycles_per_sample;
    let cw = w.cos();
    let coeff = 2.0 * cw;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in buf {
        let s0 = x.into() + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let re = s1 - s2 * cw;
    let im = s2 * w.sin();
    (re * re + im * im).sqrt() / (n as f64 * 0.5)
}

/// The one-sided power spectral density of a stationary random signal
/// by Welch's method (IEEE Trans. Audio Electroacoust. 15(2), 1967):
/// Hann-windowed segments of `segment` samples (a power of two), half
/// overlapping, each segment's mean removed, their periodograms averaged
/// and scaled so the PSD integrates to the signal's variance (units²/Hz
/// at `sample_rate`). Bin `k` is `k·sample_rate/segment` Hz,
/// `0..=segment/2`. Each bin's standard error is about `1/√segments`;
/// average neighbouring bins for a tighter estimate. For noise only: a
/// tone belongs to [`goertzel`], on a bin, unwindowed.
pub fn welch(signal: &[f64], segment: usize, sample_rate: f64) -> Vec<f64> {
    assert!(segment.is_power_of_two() && segment >= 4, "welch: segment a power of two");
    assert!(signal.len() >= segment, "welch: shorter than a segment");
    let window: Vec<f64> = (0..segment)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / segment as f64).cos())
        .collect();
    let energy: f64 = window.iter().map(|w| w * w).sum();
    let plan = Fft::new(segment);
    let mut psd = vec![0.0; segment / 2 + 1];
    let mut data = vec![Complex::new(0.0, 0.0); segment];
    let mut count = 0usize;
    let mut start = 0;
    while start + segment <= signal.len() {
        let chunk = &signal[start..start + segment];
        let mean = chunk.iter().sum::<f64>() / segment as f64;
        for ((z, x), w) in data.iter_mut().zip(chunk).zip(&window) {
            *z = Complex::new((x - mean) * w, 0.0);
        }
        plan.forward(&mut data);
        for (p, z) in psd.iter_mut().zip(&data) {
            *p += z.norm_sqr();
        }
        count += 1;
        start += segment / 2;
    }
    let scale = 1.0 / (count as f64 * energy * sample_rate);
    let last = segment / 2;
    for (k, p) in psd.iter_mut().enumerate() {
        // One-sided: every bin but DC and Nyquist holds both halves.
        let fold = if k == 0 || k == last { 1.0 } else { 2.0 };
        *p *= scale * fold;
    }
    psd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welch_integrates_to_the_variance_of_white_noise() {
        // A deterministic white sequence: a 64-bit LCG's top bits.
        let mut state = 1u64;
        let x: Vec<f64> = (0..1 << 16)
            .map(|_| {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect();
        let fs = 48_000.0;
        let psd = welch(&x, 1024, fs);
        let total: f64 = psd.iter().sum::<f64>() * fs / 1024.0;
        // Uniform ±½ has variance 1/12, flat at (1/12)/(fs/2).
        assert!((total * 12.0 - 1.0).abs() < 0.02, "{}", total * 12.0);
        let mid: f64 = psd[100..400].iter().sum::<f64>() / 300.0;
        assert!((mid * 6.0 * fs - 1.0).abs() < 0.02, "{}", mid * 6.0 * fs);
    }

    #[test]
    fn goertzel_agrees_with_fft_on_bin_centred_tone() {
        let n = 4096usize;
        let m = 129usize;
        let buf: Vec<f64> = (0..n)
            .map(|i| 0.7 * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin())
            .collect();
        let g = goertzel(&buf, m as f64 / n as f64);
        assert!((g - 0.7).abs() < 1e-9, "goertzel magnitude {g} != 0.7");
        let mut re = buf.clone();
        let mut im = vec![0.0f64; n];
        fft(&mut re, &mut im);
        let mag = (re[m] * re[m] + im[m] * im[m]).sqrt() / (n as f64 * 0.5);
        assert!((mag - 0.7).abs() < 1e-9, "fft magnitude {mag} != 0.7");
    }
}
