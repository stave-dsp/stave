//! The [`Fft`] against its definition (DSP.md F1, R10): every length
//! from 2 to 2^16 against a naive DFT in `f64`, and the properties a
//! Fourier transform must have — the round trip, Parseval's theorem and
//! linearity — in `f32` and `f64`, complex and real.
//!
//! Error bounds are stated relative to the signal's norm `‖x‖₂` and the
//! transform's depth: a radix-2/4 FFT's rounding grows like
//! `ε·log₂ n`, so every check is `error ≤ C·ε·log₂(n)·‖x‖₂` with a
//! fixed `C`. The reference DFT sums with Neumaier compensation so it
//! is not the noisier side of the comparison at 2^16 points.

use stave_dsp::primitives::spectral::{Complex, FftFloat};
use stave_dsp::{Fft, Xorshift32};

/// Random complex points in the unit square.
fn signal<T: FftFloat>(n: usize, seed: u32) -> Vec<Complex<T>> {
    let mut rng = Xorshift32::new(seed);
    (0..n)
        .map(|_| {
            let re = f64::from(rng.next_bipolar());
            let im = f64::from(rng.next_bipolar());
            Complex::new(T::from_f64(re), T::from_f64(im))
        })
        .collect()
}

fn real_signal<T: FftFloat>(n: usize, seed: u32) -> Vec<T> {
    let mut rng = Xorshift32::new(seed);
    (0..n)
        .map(|_| T::from_f64(f64::from(rng.next_bipolar())))
        .collect()
}

fn norm<T: FftFloat>(x: &[Complex<T>]) -> f64 {
    x.iter()
        .map(|z| z.re.to_f64().powi(2) + z.im.to_f64().powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Neumaier-compensated accumulator.
#[derive(Default)]
struct Sum {
    s: f64,
    c: f64,
}
impl Sum {
    fn add(&mut self, x: f64) {
        let t = self.s + x;
        self.c += if self.s.abs() >= x.abs() {
            (self.s - t) + x
        } else {
            (x - t) + self.s
        };
        self.s = t;
    }
    fn get(&self) -> f64 {
        self.s + self.c
    }
}

/// A compensated sum (summation error far below the transform's).
fn compensated(terms: impl Iterator<Item = f64>) -> f64 {
    let mut sum = Sum::default();
    terms.for_each(|t| sum.add(t));
    sum.get()
}

/// Bin `k` of the DFT of `x` (sign −1 forward, +1 inverse), from an
/// exact-angle twiddle table `w[m] = e^(−2πi·m/n)`.
fn dft_bin(x: &[Complex<f64>], w: &[(f64, f64)], k: usize, inverse: bool) -> (f64, f64) {
    let n = x.len();
    let (mut re, mut im) = (Sum::default(), Sum::default());
    for (j, z) in x.iter().enumerate() {
        let (c, s) = w[(j * k) % n];
        let s = if inverse { -s } else { s };
        re.add(z.re * c);
        re.add(-z.im * s);
        im.add(z.re * s);
        im.add(z.im * c);
    }
    (re.get(), im.get())
}

fn twiddles(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|m| {
            let a = -core::f64::consts::TAU * m as f64 / n as f64;
            (a.cos(), a.sin())
        })
        .collect()
}

/// The bins checked at length `n`: all of them up to 1024 points, else
/// the edges and a seeded scatter (32 bins × n terms keeps 2^16 fast).
fn bins_to_check(n: usize, seed: u32) -> Vec<usize> {
    if n <= 1024 {
        return (0..n).collect();
    }
    let mut rng = Xorshift32::new(seed);
    let mut bins = vec![0, 1, n / 4, n / 2, n - 1];
    bins.extend((0..27).map(|_| rng.next_u32() as usize % n));
    bins
}

#[test]
#[cfg_attr(miri, ignore)]
fn forward_and_inverse_match_a_naive_dft_at_every_length() {
    let mut worst = 0.0f64;
    for bits in 1..=16u32 {
        let n = 1usize << bits;
        let fft = Fft::<f64>::new(n);
        let x = signal::<f64>(n, 0x5EED + bits);
        let w = twiddles(n);
        let scale = norm(&x) * f64::EPSILON * f64::from(bits);
        for inverse in [false, true] {
            let mut y = x.clone();
            if inverse {
                fft.inverse(&mut y);
            } else {
                fft.forward(&mut y);
            }
            for k in bins_to_check(n, bits) {
                let (re, im) = dft_bin(&x, &w, k, inverse);
                let err = (y[k].re - re).hypot(y[k].im - im) / scale;
                worst = worst.max(err);
                assert!(
                    err < 4.0,
                    "n = {n}, bin {k}, inverse {inverse}: error {err:.2} ε·log₂n·‖x‖"
                );
            }
        }
    }
    println!("worst f64 error vs the DFT: {worst:.3} ε·log₂n·‖x‖₂");
}

#[test]
#[cfg_attr(miri, ignore)]
fn the_real_transform_is_the_complex_transform_of_a_real_signal() {
    for bits in 1..=16u32 {
        let n = 1usize << bits;
        let fft = Fft::<f64>::new(n);
        let x = real_signal::<f64>(n, 0xA11 + bits);
        let mut full: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v, 0.0)).collect();
        fft.forward(&mut full);
        let mut bins = vec![Complex::default(); fft.real_bins()];
        fft.forward_real(&x, &mut bins);
        let scale = norm(&full) / (n as f64).sqrt() * f64::EPSILON * f64::from(bits);
        for (k, (a, b)) in bins.iter().zip(&full).enumerate() {
            let err = (a.re - b.re).hypot(a.im - b.im) / scale;
            assert!(err < 8.0, "n = {n}, bin {k}: {err:.2} ε·log₂n·‖x‖");
        }
        assert_eq!(bins[0].im, 0.0, "n = {n}: DC must be real");
        assert_eq!(bins[n / 2].im, 0.0, "n = {n}: Nyquist must be real");
    }
}

/// The `f32` transform against the `f64` one (itself checked against
/// the DFT above), at f32's epsilon.
#[test]
#[cfg_attr(miri, ignore)]
fn the_f32_transform_is_within_its_epsilon_of_the_f64_one() {
    for bits in 1..=16u32 {
        let n = 1usize << bits;
        let x32 = signal::<f32>(n, 0xF32 + bits);
        let x64: Vec<Complex<f64>> = x32
            .iter()
            .map(|z| Complex::new(f64::from(z.re), f64::from(z.im)))
            .collect();
        let (mut y32, mut y64) = (x32.clone(), x64.clone());
        Fft::<f32>::new(n).forward(&mut y32);
        Fft::<f64>::new(n).forward(&mut y64);
        let scale = norm(&x64) * f64::from(f32::EPSILON) * f64::from(bits);
        let worst = y32
            .iter()
            .zip(&y64)
            .map(|(a, b)| (f64::from(a.re) - b.re).hypot(f64::from(a.im) - b.im))
            .fold(0.0, f64::max)
            / scale;
        assert!(worst < 4.0, "n = {n}: f32 error {worst:.2} ε·log₂n·‖x‖");
    }
}

fn round_trip<T: FftFloat>(bound: f64) {
    let eps = if core::mem::size_of::<T>() == 4 {
        f64::from(f32::EPSILON)
    } else {
        f64::EPSILON
    };
    for bits in 1..=16u32 {
        let n = 1usize << bits;
        let fft = Fft::<T>::new(n);
        let x = signal::<T>(n, 0xB0B + bits);
        let mut y = x.clone();
        fft.forward(&mut y);
        fft.inverse(&mut y);
        let inv_n = 1.0 / n as f64;
        let rms = norm(&x) / (n as f64).sqrt();
        let err = x
            .iter()
            .zip(&y)
            .map(|(a, b)| {
                (a.re.to_f64() - b.re.to_f64() * inv_n).hypot(a.im.to_f64() - b.im.to_f64() * inv_n)
            })
            .fold(0.0, f64::max);
        assert!(
            err < bound * eps * f64::from(bits) * rms * 4.0,
            "complex n = {n}: round-trip error {err:e}"
        );

        let xr = real_signal::<T>(n, 0xCAB + bits);
        let mut bins = vec![Complex::default(); fft.real_bins()];
        fft.forward_real(&xr, &mut bins);
        let mut back = vec![T::default(); n];
        fft.inverse_real(&mut bins, &mut back);
        let err = xr
            .iter()
            .zip(&back)
            .map(|(a, b)| (a.to_f64() - b.to_f64() * inv_n).abs())
            .fold(0.0, f64::max);
        assert!(
            err < bound * eps * f64::from(bits) * 4.0,
            "real n = {n}: round-trip error {err:e}"
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn the_round_trip_returns_the_input_times_n() {
    round_trip::<f64>(2.0);
    round_trip::<f32>(2.0);
}

#[test]
#[cfg_attr(miri, ignore)]
fn parseval_energy_is_conserved() {
    for bits in 1..=16u32 {
        let n = 1usize << bits;
        let x = signal::<f64>(n, 0xE + bits);
        let mut y = x.clone();
        Fft::<f64>::new(n).forward(&mut y);
        let energy = |z: &[Complex<f64>]| compensated(z.iter().map(|z| z.norm_sqr()));
        let (ex, ey) = (energy(&x), energy(&y) / n as f64);
        let rel = (ex - ey).abs() / ex;
        assert!(
            rel < 4.0 * f64::EPSILON * f64::from(bits),
            "n = {n}: Parseval off by {rel:e}"
        );

        // The real transform: the half spectrum counts every bin but DC
        // and Nyquist twice.
        let xr = real_signal::<f64>(n, 0xF + bits);
        let mut bins = vec![Complex::default(); n / 2 + 1];
        Fft::<f64>::new(n).forward_real(&xr, &mut bins);
        let time = compensated(xr.iter().map(|v| v * v));
        let freq = compensated(
            bins.iter()
                .enumerate()
                .map(|(k, z)| z.norm_sqr() * if k == 0 || k == n / 2 { 1.0 } else { 2.0 }),
        ) / n as f64;
        let rel = (time - freq).abs() / time;
        assert!(
            rel < 4.0 * f64::EPSILON * f64::from(bits),
            "real n = {n}: Parseval off by {rel:e}"
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn the_transform_is_linear() {
    let mut rng = Xorshift32::new(0x11AE);
    for bits in 1..=14u32 {
        let n = 1usize << bits;
        let fft = Fft::<f64>::new(n);
        let (a, b) = (
            f64::from(rng.next_bipolar()) * 4.0,
            f64::from(rng.next_bipolar()) * 4.0,
        );
        let (x, y) = (signal::<f64>(n, bits), signal::<f64>(n, 100 + bits));
        let mut mix: Vec<Complex<f64>> = x
            .iter()
            .zip(&y)
            .map(|(x, y)| x.scale(a) + y.scale(b))
            .collect();
        let (mut fx, mut fy) = (x.clone(), y.clone());
        fft.forward(&mut mix);
        fft.forward(&mut fx);
        fft.forward(&mut fy);
        let scale = (a.abs() * norm(&x) + b.abs() * norm(&y)) * f64::EPSILON * f64::from(bits);
        for k in 0..n {
            let want = fx[k].scale(a) + fy[k].scale(b);
            let err = (mix[k].re - want.re).hypot(mix[k].im - want.im) / scale;
            assert!(err < 8.0, "n = {n}, bin {k}: {err:.2}");
        }
    }
}

#[test]
fn the_real_inverse_ignores_the_imaginary_parts_of_dc_and_nyquist() {
    let fft = Fft::<f64>::new(16);
    let x = real_signal::<f64>(16, 3);
    let mut bins = vec![Complex::default(); 9];
    fft.forward_real(&x, &mut bins);
    let mut clean = bins.clone();
    bins[0].im = 5.0;
    bins[8].im = -2.0;
    let (mut a, mut b) = (vec![0.0; 16], vec![0.0; 16]);
    fft.inverse_real(&mut bins, &mut a);
    fft.inverse_real(&mut clean, &mut b);
    assert_eq!(a, b);
}
