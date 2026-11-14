//! **Window** — the tapers the spectral family shares: FIR design reads
//! them symmetric ([`Window::symmetric`]), the STFT periodic
//! ([`Window::periodic`]).

use crate::units::Db;

/// A window (taper): the weights that fade a finite stretch of signal or
/// of an ideal response in and out.
///
/// The cosine windows are the textbook sums (Harris, *On the use of
/// windows for harmonic analysis with the discrete Fourier transform*,
/// Proc. IEEE 1978); Kaiser's is the zeroth-order Bessel window (Kaiser,
/// *Nonrecursive digital filter design using the I₀-sinh window
/// function*, 1974). [`SqrtHann`](Self::SqrtHann) is the square root of
/// Hann, the analysis-and-synthesis pair of a 50 % overlap STFT.
///
/// ```
/// use stave_dsp::primitives::spectral::Window;
///
/// let close = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-15);
/// // Periodic, for an STFT: shifted by half its length, it sums to one.
/// assert!(close(&Window::Hann.periodic(4), &[0.0, 0.5, 1.0, 0.5]));
/// // Symmetric, for FIR design: both ends included.
/// assert!(close(&Window::Hann.symmetric(5), &[0.0, 0.5, 1.0, 0.5, 0.0]));
/// // Kaiser's β for 80 dB of stopband, by Kaiser's formula.
/// assert_eq!(Window::kaiser_for(db(80.0)), Window::Kaiser { beta: 0.1102 * (80.0 - 8.7) });
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Window {
    /// All ones: no taper.
    Rectangular,
    /// `0.5 − 0.5·cos(2πx)`.
    Hann,
    /// `0.54 − 0.46·cos(2πx)`.
    Hamming,
    /// `0.42 − 0.5·cos(2πx) + 0.08·cos(4πx)`.
    Blackman,
    /// `√(0.5 − 0.5·cos(2πx))`: Hann when applied twice.
    SqrtHann,
    /// `I₀(β·√(1 − r²)) / I₀(β)` over `r ∈ [−1, 1]`: β trades main-lobe
    /// width for side-lobe level (see [`kaiser_for`](Self::kaiser_for)).
    Kaiser {
        /// The shape parameter: 0 is rectangular, larger is smoother.
        beta: f64,
    },
}

impl Window {
    /// The Kaiser window for a filter with `attenuation_db` of stopband
    /// (Kaiser's empirical formula: `0.1102·(A − 8.7)` above 50 dB,
    /// `0.5842·(A − 21)^0.4 + 0.07886·(A − 21)` from 21 to 50, 0 below).
    pub fn kaiser_for(attenuation: Db) -> Self {
        let attenuation_db = f64::from(attenuation.get());
        let a = attenuation_db;
        let beta = if a > 50.0 {
            0.1102 * (a - 8.7)
        } else if a >= 21.0 {
            0.5842 * (a - 21.0).powf(0.4) + 0.07886 * (a - 21.0)
        } else {
            0.0
        };
        Window::Kaiser { beta }
    }

    /// The weight at offset `k` from the centre of a window of half-width
    /// `m` (`k ∈ [−m, m]`; the centre weighs 1). Both layouts read it:
    /// symmetric with `m = (len − 1)/2`, periodic with `m = len/2`.
    pub(crate) fn at(self, k: f64, m: f64) -> f64 {
        if m == 0.0 {
            return 1.0;
        }
        let x = (k + m) / (2.0 * m);
        let c = |h: f64| (core::f64::consts::TAU * h * x).cos();
        match self {
            Window::Rectangular => 1.0,
            Window::Hann => 0.5 - 0.5 * c(1.0),
            Window::Hamming => 0.54 - 0.46 * c(1.0),
            Window::Blackman => 0.42 - 0.5 * c(1.0) + 0.08 * c(2.0),
            Window::SqrtHann => (0.5 - 0.5 * c(1.0)).max(0.0).sqrt(),
            Window::Kaiser { beta } => {
                let r = k / m;
                bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) * (1.0 / bessel_i0(beta))
            }
        }
    }

    /// `len` weights symmetric about the centre, both ends included:
    /// the window FIR design multiplies an ideal response by.
    pub fn symmetric(self, len: usize) -> Vec<f64> {
        let m = len.saturating_sub(1) as f64 / 2.0;
        (0..len).map(|n| self.at(n as f64 - m, m)).collect()
    }

    /// `len` weights of the periodic (DFT-even) window: the symmetric
    /// window of `len + 1` without its last point, so shifted copies
    /// overlap-add exactly. The window an STFT analyses with.
    pub fn periodic(self, len: usize) -> Vec<f64> {
        let m = len as f64 / 2.0;
        (0..len).map(|n| self.at(n as f64 - m, m)).collect()
    }
}

/// The zeroth-order modified Bessel function of the first kind, by its
/// power series (summed until a term is below `1e-14` of the sum; fast
/// for the β of audio filters). The same series as the oversampler's
/// half-band design, term for term.
pub(crate) fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let half = 0.5 * x;
    let mut k = 1.0f64;
    loop {
        term *= (half / k) * (half / k);
        sum += term;
        if term < sum * 1e-14 {
            return sum;
        }
        k += 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_peak_at_the_centre_and_are_symmetric() {
        for w in [
            Window::Rectangular,
            Window::Hann,
            Window::Hamming,
            Window::Blackman,
            Window::SqrtHann,
            Window::Kaiser { beta: 8.6 },
        ] {
            let s = w.symmetric(33);
            assert!((s[16] - 1.0).abs() < 1e-12, "{w:?}");
            for n in 0..33 {
                assert!((s[n] - s[32 - n]).abs() < 1e-12, "{w:?}");
                assert!(s[n] <= 1.0 + 1e-12 && s[n] >= -1e-15);
            }
            let p = w.periodic(32);
            assert!((p[16] - 1.0).abs() < 1e-12);
            for n in 1..32 {
                assert!((p[n] - p[32 - n]).abs() < 1e-12, "{w:?} periodic");
            }
        }
        assert_eq!(Window::Hann.symmetric(1), vec![1.0]);
    }

    #[test]
    fn bessel_i0_matches_its_table_values() {
        // I0(1) and I0(5) to 16 digits (mpmath).
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-15);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008).abs() < 1e-13);
        assert!((bessel_i0(5.0) / 27.239_871_823_604_44 - 1.0).abs() < 1e-13);
    }
}
