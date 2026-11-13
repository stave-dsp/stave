//! **Fft** — the workspace's one fast Fourier transform: complex and
//! real, power-of-two lengths, `f32` and `f64`.
//!
//! Decimation in time over a bit-reversed copy: one radix-2 pass when
//! the length's exponent is odd, then radix-4 passes (the radix-2/4
//! split). Twiddles are computed in `f64` once, at planning, and stored
//! contiguously per pass, so a transform reads them in order and never
//! calls `sin`/`cos`. The real transform of length `n` is a complex
//! transform of length `n/2` plus one pass of post-processing, so it
//! costs about half the complex one.
//!
//! Conventions (the ones FFTW and rustfft use):
//!
//! * forward is `X[k] = Σ x[j]·e^(−2πi·jk/n)`;
//! * inverse is the same sum with `+i` and **no** `1/n`: a round trip
//!   returns the input times `n`. Fold the `1/n` into whatever you
//!   multiply the spectrum by (a convolution folds it into the filter);
//! * the real forward transform writes the `n/2 + 1` non-redundant bins,
//!   DC first and Nyquist last (both with a zero imaginary part).
//!
//! The decision to own this (DSP.md F1, the "own it unless a dependency
//! is 10–50× better" rule): measured against rustfft 6.4 / realfft 3.5
//! (with their NEON kernels) on an Apple M-series core, rustfft is
//! 1.6–2.0× faster in `f64` and 2.6–3.3× in `f32` from 64 to 65 536
//! points — far from the 10–50× that would pay for a dependency.
//!
//! SIMD (DSP.md P4): every radix-4 pass of a quarter length of at least
//! four runs four butterflies at once on the crate's lanes, real and
//! imaginary parts in separate lanes and the twiddles stored split to
//! match. Each lane is one butterfly computed exactly as the scalar one
//! is, so the transform's output is bit for bit what it was.

use core::fmt::Debug;
use core::ops::{Add, Mul, Neg, Sub};

use crate::kernel::simd::Lanes;

/// Butterflies a radix-4 pass computes at once: one 128-bit register a
/// part in `f32`, two in `f64`.
const WIDTH: usize = 4;
type V<T> = Lanes<T, WIDTH>;

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// The scalar an [`Fft`] runs in: `f32` or `f64` (sealed).
pub trait FftFloat:
    Copy
    + Default
    + Debug
    + PartialEq
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Neg<Output = Self>
    + sealed::Sealed
{
    /// The nearest value of this type to `x`.
    fn from_f64(x: f64) -> Self;
    /// This value widened to `f64` (exact).
    fn to_f64(self) -> f64;
}

impl FftFloat for f32 {
    #[inline]
    fn from_f64(x: f64) -> Self {
        x as f32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl FftFloat for f64 {
    #[inline]
    fn from_f64(x: f64) -> Self {
        x
    }
    #[inline]
    fn to_f64(self) -> f64 {
        self
    }
}

/// A complex number: one bin of a spectrum, or one point of a complex
/// signal.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Complex<T> {
    /// Real part.
    pub re: T,
    /// Imaginary part.
    pub im: T,
}

impl<T: FftFloat> Complex<T> {
    /// `re + i·im`.
    #[inline]
    pub const fn new(re: T, im: T) -> Self {
        Self { re, im }
    }

    /// The complex conjugate, `re − i·im`.
    #[inline]
    pub fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// The squared magnitude, `re² + im²` (power, without the square
    /// root).
    #[inline]
    pub fn norm_sqr(self) -> T {
        self.re * self.re + self.im * self.im
    }

    /// The magnitude, `√(re² + im²)` (computed in `f64`, without
    /// overflow).
    #[inline]
    pub fn norm(self) -> T {
        T::from_f64(self.re.to_f64().hypot(self.im.to_f64()))
    }

    /// The phase in radians, in `(−π, π]`.
    #[inline]
    pub fn arg(self) -> T {
        T::from_f64(self.im.to_f64().atan2(self.re.to_f64()))
    }

    /// Both parts times a real `k`.
    #[inline]
    pub fn scale(self, k: T) -> Self {
        Self::new(self.re * k, self.im * k)
    }

    /// `self · −i` (a quarter turn clockwise), without a multiply.
    #[inline]
    fn times_neg_i(self) -> Self {
        Self::new(self.im, -self.re)
    }

    /// `self · i` (a quarter turn anticlockwise), without a multiply.
    #[inline]
    fn times_i(self) -> Self {
        Self::new(-self.im, self.re)
    }
}

impl<T: FftFloat> Add for Complex<T> {
    type Output = Self;
    #[inline]
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
}

impl<T: FftFloat> Sub for Complex<T> {
    type Output = Self;
    #[inline]
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
}

impl<T: FftFloat> Mul for Complex<T> {
    type Output = Self;
    #[inline]
    fn mul(self, o: Self) -> Self {
        Self::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

/// `i` reversed in its low `bits` bits (the decimation-in-time order).
#[inline]
pub(crate) fn reverse_bits(i: usize, bits: u32) -> usize {
    if bits == 0 {
        0
    } else {
        i.reverse_bits() >> (usize::BITS - bits)
    }
}

/// A planned FFT of one power-of-two length (at least 2), in `f32` or
/// `f64`: the complex transform of that length and the real transform
/// of that length, from one plan.
///
/// Plan it where allocation is allowed — a constructor or
/// [`Stage::prepare`](crate::Stage::prepare) — then call it from the
/// audio thread: the transforms run in place and never allocate.
///
/// # Models
///
/// The discrete Fourier transform `X[k] = Σ x[j]·e^(−2πi·jk/n)` and its
/// unnormalised inverse.
///
/// # Derivation
///
/// Textbook decimation in time (Cooley–Tukey), radix-2/4 split; the
/// real transform packs even and odd samples into one complex transform
/// of half the length and separates them in one pass.
///
/// # Verified
///
/// Against a naive DFT (compensated sums) at every length from 2 to
/// 2^16 in `f64`, worst error 0.84 `ε·log₂n·‖x‖₂` (budget 4); round trip,
/// Parseval and linearity in `f32` and `f64`, complex and real:
/// `crates/stave-dsp/tests/fft_properties.rs`. The bit-reversal
/// permutation is a Kani proof (`bit_reversal_is_an_in_range_involution`).
///
/// # Cost
///
/// `tests/datasheet.rs`, "Fft": the `f32` real transform about 2.1 ns
/// per point at 512 points and 2.6 at 4096 (complex: 3.5 and 4.6), on an
/// Apple M-series core.
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// Lengths `2..=2^31`, powers of two. Accuracy degrades like
/// `ε·log₂ n`.
///
/// ```
/// use stave_dsp::primitives::spectral::Complex;
/// use stave_dsp::Fft;
///
/// let fft = Fft::<f64>::new(8);
/// // A cosine at bin 1: its energy lands in bins 1 and 7 (n/2 each).
/// let mut x: Vec<Complex<f64>> = (0..8)
///     .map(|j| Complex::new((core::f64::consts::TAU * j as f64 / 8.0).cos(), 0.0))
///     .collect();
/// fft.forward(&mut x);
/// assert!((x[1].re - 4.0).abs() < 1e-12 && (x[7].re - 4.0).abs() < 1e-12);
/// // The inverse is unnormalised: the round trip is n times the input.
/// fft.inverse(&mut x);
/// assert!((x[0].re - 8.0).abs() < 1e-12);
///
/// // The real transform: n samples in, n/2 + 1 bins out.
/// let signal = [1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0];
/// let mut bins = vec![Complex::default(); fft.real_bins()];
/// fft.forward_real(&signal, &mut bins);
/// assert!((bins[2].re - 4.0).abs() < 1e-12);
/// let mut back = [0.0; 8];
/// fft.inverse_real(&mut bins, &mut back);
/// assert!((back[0] / 8.0 - 1.0).abs() < 1e-12);
/// ```
#[derive(Clone, Debug)]
pub struct Fft<T> {
    len: usize,
    bits: u32,
    /// `reverse_bits(i, bits)` for every `i < len`; a shorter transform
    /// of `2^b` points reads `rev[i] >> (bits − b)`.
    rev: Vec<u32>,
    /// Radix-4 twiddles, split into parts: for each quarter length `L`
    /// (1, 2, 4, …, `len/4`), starting at `6·(L − 1)`, six rows of `L`:
    /// the real parts of `w^k`, then the imaginary parts, then `w^2k`'s
    /// and `w^3k`'s, `k < L`, `w = e^(−2πi/4L)`. A pass's table depends
    /// only on `L`, so the half-length transform the real FFT runs uses
    /// the same one.
    stages: Vec<T>,
    /// The real transform's post-processing twiddles `e^(−2πi·k/len)`
    /// for `k ≤ len/4`.
    real: Vec<Complex<T>>,
}

impl<T: FftFloat> Fft<T> {
    /// Plan the transforms of `len` points. Allocates the tables.
    ///
    /// # Panics
    ///
    /// If `len` is not a power of two, is less than 2, or exceeds 2³¹.
    pub fn new(len: usize) -> Self {
        assert!(
            len.is_power_of_two() && (2..=1 << 31).contains(&len),
            "Fft length must be a power of two in 2..=2^31, got {len}"
        );
        let bits = len.trailing_zeros();
        let rev = (0..len).map(|i| reverse_bits(i, bits) as u32).collect();
        let twiddle = |k: usize, period: usize| {
            let a = -core::f64::consts::TAU * k as f64 / period as f64;
            Complex::new(T::from_f64(a.cos()), T::from_f64(a.sin()))
        };
        let mut stages = Vec::with_capacity(6 * (len / 4).max(1));
        let mut quarter = 1;
        while quarter * 4 <= len {
            for m in 1..=3 {
                let row: Vec<Complex<T>> = (0..quarter).map(|k| twiddle(m * k, 4 * quarter)).collect();
                stages.extend(row.iter().map(|w| w.re));
                stages.extend(row.iter().map(|w| w.im));
            }
            quarter *= 2;
        }
        let real = (0..=len / 4).map(|k| twiddle(k, len)).collect();
        Self {
            len,
            bits,
            rev,
            stages,
            real,
        }
    }

    /// The planned length, in points (complex) or samples (real).
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Always `false`: a plan has at least two points.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The bins the real transform reads and writes: `len/2 + 1`.
    pub const fn real_bins(&self) -> usize {
        self.len / 2 + 1
    }

    /// The forward complex transform of `data`, in place.
    ///
    /// # Panics
    ///
    /// If `data.len()` is not the planned length.
    pub fn forward(&self, data: &mut [Complex<T>]) {
        assert_eq!(data.len(), self.len, "Fft::forward: wrong length");
        self.transform::<false>(data, self.bits);
    }

    /// The inverse complex transform of `data`, in place, without the
    /// `1/n`: `inverse(forward(x)) == n·x`.
    ///
    /// # Panics
    ///
    /// If `data.len()` is not the planned length.
    pub fn inverse(&self, data: &mut [Complex<T>]) {
        assert_eq!(data.len(), self.len, "Fft::inverse: wrong length");
        self.transform::<true>(data, self.bits);
    }

    /// The forward transform of the real signal `input` (`len` samples)
    /// into `spectrum` (`len/2 + 1` bins, DC to Nyquist).
    ///
    /// # Panics
    ///
    /// If either slice has the wrong length.
    pub fn forward_real(&self, input: &[T], spectrum: &mut [Complex<T>]) {
        let n = self.len;
        assert_eq!(input.len(), n, "Fft::forward_real: wrong input length");
        assert_eq!(spectrum.len(), n / 2 + 1, "Fft::forward_real: wrong bins");
        let m = n / 2;
        for (z, pair) in spectrum[..m].iter_mut().zip(input.chunks_exact(2)) {
            *z = Complex::new(pair[0], pair[1]);
        }
        self.transform::<false>(&mut spectrum[..m], self.bits - 1);
        // Split the half-length transform Z of z[j] = x[2j] + i·x[2j+1]
        // into the even and odd samples' transforms, E and O, and
        // recombine: X[k] = E + w^k·O, X[m−k] = conj(E − w^k·O).
        let half = T::from_f64(0.5);
        for k in 0..=m / 2 {
            let zk = spectrum[k];
            let zp = spectrum[if k == 0 { 0 } else { m - k }].conj();
            let e = (zk + zp).scale(half);
            let o = (zk - zp).scale(half).times_neg_i();
            let wo = self.real[k] * o;
            spectrum[k] = e + wo;
            if m - k != k {
                spectrum[m - k] = (e - wo).conj();
            }
        }
    }

    /// The inverse of [`forward_real`](Self::forward_real), without the
    /// `1/n`: `spectrum` (`len/2 + 1` bins) to `output` (`len` samples),
    /// so `inverse_real(forward_real(x)) == n·x`. The imaginary parts of
    /// the DC and Nyquist bins are ignored. `spectrum` is used as
    /// scratch: its contents afterwards are unspecified.
    ///
    /// # Panics
    ///
    /// If either slice has the wrong length.
    pub fn inverse_real(&self, spectrum: &mut [Complex<T>], output: &mut [T]) {
        let n = self.len;
        assert_eq!(output.len(), n, "Fft::inverse_real: wrong output length");
        assert_eq!(spectrum.len(), n / 2 + 1, "Fft::inverse_real: wrong bins");
        let m = n / 2;
        // Undo the split: Z[k] = E + i·O and Z[m−k] = conj(E − i·O),
        // with E and O doubled so the half-length inverse lands at n·x.
        let (dc, nyquist) = (spectrum[0].re, spectrum[m].re);
        spectrum[0] = Complex::new(dc, T::default());
        spectrum[m] = Complex::new(nyquist, T::default());
        for k in 0..=m / 2 {
            let xk = spectrum[k];
            let xp = spectrum[m - k].conj();
            let e = xk + xp;
            let o = self.real[k].conj() * (xk - xp);
            spectrum[k] = e + o.times_i();
            if k != 0 && m - k != k {
                spectrum[m - k] = (e - o.times_i()).conj();
            }
        }
        self.transform::<true>(&mut spectrum[..m], self.bits - 1);
        for (pair, z) in output.chunks_exact_mut(2).zip(&spectrum[..m]) {
            pair[0] = z.re;
            pair[1] = z.im;
        }
    }

    /// The transform of `2^bits` points (`bits ≤ self.bits`), in place.
    fn transform<const INVERSE: bool>(&self, data: &mut [Complex<T>], bits: u32) {
        let n = data.len();
        debug_assert_eq!(n, 1 << bits);
        let shift = self.bits - bits;
        for i in 0..n {
            let j = (self.rev[i] >> shift) as usize;
            if i < j {
                data.swap(i, j);
            }
        }
        let mut quarter = 1;
        if bits % 2 == 1 {
            for pair in data.chunks_exact_mut(2) {
                let (a, b) = (pair[0], pair[1]);
                pair[0] = a + b;
                pair[1] = a - b;
            }
            quarter = 2;
        }
        while quarter * 4 <= n {
            self.radix4::<INVERSE>(data, quarter);
            quarter *= 4;
        }
    }

    /// One radix-4 pass: every run of `4·quarter` points, four
    /// transforms of `quarter` points in bit-reversed order, becomes one
    /// transform of `4·quarter`.
    #[inline]
    fn radix4<const INVERSE: bool>(&self, data: &mut [Complex<T>], quarter: usize) {
        let table = &self.stages[6 * (quarter - 1)..6 * (quarter - 1) + 6 * quarter];
        if quarter >= WIDTH {
            return Self::radix4_lanes::<INVERSE>(data, quarter, table);
        }
        let w = |m: usize, k: usize| {
            let row = 2 * (m - 1) * quarter;
            Complex::new(table[row + k], table[row + quarter + k])
        };
        for run in data.chunks_exact_mut(4 * quarter) {
            let (a, rest) = run.split_at_mut(quarter);
            let (b, rest) = rest.split_at_mut(quarter);
            let (c, d) = rest.split_at_mut(quarter);
            let lanes = a.iter_mut().zip(b.iter_mut()).zip(c.iter_mut()).zip(d);
            for (k, (((a, b), c), d)) in lanes.enumerate() {
                let (w1, w2, w3) = if INVERSE {
                    (w(1, k).conj(), w(2, k).conj(), w(3, k).conj())
                } else {
                    (w(1, k), w(2, k), w(3, k))
                };
                // b holds the sub-transform of the residue-2 samples,
                // c of residue 1 (bit-reversed order).
                let t0 = *a;
                let t2 = w2 * *b;
                let t1 = w1 * *c;
                let t3 = w3 * *d;
                let (u0, u1) = (t0 + t2, t0 - t2);
                let (v0, v1) = (t1 + t3, t1 - t3);
                let rot = if INVERSE {
                    v1.times_i()
                } else {
                    v1.times_neg_i()
                };
                *a = u0 + v0;
                *b = u1 + rot;
                *c = u0 - v0;
                *d = u1 - rot;
            }
        }
    }

    /// [`radix4`](Self::radix4) [`WIDTH`] butterflies at a time
    /// (`quarter` a multiple of it): the same expressions, lane by lane,
    /// on the real and imaginary parts apart — a conjugate twiddle is its
    /// imaginary part negated, as `conj` does it.
    #[inline]
    fn radix4_lanes<const INVERSE: bool>(data: &mut [Complex<T>], quarter: usize, table: &[T]) {
        // `(w·z)` exactly as `Complex`'s `Mul` computes it.
        let mul = |wr: V<T>, wi: V<T>, (zr, zi): (V<T>, V<T>)| (wr * zr - wi * zi, wr * zi + wi * zr);
        let twiddle = |m: usize, k: usize| {
            let row = 2 * (m - 1) * quarter;
            let (re, im) = (V::load(&table[row + k..]), V::load(&table[row + quarter + k..]));
            (re, if INVERSE { -im } else { im })
        };
        for run in data.chunks_exact_mut(4 * quarter) {
            for k in (0..quarter).step_by(WIDTH) {
                let (w1r, w1i) = twiddle(1, k);
                let (w2r, w2i) = twiddle(2, k);
                let (w3r, w3i) = twiddle(3, k);
                let t0 = split(&run[k..]);
                let t2 = mul(w2r, w2i, split(&run[quarter + k..]));
                let t1 = mul(w1r, w1i, split(&run[2 * quarter + k..]));
                let t3 = mul(w3r, w3i, split(&run[3 * quarter + k..]));
                let (u0, u1) = ((t0.0 + t2.0, t0.1 + t2.1), (t0.0 - t2.0, t0.1 - t2.1));
                let (v0, v1) = ((t1.0 + t3.0, t1.1 + t3.1), (t1.0 - t3.0, t1.1 - t3.1));
                // v1·i = (−im, re); v1·(−i) = (im, −re).
                let rot = if INVERSE { (-v1.1, v1.0) } else { (v1.1, -v1.0) };
                join(&mut run[k..], (u0.0 + v0.0, u0.1 + v0.1));
                join(&mut run[quarter + k..], (u1.0 + rot.0, u1.1 + rot.1));
                join(&mut run[2 * quarter + k..], (u0.0 - v0.0, u0.1 - v0.1));
                join(&mut run[3 * quarter + k..], (u1.0 - rot.0, u1.1 - rot.1));
            }
        }
    }
}

/// The first [`WIDTH`] points of `z` as (real lanes, imaginary lanes).
#[inline(always)]
fn split<T: FftFloat>(z: &[Complex<T>]) -> (V<T>, V<T>) {
    let z = &z[..WIDTH];
    let (mut re, mut im) = (V::default(), V::default());
    for l in 0..WIDTH {
        re.0[l] = z[l].re;
        im.0[l] = z[l].im;
    }
    (re, im)
}

/// (real lanes, imaginary lanes) into the first [`WIDTH`] points of `z`.
#[inline(always)]
fn join<T: FftFloat>(z: &mut [Complex<T>], (re, im): (V<T>, V<T>)) {
    let z = &mut z[..WIDTH];
    for l in 0..WIDTH {
        z[l] = Complex::new(re.0[l], im.0[l]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_reversal_is_an_involution() {
        for bits in 0..12 {
            for i in 0..1usize << bits {
                assert_eq!(reverse_bits(reverse_bits(i, bits), bits), i);
            }
        }
        assert_eq!(reverse_bits(1, 3), 4);
        assert_eq!(reverse_bits(6, 3), 3);
    }

    #[test]
    fn length_two_is_sum_and_difference() {
        let fft = Fft::<f32>::new(2);
        let mut x = [Complex::new(3.0, 1.0), Complex::new(1.0, -1.0)];
        fft.forward(&mut x);
        assert_eq!(x, [Complex::new(4.0, 0.0), Complex::new(2.0, 2.0)]);
        let mut bins = [Complex::default(); 2];
        fft.forward_real(&[3.0, 1.0], &mut bins);
        assert_eq!(bins, [Complex::new(4.0, 0.0), Complex::new(2.0, 0.0)]);
    }

    #[test]
    #[should_panic(expected = "power of two")]
    fn a_length_that_is_not_a_power_of_two_is_refused() {
        let _ = Fft::<f64>::new(12);
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// The permutation the transform starts with is its own inverse and
    /// stays inside the buffer, for every index of every length up to
    /// 2^16; and a half-length transform's order is the full table's,
    /// shifted (the identity the real FFT relies on).
    #[kani::proof]
    fn bit_reversal_is_an_in_range_involution() {
        let bits: u32 = kani::any_where(|b: &u32| (1..=16).contains(b));
        let i: usize = kani::any_where(|i: &usize| *i < 1 << bits);
        let j = reverse_bits(i, bits);
        assert!(j < 1 << bits);
        assert_eq!(reverse_bits(j, bits), i);
        if i < 1 << (bits - 1) {
            assert_eq!(j >> 1, reverse_bits(i, bits - 1));
        }
    }
}
