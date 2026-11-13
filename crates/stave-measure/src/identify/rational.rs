//! Rational transfer functions fitted to frequency responses: the
//! Sanathanan–Koerner iteration (each step a linear least-squares fit
//! of `B − H·A` weighted by the last `1/|A|`), the result as poles and
//! zeros, and as cascaded sections.

use stave_dsp::learned::block::Section;

use super::solve::{least_squares, roots};
use crate::suite::{divide, Complex};

/// `gain · z⁻ᵈ · Π(1 − qᵢ·z⁻¹) / Π(1 − pᵢ·z⁻¹)` by its roots.
#[derive(Clone, Debug, PartialEq)]
pub struct Rational {
    pub gain: f64,
    /// Whole samples of delay (`d`).
    pub delay: usize,
    pub zeros: Vec<Complex<f64>>,
    pub poles: Vec<Complex<f64>>,
}

/// A real root, or a complex one with its conjugate.
pub type Factor = Vec<Complex<f64>>;

/// One point of a response to fit: `w` radians per sample, the value,
/// and its weight.
#[derive(Copy, Clone, Debug)]
pub struct Point {
    pub w: f64,
    pub h: Complex<f64>,
    pub weight: f64,
}

fn unit(w: f64, k: usize) -> Complex<f64> {
    let p = -w * k as f64;
    Complex::new(p.cos(), p.sin())
}

/// `Σ c[k]·e^{−jwk}`.
fn poly(c: &[f64], w: f64) -> Complex<f64> {
    c.iter().enumerate().fold(Complex::new(0.0, 0.0), |acc, (k, &v)| acc + unit(w, k).scale(v))
}

/// Coefficients (in z⁻¹, constant first) of `Π(1 − rᵢ·z⁻¹)`.
fn expand(roots: &[Complex<f64>]) -> Vec<f64> {
    let mut c = vec![Complex::new(1.0, 0.0)];
    for &r in roots {
        let mut next = vec![Complex::new(0.0, 0.0); c.len() + 1];
        for (i, &v) in c.iter().enumerate() {
            next[i] = next[i] + v;
            next[i + 1] = next[i + 1] - v * r;
        }
        c = next;
    }
    c.iter().map(|v| v.re).collect()
}

/// Group roots into factors: real ones alone, complex ones with their
/// conjugates.
pub fn factors(roots: &[Complex<f64>]) -> Vec<Factor> {
    let mut out: Vec<Factor> = Vec::new();
    let mut used = vec![false; roots.len()];
    for i in 0..roots.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        if roots[i].im == 0.0 {
            out.push(vec![roots[i]]);
            continue;
        }
        let partner = (0..roots.len())
            .filter(|&j| !used[j])
            .min_by(|&a, &b| {
                let da = (roots[a] - roots[i].conj()).norm();
                let db = (roots[b] - roots[i].conj()).norm();
                da.total_cmp(&db)
            });
        match partner {
            Some(j) => {
                used[j] = true;
                out.push(vec![roots[i], roots[i].conj()]);
            }
            None => out.push(vec![Complex::new(roots[i].re, 0.0)]),
        }
    }
    out
}

impl Rational {
    /// `gain`.
    pub fn constant(gain: f64) -> Self {
        Self {
            gain,
            delay: 0,
            zeros: Vec::new(),
            poles: Vec::new(),
        }
    }

    /// The response at `w` radians per sample.
    pub fn response(&self, w: f64) -> Complex<f64> {
        let num = self.zeros.iter().fold(unit(w, self.delay).scale(self.gain), |acc, &q| {
            acc * (Complex::new(1.0, 0.0) - q * unit(w, 1))
        });
        let den = self
            .poles
            .iter()
            .fold(Complex::new(1.0, 0.0), |acc, &p| acc * (Complex::new(1.0, 0.0) - p * unit(w, 1)));
        divide(num, den)
    }

    /// Every pole strictly inside the unit circle.
    pub fn is_stable(&self) -> bool {
        self.poles.iter().all(|p| p.norm() < 1.0)
    }

    /// The model with `zeros` and `poles` chosen from this one's factors
    /// and the given gain (no delay).
    pub fn from_factors(gain: f64, zeros: &[&Factor], poles: &[&Factor]) -> Self {
        Self {
            gain,
            delay: 0,
            zeros: zeros.iter().flat_map(|f| f.iter().copied()).collect(),
            poles: poles.iter().flat_map(|f| f.iter().copied()).collect(),
        }
    }

    /// As cascaded sections: conjugate pairs and pairs of real roots
    /// each make one second-order polynomial, a leftover real root a
    /// first-order one; the delay is `z⁻¹` factors; the gain sits on the
    /// first section.
    pub fn sections(&self) -> Vec<Section> {
        let quadratics = |roots: &[Complex<f64>]| -> Vec<[f64; 3]> {
            let mut out = Vec::new();
            let mut reals: Vec<f64> = Vec::new();
            for f in factors(roots) {
                if f.len() == 2 {
                    out.push([1.0, -2.0 * f[0].re, f[0].norm_sqr()]);
                } else {
                    reals.push(f[0].re);
                }
            }
            reals.sort_by(|a, b| b.abs().total_cmp(&a.abs()));
            for pair in reals.chunks(2) {
                match *pair {
                    [p, q] => out.push([1.0, -(p + q), p * q]),
                    [p] => out.push([1.0, -p, 0.0]),
                    _ => {}
                }
            }
            out
        };
        let mut num = quadratics(&self.zeros);
        let den = quadratics(&self.poles);
        let mut delay = self.delay;
        while delay > 0 {
            let d = delay.min(2);
            num.push(if d == 2 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] });
            delay -= d;
        }
        let n = num.len().max(den.len()).max(1);
        (0..n)
            .map(|i| {
                let mut b = num.get(i).copied().unwrap_or([1.0, 0.0, 0.0]);
                if i == 0 {
                    b.iter_mut().for_each(|v| *v *= self.gain);
                }
                let a = den.get(i).copied().unwrap_or([1.0, 0.0, 0.0]);
                Section { b, a: [a[1], a[2]] }
            })
            .collect()
    }

    /// The Sanathanan–Koerner fit of `nb` zeros and `na` poles to
    /// `points` (each weighted by its weight and, so the error is
    /// relative, by `1/|h|²`), `iterations` steps; unstable poles are
    /// reflected into the unit circle and the numerator refitted.
    pub fn fit(points: &[Point], nb: usize, na: usize, iterations: usize) -> Option<Self> {
        let cols = nb + 1 + na;
        if 2 * points.len() < cols {
            return None;
        }
        let mut a_prev = vec![1.0; na + 1];
        let mut b = vec![0.0; nb + 1];
        for _ in 0..iterations.max(1) {
            let mut rows = Vec::with_capacity(2 * points.len() * cols);
            let mut rhs = Vec::with_capacity(2 * points.len());
            for p in points {
                let s = (p.weight / p.h.norm_sqr().max(1e-300)).sqrt() / poly(&a_prev, p.w).norm();
                let mut re = Vec::with_capacity(cols);
                let mut im = Vec::with_capacity(cols);
                for k in 0..=nb {
                    let e = unit(p.w, k);
                    re.push(s * e.re);
                    im.push(s * e.im);
                }
                for k in 1..=na {
                    let e = p.h * unit(p.w, k);
                    re.push(-s * e.re);
                    im.push(-s * e.im);
                }
                rows.extend(re);
                rows.extend(im);
                rhs.push(s * p.h.re);
                rhs.push(s * p.h.im);
            }
            let x = least_squares(rows, 2 * points.len(), cols, rhs)?;
            b = x[..=nb].to_vec();
            let mut a = vec![1.0];
            a.extend_from_slice(&x[nb + 1..]);
            a_prev = a;
        }
        // Poles: z^na·A(z⁻¹) = zⁿᵃ + a1·zⁿᵃ⁻¹ + … (ascending for `roots`).
        let mut poles = roots(&a_prev.iter().rev().copied().collect::<Vec<_>>());
        let reflected = poles.iter().any(|p| p.norm() >= 1.0);
        for p in poles.iter_mut() {
            let r = p.norm();
            if r >= 1.0 {
                // 1/conj(p), nudged inside: the same magnitude response
                // to a gain.
                let q = p.conj().scale(1.0 / (r * r));
                *p = q.scale(0.999_999 / q.norm().max(0.999_999));
            }
        }
        if reflected {
            b = Self::numerator(points, &expand(&poles), nb)?;
        }
        Some(Self::from_coefficients(&b, poles))
    }

    /// The numerator that best fits `points` under a fixed denominator
    /// `a` (linear least squares, relative error).
    fn numerator(points: &[Point], a: &[f64], nb: usize) -> Option<Vec<f64>> {
        let cols = nb + 1;
        let mut rows = Vec::with_capacity(2 * points.len() * cols);
        let mut rhs = Vec::with_capacity(2 * points.len());
        for p in points {
            let s = (p.weight / p.h.norm_sqr().max(1e-300)).sqrt();
            let target = p.h * poly(a, p.w);
            let (mut re, mut im) = (Vec::new(), Vec::new());
            for k in 0..=nb {
                let e = unit(p.w, k);
                re.push(s * e.re);
                im.push(s * e.im);
            }
            rows.extend(re);
            rows.extend(im);
            rhs.push(s * target.re);
            rhs.push(s * target.im);
        }
        least_squares(rows, 2 * points.len(), cols, rhs)
    }

    /// From numerator coefficients (in z⁻¹) and the poles.
    fn from_coefficients(b: &[f64], poles: Vec<Complex<f64>>) -> Self {
        let largest = b.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let delay = b.iter().position(|v| v.abs() > 1e-12 * largest).unwrap_or(0);
        let b = &b[delay..];
        let gain = b[0];
        // Zeros: b0·z^m + b1·z^(m−1) + … (ascending for `roots`).
        let zeros = roots(&b.iter().rev().copied().collect::<Vec<_>>());
        Self {
            gain,
            delay,
            zeros,
            poles,
        }
    }

    /// Fit orders `1..=max_order` (as many zeros as poles) and keep the
    /// lowest whose weighted RMS error ([`rms_db`](Self::rms_db), what
    /// the fit minimises) is within `tolerance_db` of the best order's
    /// (or under `tolerance_db` outright); returned with its worst error
    /// ([`error_db`](Self::error_db)). The worst error can stay large
    /// where no low order reaches: an analog response's phase near
    /// Nyquist, which a rational function of `z` must bring to 0° or
    /// 180° there.
    pub fn fit_order(points: &[Point], max_order: usize, tolerance_db: f64) -> Option<(Self, f64)> {
        let fits: Vec<(Self, f64)> = (1..=max_order)
            .filter_map(|n| Self::fit(points, n, n, 20))
            .map(|r| {
                let e = Self::rms_db(&r, points);
                (r, e)
            })
            .collect();
        let best = fits.iter().map(|f| f.1).fold(f64::INFINITY, f64::min);
        fits.into_iter()
            .find(|f| f.1 <= tolerance_db.max(best + tolerance_db))
            .map(|(r, _)| {
                let worst = Self::error_db(&r, points);
                (r, worst)
            })
    }

    /// The weighted RMS of the relative complex error over `points`, as
    /// dB of `1 + e`.
    pub fn rms_db(r: &Self, points: &[Point]) -> f64 {
        let (mut e, mut w) = (0.0, 0.0);
        for p in points {
            e += p.weight * divide(r.response(p.w) - p.h, p.h).norm_sqr();
            w += p.weight;
        }
        20.0 * (1.0 + (e / w.max(1e-300)).sqrt()).log10()
    }

    /// The worst error over `points` (weights ignored): the magnitude of
    /// the relative complex error, in dB of `|1 + e|` (a 0.1 dB error is
    /// 1.2 % in magnitude or 0.66° in phase).
    pub fn error_db(r: &Self, points: &[Point]) -> f64 {
        points
            .iter()
            .map(|p| {
                let e = divide(r.response(p.w) - p.h, p.h).norm();
                20.0 * (1.0 + e).log10()
            })
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_rational_response_comes_back() {
        let truth = Rational {
            gain: 0.3,
            delay: 0,
            zeros: vec![Complex::new(1.0, 0.0), Complex::new(-0.4, 0.0)],
            poles: vec![Complex::new(0.995, 0.0), Complex::new(0.7, 0.4), Complex::new(0.7, -0.4)],
        };
        let points: Vec<Point> = (1..400)
            .map(|i| {
                let w = std::f64::consts::PI * (i as f64 / 400.0).powi(2);
                Point {
                    w,
                    h: truth.response(w),
                    weight: 1.0,
                }
            })
            .collect();
        let fit = Rational::fit(&points, 2, 3, 20).unwrap();
        assert!(Rational::error_db(&fit, &points) < 1e-6, "{fit:?}");
        let sections = fit.sections();
        let w = 0.3;
        let cascade = sections.iter().fold(Complex::new(1.0, 0.0), |acc, s| acc * s.response(w));
        assert!((cascade - truth.response(w)).norm() < 1e-9 * truth.response(w).norm());
        assert!(sections.iter().all(Section::is_stable));
    }
}
