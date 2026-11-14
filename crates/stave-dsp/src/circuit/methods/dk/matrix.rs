//! Dense row-major linear algebra for the small systems a circuit makes
//! (tens of unknowns): LU with partial pivoting, products, and a complex
//! solve for frequency responses. Factorisations run at prepare; the
//! per-sample Newton solve factors only the nonlinear subsystem, with
//! `Lu` (the same factorisation, column-major, for speed).

/// LU-factor the `n`×`n` matrix `a` in place, rows swapped as `pivots`
/// records. `false` when a pivot vanishes against the matrix's scale
/// (a floating node, a loop of voltage sources).
pub(crate) fn lu(a: &mut [f64], n: usize, pivots: &mut [usize]) -> bool {
    let a = &mut a[..n * n];
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
            let (top, bottom) = a.split_at_mut(row * n);
            top[k * n..(k + 1) * n].swap_with_slice(&mut bottom[..n]);
        }
        // Rows below eliminated against the pivot row, as slices (one
        // bounds check a row, and the update vectorises).
        let (upper, lower) = a.split_at_mut((k + 1) * n);
        let pivot_row = &upper[k * n..];
        let pivot = pivot_row[k];
        let tail = &pivot_row[k + 1..];
        for row in lower.chunks_exact_mut(n) {
            let f = row[k] / pivot;
            row[k] = f;
            if f != 0.0 {
                for (x, &p) in row[k + 1..].iter_mut().zip(tail) {
                    *x -= f * p;
                }
            }
        }
    }
    true
}

/// Solve with an [`lu`] factorisation, `b` overwritten by the solution.
pub(crate) fn lu_solve(a: &[f64], n: usize, pivots: &[usize], b: &mut [f64]) {
    let b = &mut b[..n];
    for k in 0..n {
        b.swap(k, pivots[k]);
    }
    for i in 0..n {
        let mut s = b[i];
        for (x, y) in a[i * n..i * n + i].iter().zip(&b[..i]) {
            s -= x * y;
        }
        b[i] = s;
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for (x, y) in a[i * n + i + 1..(i + 1) * n].iter().zip(&b[i + 1..]) {
            s -= x * y;
        }
        b[i] = s / a[i * n + i];
    }
}

/// The column stride [`Lu`] keeps for `n` unknowns: `n` rounded up to a
/// whole number of [`LANES`].
const fn padded(n: usize) -> usize {
    n.div_ceil(LANES) * LANES
}

/// The granularity of the per-sample factorisation's column updates:
/// four doubles (two SSE2 or NEON registers, one AVX).
const LANES: usize = 4;

/// The per-sample LU factorisation (the nonlinear subsystem's Jacobian,
/// factored once or more a sample): [`lu`]'s partial pivoting and its
/// arithmetic — the same pivots, the same products in the same order, so
/// the same factors (up to the sign of a zero; the solve's back
/// substitution sums in another order, so solutions agree to rounding) —
/// in a layout made for the small dense systems a circuit's subsystem
/// makes (tens of unknowns), where a row-major elimination spends more on
/// loop overhead than on arithmetic. At the Moog 904-A's 21 unknowns it
/// takes half the instructions.
///
/// Column-major, each column padded with zeros to [`LANES`]: the matrix
/// is assembled a column at a time (`K`'s columns times the devices'
/// slopes, contiguous), the pivot is searched down a contiguous column,
/// and eliminating below it updates each later column over the same
/// span — from the pivot row rounded down to a vector boundary to the
/// column's end, a width the compiler knows, so the update is straight-
/// line vector code with no remainder. The span's rows at and above the
/// pivot take a zero multiplier, so they are left exactly as they were.
#[derive(Clone, Debug, Default)]
pub(crate) struct Lu {
    n: usize,
    stride: usize,
    /// The matrix, column `c` at `c·stride`; after [`factor`](Self::factor),
    /// `U` on and above the diagonal and `L`'s multipliers below it.
    a: Vec<f64>,
    /// The current pivot's multipliers, zero at and above its row.
    multipliers: Vec<f64>,
    pivots: Vec<usize>,
}

impl Lu {
    pub fn new(n: usize) -> Self {
        let stride = padded(n);
        Self {
            n,
            stride,
            a: vec![0.0; n * stride],
            multipliers: vec![0.0; stride],
            pivots: vec![0; n],
        }
    }

    /// The matrix to assemble, as columns of [`stride`](Self::stride):
    /// the caller writes rows `0..n` of every column (the rows past `n`
    /// are padding, which no update can move off zero for a finite one).
    pub fn columns_mut(&mut self) -> &mut [f64] {
        &mut self.a
    }

    /// The column stride of [`columns_mut`](Self::columns_mut).
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Factor the assembled matrix in place; `false` when a pivot vanishes
    /// against the matrix's scale, as [`lu`].
    pub fn factor(&mut self) -> bool {
        let (n, s) = (self.n, self.stride);
        let a = &mut self.a[..n * s];
        let floor = largest_magnitude(a) * 1e-15;
        let multipliers = &mut self.multipliers[..s];
        multipliers.fill(0.0);
        for k in 0..n {
            let column = &a[k * s..k * s + n];
            let (mut best, mut row) = (column[k].abs(), k);
            for (i, x) in column.iter().enumerate().skip(k + 1) {
                if x.abs() > best {
                    (best, row) = (x.abs(), i);
                }
            }
            self.pivots[k] = row;
            if best.is_nan() || best <= floor {
                return false;
            }
            if row != k {
                for c in a.chunks_exact_mut(s) {
                    c.swap(k, row);
                }
            }
            let (done, rest) = a.split_at_mut((k + 1) * s);
            let column = &mut done[k * s..];
            let pivot = column[k];
            multipliers[k] = 0.0;
            for (m, x) in multipliers[k + 1..n].iter_mut().zip(&mut column[k + 1..n]) {
                *x /= pivot;
                *m = *x;
            }
            if k + 1 == n {
                break;
            }
            let start = k / LANES * LANES;
            let (later, multipliers) = (Later { k, start, stride: s }, &multipliers[start..]);
            match s - start {
                // One vector: a call costs more than the update (the 2×2
                // blocks an op-amp pedal splits into).
                4 => eliminate_any(rest, multipliers, later),
                8 => eliminate::<8>(rest, multipliers, later),
                12 => eliminate::<12>(rest, multipliers, later),
                16 => eliminate::<16>(rest, multipliers, later),
                20 => eliminate::<20>(rest, multipliers, later),
                24 => eliminate::<24>(rest, multipliers, later),
                28 => eliminate::<28>(rest, multipliers, later),
                32 => eliminate::<32>(rest, multipliers, later),
                36 => eliminate::<36>(rest, multipliers, later),
                40 => eliminate::<40>(rest, multipliers, later),
                44 => eliminate::<44>(rest, multipliers, later),
                48 => eliminate::<48>(rest, multipliers, later),
                _ => eliminate_any(rest, multipliers, later),
            }
        }
        true
    }

    /// Solve with the factorisation, `b` overwritten by the solution —
    /// [`lu_solve`]'s substitutions, a column at a time.
    pub fn solve(&self, b: &mut [f64]) {
        let (n, s) = (self.n, self.stride);
        let a = &self.a[..n * s];
        let b = &mut b[..n];
        for k in 0..n {
            b.swap(k, self.pivots[k]);
        }
        // Forward, a column at a time: each b[i] still subtracts its
        // terms in ascending k, as a row's dot product would.
        for k in 0..n {
            let x = b[k];
            for (y, l) in b[k + 1..].iter_mut().zip(&a[k * s + k + 1..k * s + n]) {
                *y -= l * x;
            }
        }
        // Back, a column at a time too: each solved unknown is taken from
        // the ones above it at once, independent updates where a row's dot
        // product would be one chain of dependent subtractions, the
        // latency the per-sample solve was bound by. Each b[i] subtracts
        // its terms in descending j, the reverse of a dot product's order:
        // the same solution to rounding.
        for k in (0..n).rev() {
            let x = b[k] / a[k * s + k];
            b[k] = x;
            for (y, u) in b[..k].iter_mut().zip(&a[k * s..k * s + k]) {
                *y -= u * x;
            }
        }
    }
}

/// The largest `|x|` (NaN ignored, as `f64::max` would), in [`LANES`]
/// running maxima so it vectorises; `a` is whole columns.
fn largest_magnitude(a: &[f64]) -> f64 {
    let mut best = [0.0f64; LANES];
    for chunk in a.chunks_exact(LANES) {
        for (m, x) in best.iter_mut().zip(chunk) {
            let x = x.abs();
            *m = if x > *m { x } else { *m };
        }
    }
    best.iter().fold(0.0f64, |m, &x| m.max(x))
}

/// Where the columns after pivot `k` are updated: from row `start`, a
/// vector boundary at or above `k`, each column `stride` long.
#[derive(Copy, Clone)]
struct Later {
    k: usize,
    start: usize,
    stride: usize,
}

/// Eliminate below pivot `k` in the later `columns`, each updated over
/// its last `W` rows against the `multipliers` from row `start` (zero at
/// and above `k`, leaving those rows as they are). Not inlined, and the
/// two slices passed as arguments of their own: so they are known not to
/// overlap, which the update's vectorisation needs.
#[inline(never)]
fn eliminate<const W: usize>(columns: &mut [f64], multipliers: &[f64], later: Later) {
    let Later { k, start, stride } = later;
    let Ok(multipliers) = <&[f64; W]>::try_from(multipliers) else {
        unreachable!("the multipliers span W rows")
    };
    for column in columns.chunks_exact_mut(stride) {
        let u = column[k];
        if let Ok(x) = <&mut [f64; W]>::try_from(&mut column[start..]) {
            for (x, &m) in x.iter_mut().zip(multipliers) {
                *x -= m * u;
            }
        }
    }
}

/// [`eliminate`] for a width it has no instance for, or one too short to
/// be worth a call.
#[inline]
fn eliminate_any(columns: &mut [f64], multipliers: &[f64], later: Later) {
    let Later { k, start, stride } = later;
    for column in columns.chunks_exact_mut(stride) {
        let u = column[k];
        for (x, &m) in column[start..].iter_mut().zip(multipliers) {
            *x -= m * u;
        }
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
    let x = &x[..cols];
    for (i, yi) in y.iter_mut().enumerate() {
        let mut s = 0.0;
        for (a, b) in m[i * cols..(i + 1) * cols].iter().zip(x) {
            s += a * b;
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

    /// The per-sample factorisation is the prepare-time one, value for
    /// value, and solves alike: every size from one unknown past the
    /// widest compiled update (each update width, and the fallback),
    /// pivoting on every column.
    #[test]
    fn the_column_major_lu_is_the_row_major_lu() {
        let mut lcg: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (lcg >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        for n in 1..=53 {
            let s: Vec<f64> = (0..n * n).map(|_| next()).collect();
            let mut row_major = s.clone();
            let mut pivots = vec![0; n];
            assert!(lu(&mut row_major, n, &mut pivots));
            let mut factored = Lu::new(n);
            let stride = factored.stride();
            let columns = factored.columns_mut();
            for r in 0..n {
                for c in 0..n {
                    columns[c * stride + r] = s[r * n + c];
                }
            }
            assert!(factored.factor());
            assert_eq!(factored.pivots, pivots, "n = {n}");
            for r in 0..n {
                for c in 0..n {
                    assert_eq!(factored.a[c * stride + r], row_major[r * n + c], "n = {n}, ({r}, {c})");
                }
            }
            // The back substitution sums in another order: the same
            // solution to rounding.
            let b: Vec<f64> = (0..n).map(|_| next()).collect();
            let (mut want, mut got) = (b.clone(), b);
            lu_solve(&row_major, n, &pivots, &mut want);
            factored.solve(&mut got);
            let scale = want.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() <= 1e-12 * scale, "n = {n}: {g} against {w}");
            }
        }
        let mut singular = Lu::new(2);
        singular.columns_mut().copy_from_slice(&[1.0, 2.0, 0.0, 0.0, 2.0, 4.0, 0.0, 0.0]);
        assert!(!singular.factor());
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
