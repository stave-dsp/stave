//! What an iterative solve did, and the running tally a host reads.
//! The accounting lives here, below every layer that solves, so a
//! behavioral stage can report its Newton work without reaching up
//! into `circuit/`; the solvers themselves live in `circuit::methods`.

/// One solve's outcome.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Solved {
    pub value: f64,
    pub iterations: u32,
    pub converged: bool,
    pub final_step: f64,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct NewtonStats {
    pub solves: u64,
    pub iterations: u64,
    /// Solves that used every allowed iteration without converging.
    pub capped: u64,
    /// The largest final step, in the solved variable's units.
    pub max_final_step: f64,
}

impl NewtonStats {
    #[inline]
    pub fn record_solve(&mut self, solved: &Solved) {
        self.record(solved.iterations, solved.converged, solved.final_step);
    }

    #[inline]
    pub fn record(&mut self, iterations: u32, converged: bool, final_step: f64) {
        self.solves += 1;
        self.iterations += u64::from(iterations);
        self.capped += u64::from(!converged);
        self.max_final_step = self.max_final_step.max(final_step.abs());
    }

    pub fn mean_iterations(&self) -> f64 {
        if self.solves == 0 {
            0.0
        } else {
            self.iterations as f64 / self.solves as f64
        }
    }

    pub fn merge(&mut self, other: &Self) {
        self.solves += other.solves;
        self.iterations += other.iterations;
        self.capped += other.capped;
        self.max_final_step = self.max_final_step.max(other.max_final_step);
    }
}
