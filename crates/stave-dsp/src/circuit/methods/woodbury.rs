//! The Woodbury identity as a method's knob-move update (N6): matrices
//! solved through a system `S₀` at prepare, corrected for
//! `S = S₀ + E·Δ·Eᵀ` (`k` variable branches, `Δ` their changes) without
//! factoring `S`.
//!
//! With `Y = S₀⁻¹·E` and `Q = Eᵀ·Y`, push-through gives
//! `S⁻¹ = S₀⁻¹ − Y·(I + Δ·Q)⁻¹·Δ·Eᵀ·S₀⁻¹`. A method reads rows `R` of
//! `M = S⁻¹·B` for some right-hand side `B` that may itself move as
//! `B₀ + E·Δ·F` (a wave port's Norton injection does), so
//!
//! ```text
//!     R·M = R·M₀ + (R·Y)·(I + Δ·Q)⁻¹·Δ·(F − Eᵀ·M₀)
//!         = base  +   U  ·    A⁻¹    ·Δ·   W
//! ```
//!
//! `base`, `U`, `Q` and `W` are the method's, computed at prepare; an
//! update factors the `k`×`k` matrix `A` and costs
//! `O(k³ + k²·cols + rows·k·cols)`, allocating nothing.

use super::dk::matrix::{lu, lu_solve};

#[derive(Clone, Debug)]
pub(crate) struct Woodbury {
    k: usize,
    rows: usize,
    cols: usize,
    q: Vec<f64>,
    u: Vec<f64>,
    w: Vec<f64>,
    base: Vec<f64>,
    /// `R·M` after the last update (`rows`×`cols`, row-major).
    pub out: Vec<f64>,
    a: Vec<f64>,
    pivots: Vec<usize>,
    x: Vec<f64>,
    column: Vec<f64>,
}

impl Woodbury {
    /// `q` is `k`×`k`, `u` `rows`×`k`, `w` `k`×`cols`, `base` `rows`×`cols`.
    pub fn new(k: usize, rows: usize, cols: usize, q: Vec<f64>, u: Vec<f64>, w: Vec<f64>, base: Vec<f64>) -> Self {
        debug_assert_eq!((q.len(), u.len(), w.len(), base.len()), (k * k, rows * k, k * cols, rows * cols));
        Self {
            k,
            rows,
            cols,
            q,
            u,
            w,
            out: base.clone(),
            base,
            a: vec![0.0; k * k],
            pivots: vec![0; k],
            x: vec![0.0; k * cols],
            column: vec![0.0; k],
        }
    }

    /// `out = base + U·(I + Δ·Q)⁻¹·Δ·W` for the branches' changes
    /// `delta`; false (and `out` untouched) when `I + Δ·Q` is singular —
    /// the new values short a source or float a node.
    pub fn update(&mut self, delta: &[f64]) -> bool {
        let (k, cols) = (self.k, self.cols);
        for i in 0..k {
            for j in 0..k {
                self.a[i * k + j] = if i == j { 1.0 } else { 0.0 } + delta[i] * self.q[i * k + j];
            }
        }
        if !lu(&mut self.a, k, &mut self.pivots) {
            return false;
        }
        for c in 0..cols {
            for i in 0..k {
                self.column[i] = delta[i] * self.w[i * cols + c];
            }
            lu_solve(&self.a, k, &self.pivots, &mut self.column);
            for i in 0..k {
                self.x[i * cols + c] = self.column[i];
            }
        }
        self.out.copy_from_slice(&self.base);
        for r in 0..self.rows {
            let out = &mut self.out[r * cols..(r + 1) * cols];
            for i in 0..k {
                let u = self.u[r * k + i];
                if u != 0.0 {
                    for (o, x) in out.iter_mut().zip(&self.x[i * cols..(i + 1) * cols]) {
                        *o += u * x;
                    }
                }
            }
        }
        true
    }
}
