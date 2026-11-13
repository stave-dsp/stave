//! Dense row-major linear algebra for the small systems a circuit makes
//! (tens of unknowns): LU with partial pivoting, products, and a complex
//! solve for frequency responses. Factorisations run at prepare; the
//! per-sample Newton solve factors only the nonlinear subsystem.

/// LU-factor the `n`×`n` matrix `a` in place, rows swapped as `pivots`
/// records. `false` when a pivot vanishes against the matrix's scale
/// (a floating node, a loop of voltage sources).
pub(crate) fn lu(a: &mut [f64], n: usize, pivots: &mut [usize]) -> bool {
    let scale = a.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let floor = scale * 1e-15;
    for k in 0..n {
        let (mut best, mut row) = (a[k * n + k].abs(), k);
        for i in k + 1..n {
            let x = a[i * n + k].abs();
            if x > best {
                (best, row) = (x, i);
            }
        }
        pivots[k] = row;
        if best.is_nan() || best <= floor {
            return false;
        }
        if row != k {
            for j in 0..n {
                a.swap(k * n + j, row * n + j);
            }
        }
        let pivot = a[k * n + k];
        for i in k + 1..n {
            let f = a[i * n + k] / pivot;
            a[i * n + k] = f;
            if f != 0.0 {
                for j in k + 1..n {
                    a[i * n + j] -= f * a[k * n + j];
                }
            }
        }
    }
    true
}

/// Solve with an [`lu`] factorisation, `b` overwritten by the solution.
pub(crate) fn lu_solve(a: &[f64], n: usize, pivots: &[usize], b: &mut [f64]) {
    for k in 0..n {
        b.swap(k, pivots[k]);
    }
    for i in 0..n {
        let mut s = b[i];
        for j in 0..i {
            s -= a[i * n + j] * b[j];
        }
        b[i] = s;
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= a[i * n + j] * b[j];
        }
        b[i] = s / a[i * n + i];
    }
}

/// `S⁻¹·B` for `n`×`n` `s` and `n`×`cols` `b`, or `None` when singular.
pub(crate) fn solve_columns(s: &[f64], n: usize, b: &[f64], cols: usize) -> Option<Vec<f64>> {
    let mut a = s.to_vec();
    let mut pivots = vec![0; n];
    if !lu(&mut a, n, &mut pivots) {
        return None;
    }
    let mut out = vec![0.0; n * cols];
    let mut column = vec![0.0; n];
    for c in 0..cols {
        for r in 0..n {
            column[r] = b[r * cols + c];
        }
        lu_solve(&a, n, &pivots, &mut column);
        for r in 0..n {
            out[r * cols + c] = column[r];
        }
    }
    Some(out)
}

/// The `rows`×`cols` product of `rows`×`inner` `a` and `inner`×`cols` `b`.
pub(crate) fn product(a: &[f64], b: &[f64], rows: usize, inner: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; rows * cols];
    for i in 0..rows {
        for k in 0..inner {
            let x = a[i * inner + k];
            if x != 0.0 {
                for j in 0..cols {
                    out[i * cols + j] += x * b[k * cols + j];
                }
            }
        }
    }
    out
}

/// `y = M·x` for `rows`×`cols` `m`, accumulated into `y`.
#[inline]
pub(crate) fn accumulate(m: &[f64], x: &[f64], y: &mut [f64], cols: usize) {
    for (i, yi) in y.iter_mut().enumerate() {
        let row = &m[i * cols..(i + 1) * cols];
        let mut s = 0.0;
        for j in 0..cols {
            s += row[j] * x[j];
        }
        *yi += s;
    }
}

/// A complex number, for small-signal responses.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Phasor {
    pub re: f64,
    pub im: f64,
}

impl Phasor {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    pub fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Radians.
    pub fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }

    pub(crate) fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }

    pub(crate) fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }

    pub(crate) fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }

    pub(crate) fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
}

/// Solve the complex `n`×`n` system `a·x = b` by Gaussian elimination
/// with partial pivoting; `b` becomes `x`. `false` when singular.
pub(crate) fn complex_solve(a: &mut [Phasor], n: usize, b: &mut [Phasor]) -> bool {
    for k in 0..n {
        let (mut best, mut row) = (a[k * n + k].magnitude(), k);
        for i in k + 1..n {
            let x = a[i * n + k].magnitude();
            if x > best {
                (best, row) = (x, i);
            }
        }
        if best.is_nan() || best <= 0.0 {
            return false;
        }
        if row != k {
            for j in 0..n {
                a.swap(k * n + j, row * n + j);
            }
            b.swap(k, row);
        }
        let pivot = a[k * n + k];
        for i in k + 1..n {
            let f = a[i * n + k].div(pivot);
            for j in k..n {
                a[i * n + j] = a[i * n + j].sub(f.mul(a[k * n + j]));
            }
            b[i] = b[i].sub(f.mul(b[k]));
        }
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s = s.sub(a[i * n + j].mul(b[j]));
        }
        b[i] = s.div(a[i * n + i]);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lu_solves_a_pivoting_system_and_refuses_a_singular_one() {
        let s = [0.0, 2.0, 1.0, 1.0, 1.0, 0.0, 3.0, 0.0, 1.0];
        let x = solve_columns(&s, 3, &[3.0, 2.0, 4.0], 1).unwrap();
        for (got, want) in x.iter().zip([1.0, 1.0, 1.0]) {
            assert!((got - want).abs() < 1e-14, "{x:?}");
        }
        assert!(solve_columns(&[1.0, 2.0, 2.0, 4.0], 2, &[1.0, 1.0], 1).is_none());
    }

    #[test]
    fn the_complex_solve_inverts_a_rotation() {
        let i = Phasor::new(0.0, 1.0);
        let mut a = [Phasor::new(1.0, 0.0), i, i, Phasor::new(1.0, 0.0)];
        let mut b = [Phasor::new(1.0, 1.0), Phasor::new(1.0, 1.0)];
        assert!(complex_solve(&mut a, 2, &mut b));
        // (1 + i)·x = 1 + i  →  x = 1 for both.
        for x in b {
            assert!((x.re - 1.0).abs() < 1e-15 && x.im.abs() < 1e-15, "{b:?}");
        }
    }
}
