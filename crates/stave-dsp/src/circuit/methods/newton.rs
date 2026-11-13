//! How hard a per-sample Newton solve tries, and what it did,
//! accumulated so a host can read it: a solve that hits its iteration
//! cap without converging is a defect to measure, not a log line.

pub use crate::primitives::solve::{NewtonStats, Solved};

/// The policy a topology's config carries for its Newton solve.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NewtonSettings {
    pub max_iterations: u32,
    /// Converged when every update is smaller, in the solved unit.
    pub tolerance: f64,
    /// Largest single update, in the solved unit.
    pub max_step: f64,
    /// Backtracking halves a step at most down to this fraction.
    pub min_damping: f64,
}

impl NewtonSettings {
    pub const DEFAULT: Self = Self {
        max_iterations: 32,
        tolerance: 1e-9,
        max_step: f64::INFINITY,
        min_damping: 1.0 / 1024.0,
    };
}

impl Default for NewtonSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl NewtonSettings {
    /// Damped Newton on one unknown. `residual(x)` returns the residual
    /// and its slope; `limit(candidate, current)` lets a device bound a
    /// step (a junction's pnjlim); it must not move the solution. A step that would not
    /// reduce |residual| is halved, down to `min_damping`.
    #[inline(always)]
    pub fn solve_scalar(
        &self,
        start: f64,
        residual: impl Fn(f64) -> (f64, f64),
        limit: impl Fn(f64, f64) -> f64,
    ) -> Solved {
        let mut x = start;
        let (mut r, mut slope) = residual(x);
        let (mut iterations, mut step) = (0, f64::INFINITY);
        while iterations < self.max_iterations && step.abs() >= self.tolerance {
            let full = (r / slope).clamp(-self.max_step, self.max_step);
            let mut damping = 1.0;
            let (next, next_r, next_slope) = loop {
                let candidate = limit(x - damping * full, x);
                let (cr, cs) = residual(candidate);
                if cr.abs() < r.abs() || damping <= self.min_damping {
                    break (candidate, cr, cs);
                }
                damping *= 0.5;
            };
            step = next - x;
            (x, r, slope) = (next, next_r, next_slope);
            iterations += 1;
        }
        Solved {
            value: x,
            iterations,
            converged: step.abs() < self.tolerance,
            final_step: step,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_iterations_caps_and_the_worst_step() {
        let mut s = NewtonStats::default();
        s.record(2, true, 1e-9);
        s.record(3, false, -1e-3);
        assert_eq!((s.solves, s.iterations, s.capped), (2, 5, 1));
        assert_eq!(s.max_final_step, 1e-3);
        assert_eq!(s.mean_iterations(), 2.5);
    }
}
