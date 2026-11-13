//! Minimum-phase conversion by the real cepstrum, shared by
//! [`Fir::minimum_phase`](super::Fir::minimum_phase) and
//! [`ImpulseResponse::minimum_phase`](super::ImpulseResponse::minimum_phase).
//!
//! The homomorphic method (Oppenheim & Schafer, *Discrete-Time Signal
//! Processing*, 3rd ed., §13.8.1): take `ln|H|` on a fine frequency grid,
//! transform to the real cepstrum, fold its anticausal half onto the
//! causal one (double `c[1..N/2]`, zero `c[N/2+1..]`), and return through
//! the exponential. The result has the same magnitude and every zero
//! inside the unit circle, so its energy arrives as early as any response
//! with that magnitude can.
//!
//! The magnitude fixes a response only up to its sign; the conversion
//! keeps the polarity of the low frequencies (the sign of `Σh`, or of the
//! largest tap when the response has no DC), so a converted cabinet or
//! room does not flip.

use super::fft::{Complex, Fft};

/// The grid is this many times the response's length (then a power of
/// two), and at least [`MIN_GRID`] points, so the folded cepstrum's time
/// aliasing stays far below the magnitude budget: a stopband null on the
/// unit circle makes the cepstrum decay slowly, and a coarse grid folds
/// it back as lost energy (measured on an 80 dB Kaiser lowpass: 3e-5 of
/// the energy at 16× its length, 7e-7 at 256×).
const OVERSAMPLE: usize = 16;

/// The smallest grid, in points (a short filter's conversion is then
/// still cheap: a few milliseconds).
const MIN_GRID: usize = 1 << 16;

/// `|H|` is floored this far below its peak (−240 dB) before the
/// logarithm, so a zero on the unit circle (a stopband null) is a deep
/// notch rather than `ln 0`.
const FLOOR: f64 = 1e-12;

/// The minimum-phase response with the magnitude of `h`, the same length.
pub(crate) fn minimum_phase(h: &[f64]) -> Vec<f64> {
    let n = h.len();
    if n == 0 {
        return Vec::new();
    }
    let size = (OVERSAMPLE * n).max(MIN_GRID).next_power_of_two();
    let fft = Fft::<f64>::new(size);
    let mut x: Vec<Complex<f64>> = (0..size)
        .map(|i| Complex::new(h.get(i).copied().unwrap_or(0.0), 0.0))
        .collect();
    fft.forward(&mut x);
    let peak = x.iter().map(|z| z.norm()).fold(0.0f64, f64::max);
    if peak == 0.0 {
        return vec![0.0; n];
    }
    let floor = peak * FLOOR;
    for z in &mut x {
        *z = Complex::new(z.norm().max(floor).ln(), 0.0);
    }
    fft.inverse(&mut x); // the real cepstrum, times `size`
    let half = size / 2;
    for (i, z) in x.iter_mut().enumerate() {
        let fold = match i {
            0 => 1.0,
            i if i < half => 2.0,
            i if i == half => 1.0,
            _ => 0.0,
        };
        *z = Complex::new(z.re * fold / size as f64, 0.0);
    }
    fft.forward(&mut x);
    for z in &mut x {
        let m = z.re.exp();
        *z = Complex::new(m * z.im.cos(), m * z.im.sin());
    }
    fft.inverse(&mut x);
    let sign = polarity(h);
    x[..n].iter().map(|z| sign * z.re / size as f64).collect()
}

/// `±1`: the sign of `h`'s DC gain, or of its largest tap when the DC
/// gain is negligible (a highpass or bandpass).
fn polarity(h: &[f64]) -> f64 {
    let dc: f64 = h.iter().sum();
    let total: f64 = h.iter().map(|v| v.abs()).sum();
    let lead = if dc.abs() > 1e-6 * total {
        dc
    } else {
        h.iter().copied().fold(0.0, |a: f64, v| if v.abs() > a.abs() { v } else { a })
    };
    if lead < 0.0 {
        -1.0
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_outside_the_circle_moves_inside() {
        // 1 − 2z⁻¹ has its zero at z = 2; the minimum-phase responses with
        // the same magnitude, |1 − 2e^{−iω}|² = 5 − 4cos ω, are ±(2 − z⁻¹),
        // and the one with the same DC polarity (−1) is −2 + z⁻¹.
        let m = minimum_phase(&[1.0, -2.0]);
        assert!((m[0] + 2.0).abs() < 1e-9 && (m[1] - 1.0).abs() < 1e-9, "{m:?}");
        let m = minimum_phase(&[-1.0, 2.0]);
        assert!((m[0] - 2.0).abs() < 1e-9 && (m[1] + 1.0).abs() < 1e-9, "{m:?}");
    }

    #[test]
    fn a_minimum_phase_response_is_its_own_conversion() {
        let h = [1.0, 0.5, 0.25, 0.125];
        let m = minimum_phase(&h);
        for (a, b) in h.iter().zip(&m) {
            assert!((a - b).abs() < 1e-9, "{m:?}");
        }
        assert_eq!(minimum_phase(&[]), Vec::<f64>::new());
        assert_eq!(minimum_phase(&[0.0, 0.0]), vec![0.0, 0.0]);
    }
}
