//! The Fourier tools the other metrics stand on: an in-place radix-2
//! FFT and a single-bin Goertzel (f64, bin-centred tones — see the crate
//! docs for why measurement here never windows a stationary capture).

/// In-place iterative radix-2 FFT (f64). `re.len()` must be a power of 2.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two(), "fft length must be a power of two");
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * core::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let tr = cr * re[b] - ci * im[b];
                let ti = cr * im[b] + ci * re[b];
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
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

#[cfg(test)]
mod tests {
    use super::*;

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
