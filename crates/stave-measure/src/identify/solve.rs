//! The small dense numerics identification needs: least squares by
//! Householder QR, normal equations for sparse rows, and the roots of a
//! real polynomial.

use crate::suite::{divide, Complex};

/// `min ‖A·x − b‖₂` by Householder QR with column equilibration; `a` is
/// row-major `rows × cols`. `None` when `A` is rank-deficient to working
/// precision.
pub fn least_squares(mut a: Vec<f64>, rows: usize, cols: usize, mut b: Vec<f64>) -> Option<Vec<f64>> {
    assert!(rows >= cols && a.len() == rows * cols && b.len() == rows);
    // Equilibrate the columns: the solution is rescaled at the end.
    let scale: Vec<f64> = (0..cols)
        .map(|j| {
            let n = (0..rows).map(|i| a[i * cols + j].powi(2)).sum::<f64>().sqrt();
            if n > 0.0 {
                1.0 / n
            } else {
                1.0
            }
        })
        .collect();
    for i in 0..rows {
        for j in 0..cols {
            a[i * cols + j] *= scale[j];
        }
    }
    let mut largest = 0.0f64;
    for k in 0..cols {
        let norm = (k..rows).map(|i| a[i * cols + k].powi(2)).sum::<f64>().sqrt();
        largest = largest.max(norm);
        if norm <= 1e-13 * largest.max(1e-300) {
            return None;
        }
        let alpha = if a[k * cols + k] > 0.0 { -norm } else { norm };
        // v = x − α·e1, stored in place of column k.
        a[k * cols + k] -= alpha;
        let vnorm2: f64 = (k..rows).map(|i| a[i * cols + k].powi(2)).sum();
        for j in k + 1..cols {
            let dot: f64 = (k..rows).map(|i| a[i * cols + k] * a[i * cols + j]).sum();
            let f = 2.0 * dot / vnorm2;
            for i in k..rows {
                a[i * cols + j] -= f * a[i * cols + k];
            }
        }
        let dot: f64 = (k..rows).map(|i| a[i * cols + k] * b[i]).sum();
        let f = 2.0 * dot / vnorm2;
        for i in k..rows {
            b[i] -= f * a[i * cols + k];
        }
        a[k * cols + k] = alpha;
    }
    let mut x = vec![0.0; cols];
    for k in (0..cols).rev() {
        let s: f64 = (k + 1..cols).map(|j| a[k * cols + j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k * cols + k];
    }
    Some(x.iter().zip(&scale).map(|(v, s)| v * s).collect())
}

/// Normal equations `AᵀA·x = Aᵀb`, accumulated row by row (sparse rows
/// welcome), solved by Cholesky with a relative ridge.
#[derive(Clone, Debug)]
pub struct Normal {
    n: usize,
    ata: Vec<f64>,
    atb: Vec<f64>,
}

impl Normal {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            ata: vec![0.0; n * n],
            atb: vec![0.0; n],
        }
    }

    /// A row given as `(column, value)` pairs, its target and weight.
    pub fn add_sparse(&mut self, row: &[(usize, f64)], target: f64, weight: f64) {
        for &(i, a) in row {
            self.atb[i] += weight * a * target;
            for &(j, b) in row {
                self.ata[i * self.n + j] += weight * a * b;
            }
        }
    }

    /// A dense row.
    pub fn add(&mut self, row: &[f64], target: f64, weight: f64) {
        for i in 0..self.n {
            let wa = weight * row[i];
            if wa == 0.0 {
                continue;
            }
            self.atb[i] += wa * target;
            for j in 0..self.n {
                self.ata[i * self.n + j] += wa * row[j];
            }
        }
    }

    /// The solution with `ridge` times the mean diagonal added to the
    /// diagonal (a small one only steadies a near-singular system).
    pub fn solve(&self, ridge: f64) -> Option<Vec<f64>> {
        let n = self.n;
        let mean = (0..n).map(|i| self.ata[i * n + i]).sum::<f64>() / n as f64;
        let mut l = self.ata.clone();
        for i in 0..n {
            l[i * n + i] += ridge * mean.max(1e-300);
        }
        // In-place Cholesky, lower triangle.
        for j in 0..n {
            let mut d = l[j * n + j];
            for k in 0..j {
                d -= l[j * n + k] * l[j * n + k];
            }
            if d.is_nan() || d <= 0.0 {
                return None;
            }
            let d = d.sqrt();
            l[j * n + j] = d;
            for i in j + 1..n {
                let mut s = l[i * n + j];
                for k in 0..j {
                    s -= l[i * n + k] * l[j * n + k];
                }
                l[i * n + j] = s / d;
            }
        }
        let mut y = self.atb.clone();
        for i in 0..n {
            for k in 0..i {
                y[i] -= l[i * n + k] * y[k];
            }
            y[i] /= l[i * n + i];
        }
        for i in (0..n).rev() {
            for k in i + 1..n {
                y[i] -= l[k * n + i] * y[k];
            }
            y[i] /= l[i * n + i];
        }
        Some(y)
    }
}

/// The roots of `c[0] + c[1]·z + … + c[n]·zⁿ` (real coefficients,
/// `c[n] ≠ 0`) by the Aberth–Ehrlich iteration, each polished by
/// Newton's method; conjugate pairs come out exactly conjugate and real
/// roots exactly real.
pub fn roots(c: &[f64]) -> Vec<Complex<f64>> {
    let mut c = c.to_vec();
    while c.len() > 1 && *c.last().unwrap() == 0.0 {
        c.pop();
    }
    let n = c.len() - 1;
    if n == 0 {
        return Vec::new();
    }
    let lead = c[n];
    let c: Vec<f64> = c.iter().map(|v| v / lead).collect();
    let eval = |z: Complex<f64>| {
        // p(z) and p'(z) by Horner.
        let mut p = Complex::new(1.0, 0.0);
        let mut d = Complex::new(0.0, 0.0);
        for k in (0..n).rev() {
            d = d * z + p;
            p = p * z + Complex::new(c[k], 0.0);
        }
        (p, d)
    };
    let radius = 1.0 + c[..n].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let mut z: Vec<Complex<f64>> = (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * (k as f64 + 0.25) / n as f64 + 0.4;
            Complex::new(0.5 * radius * a.cos(), 0.5 * radius * a.sin())
        })
        .collect();
    for _ in 0..500 {
        let mut moved = 0.0f64;
        for i in 0..n {
            let (p, d) = eval(z[i]);
            if p.norm() == 0.0 {
                continue;
            }
            let ratio = divide(p, d);
            let mut sum = Complex::new(0.0, 0.0);
            for j in 0..n {
                if j != i {
                    sum = sum + divide(Complex::new(1.0, 0.0), z[i] - z[j]);
                }
            }
            let denom = Complex::new(1.0, 0.0) - ratio * sum;
            let step = divide(ratio, denom);
            if step.re.is_finite() && step.im.is_finite() {
                z[i] = z[i] - step;
                moved = moved.max(step.norm() / z[i].norm().max(1e-300));
            }
        }
        if moved < 1e-15 {
            break;
        }
    }
    for r in z.iter_mut() {
        for _ in 0..3 {
            let (p, d) = eval(*r);
            let step = divide(p, d);
            if step.re.is_finite() && step.im.is_finite() {
                *r = *r - step;
            }
        }
    }
    // Clean: near-real roots real; pair the rest as exact conjugates.
    let mut out: Vec<Complex<f64>> = Vec::with_capacity(n);
    let mut complex: Vec<Complex<f64>> = Vec::new();
    for r in z {
        if r.im.abs() <= 1e-9 * r.norm().max(1e-12) {
            out.push(Complex::new(r.re, 0.0));
        } else if r.im > 0.0 {
            complex.push(r);
        }
    }
    for r in complex {
        out.push(r);
        out.push(r.conj());
    }
    // A root lost by a pairing mismatch (two near-real roots that split
    // as a pair): keep the degree.
    while out.len() < n {
        out.push(Complex::new(0.0, 0.0));
    }
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn least_squares_solves_an_overdetermined_line() {
        // y = 2 − 3x, exactly.
        let xs = [0.0, 1.0, 2.0, 3.0, 4.0];
        let a: Vec<f64> = xs.iter().flat_map(|&x| [1.0, x]).collect();
        let b: Vec<f64> = xs.iter().map(|&x| 2.0 - 3.0 * x).collect();
        let x = least_squares(a, 5, 2, b).unwrap();
        assert!((x[0] - 2.0).abs() < 1e-12 && (x[1] + 3.0).abs() < 1e-12, "{x:?}");
        let mut n = Normal::new(2);
        for &t in &xs {
            n.add_sparse(&[(0, 1.0), (1, t)], 2.0 - 3.0 * t, 1.0);
        }
        let y = n.solve(0.0).unwrap();
        assert!((y[0] - 2.0).abs() < 1e-10 && (y[1] + 3.0).abs() < 1e-10, "{y:?}");
    }

    #[test]
    fn roots_of_a_real_polynomial() {
        // (z − 0.999)(z + 0.5)(z² − 1.2z + 0.72): a slow real pole, a
        // real one and a pair at radius √0.72.
        let mut c = vec![1.0];
        for f in [[-0.999, 1.0, 0.0], [0.5, 1.0, 0.0], [0.72, -1.2, 1.0]] {
            let mut next = vec![0.0; c.len() + 2];
            for (i, a) in c.iter().enumerate() {
                for (j, b) in f.iter().enumerate() {
                    next[i + j] += a * b;
                }
            }
            c = next;
        }
        let mut r = roots(&c);
        r.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap().then(a.im.partial_cmp(&b.im).unwrap()));
        let want = [(-0.5, 0.0), (0.6, -0.6), (0.6, 0.6), (0.999, 0.0)];
        assert_eq!(r.len(), 4);
        for (z, w) in r.iter().zip(want) {
            assert!((z.re - w.0).abs() < 1e-9 && (z.im - w.1).abs() < 1e-9, "{r:?}");
        }
    }
}
