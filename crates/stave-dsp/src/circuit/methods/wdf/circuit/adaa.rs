//! First-order ADAA in `Circuit<Wdf>` (Albertini, Bernardini & Sarti,
//! DAFx 2020 and JAES 2021, on an R-type root): the trees' capacitors
//! and inductors adapted for `1.5·T`, the nonlinear root's outputs
//! replaced by their mean over the sample's line of predictions (the DK
//! method's [`Kernel`]: the same `v = p + K·q(v)`, in closed form for a
//! one-port of diodes, by quadrature otherwise), and the pass down run on
//! every tree's wave and every absorbed source averaged with its last
//! value, so the whole backward scan sits at `n − ½`.

use super::{Input, NonlinearRoot, Target, Wdf};
use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::dk::adaa::{Kernel, Mean};
use crate::circuit::methods::dk::matrix::Phasor;
use crate::circuit::methods::dk::solve::Prediction;

/// `Circuit<Wdf>`'s ADAA history, sized at build.
#[derive(Clone, Debug)]
pub(super) struct State {
    pub kernel: Kernel,
    /// The absorbed sources' last values.
    u_prev: Vec<f64>,
}

impl State {
    pub fn new(nu: usize, nv: usize, nq: usize) -> Self {
        Self {
            kernel: Kernel::new(nv, nq),
            u_prev: vec![0.0; nu],
        }
    }
}

impl Wdf {
    /// Anti-alias the nonlinear root (DSP.md A1): with
    /// [`Antialiasing::Adaa`], first-order ADAA (module docs of
    /// [`methods::adaa`](crate::circuit::methods::adaa)). The root is solved
    /// by the K-method (a scattering root, [`NonlinearRoot::Scattering`],
    /// is set back to [`NonlinearRoot::Newton`]). Re-prepares at the
    /// current rate.
    pub fn set_antialiasing(&mut self, antialiasing: Antialiasing) {
        self.antialiasing = antialiasing;
        if antialiasing == Antialiasing::Adaa && self.root == NonlinearRoot::Scattering {
            let newton = self.set_nonlinear_root(NonlinearRoot::Newton);
            debug_assert!(newton.is_ok(), "the Newton root takes any netlist the scattering one does");
        }
        self.prepare(self.sample_rate);
    }

    pub fn antialiasing(&self) -> Antialiasing {
        self.antialiasing
    }

    /// How ADAA forms the root's mean, `None` with ADAA off.
    pub fn adaa_mean(&self) -> Option<Mean> {
        self.adaa_active().then(|| self.adaa.kernel.mean())
    }

    /// ADAA is on and the root is the K-method's.
    pub(super) fn adaa_active(&self) -> bool {
        self.antialiasing == Antialiasing::Adaa && self.root == NonlinearRoot::Newton
    }

    /// At rest: the trees' waves and the sources at t = 0 are the last
    /// sample's. Called at the end of `reset`.
    pub(super) fn reset_adaa(&mut self) {
        for (k, slot) in self.slots.iter_mut().enumerate() {
            let value = if self.driven == Some(k) {
                0.0
            } else {
                match &mut slot.input {
                    Input::Source(s) => s.value_at(0.0),
                    Input::Expression { tape, stack } => tape.eval(&[], 0.0, stack).value,
                }
            };
            match slot.target {
                Target::Leaf(cell) => self.forest.cells[cell].state = value,
                Target::Root(j) => self.adaa.u_prev[j] = value,
            }
        }
        self.forest.up();
        self.forest.rest();
        self.adaa.kernel.reset(&self.work.v, &self.work.q, &self.k);
    }

    /// One sample under ADAA, from the sources' values in `self.values`:
    /// the pass up as usual, the root's mean, then the pass down
    /// synchronized.
    pub(super) fn advance_adaa(&mut self, time: f64) -> f64 {
        let na = self.reflective.len();
        for (slot, &value) in self.slots.iter().zip(&self.values) {
            match slot.target {
                Target::Leaf(cell) => self.forest.cells[cell].state = value,
                Target::Root(j) => self.z[na + j] = value,
            }
        }
        self.forest.up();
        for (z, &cell) in self.z.iter_mut().zip(&self.reflective_cells) {
            *z = self.forest.cells[cell].beta;
        }
        let nl = na + self.nu;
        let cols = self.z.len();
        if !self.devices.is_empty() {
            for (i, p) in self.p.iter_mut().enumerate() {
                let row = &self.predict[i * nl..(i + 1) * nl];
                *p = row.iter().zip(&self.z[..nl]).map(|(m, z)| m * z).sum();
            }
            let interior = self.adaa.kernel.begin(&self.p);
            for j in 0..interior {
                let p = self.adaa.kernel.node(j, &self.p);
                self.work.solve(
                    Prediction { p, k: &self.k },
                    &mut self.devices,
                    time,
                    &self.settings,
                    &mut self.health,
                );
                self.adaa.kernel.add(j, &self.work.q);
            }
            if self.p.len() != 1 || !self.scalar_root(time) {
                self.work.solve(
                    Prediction {
                        p: &self.p,
                        k: &self.k,
                    },
                    &mut self.devices,
                    time,
                    &self.settings,
                    &mut self.health,
                );
            }
            let mean = self
                .adaa
                .kernel
                .finish(&self.p, &self.work.v, &self.work.q, &self.devices, interior);
            self.z[nl..].copy_from_slice(mean);
        }
        // Every wave into the root's backward scan at n − ½.
        self.forest.synchronize();
        for (z, &cell) in self.z.iter_mut().zip(&self.reflective_cells) {
            *z = self.forest.cells[cell].beta;
        }
        for (z, previous) in self.z[na..nl].iter_mut().zip(&mut self.adaa.u_prev) {
            let raw = *z;
            *z = 0.5 * (raw + *previous);
            *previous = raw;
        }
        let z = &self.z;
        let y: f64 = self.probe_row.iter().zip(z).map(|(m, z)| m * z).sum();
        for (row, &cell) in self.scatter.chunks_exact(cols.max(1)).zip(&self.stateful_cells) {
            let b: f64 = row.iter().zip(z).map(|(m, z)| m * z).sum();
            self.forest.cells[cell].alpha = b;
        }
        self.forest.down();
        self.output = y;
        y
    }
}

/// Holters' eq. (13) on a response read at `zi = z⁻¹`: the plain
/// response at `ζ⁻¹ = ½(z⁻¹ + z⁻²)`, times `½(1 + z⁻¹)`. Returns the
/// argument to evaluate the plain response at, and the outer factor.
pub(super) fn holters(zi: Phasor) -> (Phasor, Phasor) {
    let half = Phasor::new(0.5, 0.0);
    (half.mul(zi.add(zi.mul(zi))), half.mul(Phasor::new(1.0, 0.0).add(zi)))
}

#[cfg(test)]
mod tests;
