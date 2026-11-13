//! The DK matrices for a sample rate, the operating point, and the
//! small-signal view of both (frequency response, tail).

use super::matrix::{complex_solve, product, solve_columns, Phasor};
use super::{read, Dk, Pin};
use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::newton::NewtonSettings;
use crate::circuit::netlist::NetlistError;
use crate::kernel::node::Tail;

/// Rows `M[a] − M[b]` (scaled) of an `n`×`cols` matrix.
fn difference_row(m: &[f64], cols: usize, a: Pin, b: Pin, scale: f64, out: &mut [f64]) {
    for c in 0..cols {
        let x = a.map_or(0.0, |r| m[r * cols + c]) - b.map_or(0.0, |r| m[r * cols + c]);
        out[c] = scale * x;
    }
}

impl Dk {
    /// `S` with every node shunted by `shunt` siemens to ground.
    fn shunted(&self, mut s: Vec<f64>, shunt: f64) -> Vec<f64> {
        let n = self.n;
        for i in 0..self.nodes {
            s[i * n + i] += shunt;
        }
        s
    }

    /// `S⁻¹·rhs`, with SPICE's `gmin` added if the bare matrix is
    /// singular (a node only capacitors reach).
    fn solve_system(
        &self,
        s: Vec<f64>,
        rhs: &[f64],
        cols: usize,
        analysis: &'static str,
    ) -> Result<Vec<f64>, NetlistError> {
        if let Some(m) = solve_columns(&s, self.n, rhs, cols) {
            return Ok(m);
        }
        solve_columns(&self.shunted(s, super::GMIN), self.n, rhs, cols)
            .ok_or(NetlistError::Singular { analysis })
    }

    /// The trapezoidal matrices for the current step.
    pub(super) fn discretise(&mut self) -> Result<(), NetlistError> {
        let (n, t) = (self.n, self.design_step());
        let (nc, nl) = (self.capacitors.len(), self.inductors.len());
        let nx = nc + nl;
        let (nu, nq) = (self.inputs.len(), self.nq);
        let mut s = self.static_matrix.clone();
        let cols = nx + nu + nq;
        let mut rhs = vec![0.0; n * cols];
        // History-state scale per state: 2C/T for a capacitor, 2L/T for an inductor.
        let mut scale = vec![0.0; nx];
        for (k, c) in self.capacitors.iter().enumerate() {
            let g = 2.0 * c.farads / t;
            scale[k] = g;
            for (r, sr) in [(c.a, 1.0), (c.b, -1.0)] {
                if let Some(r) = r {
                    rhs[r * cols + k] += sr;
                    for (cc, sc) in [(c.a, 1.0), (c.b, -1.0)] {
                        if let Some(cc) = cc {
                            s[r * n + cc] += sr * sc * g;
                        }
                    }
                }
            }
        }
        for (k, l) in self.inductors.iter().enumerate() {
            let r = 2.0 * l.henries / t;
            scale[nc + k] = r;
            s[l.row * n + l.row] -= r;
            rhs[l.row * cols + nc + k] = -1.0;
        }
        for r in 0..n {
            for k in 0..nu {
                rhs[r * cols + nx + k] = self.bu[r * nu + k];
            }
            for k in 0..nq {
                rhs[r * cols + nx + nu + k] = self.bq[r * nq + k];
            }
        }
        let m = self.solve_system(s, &rhs, cols, "transient")?;
        let mut row = vec![0.0; cols];
        let split = |row: &[f64], x: &mut [f64], u: &mut [f64], q: &mut [f64], i: usize| {
            x[i * nx..(i + 1) * nx].copy_from_slice(&row[..nx]);
            u[i * nu..(i + 1) * nu].copy_from_slice(&row[nx..nx + nu]);
            q[i * nq..(i + 1) * nq].copy_from_slice(&row[nx + nu..]);
        };
        for (i, &(a, b)) in self.controls.iter().enumerate() {
            difference_row(&m, cols, a, b, 1.0, &mut row);
            split(&row, &mut self.dx, &mut self.du, &mut self.k, i);
        }
        for (k, c) in self.capacitors.iter().enumerate() {
            difference_row(&m, cols, c.a, c.b, 2.0 * scale[k], &mut row);
            split(&row, &mut self.ax, &mut self.au, &mut self.aq, k);
        }
        for (k, l) in self.inductors.iter().enumerate() {
            difference_row(&m, cols, Some(l.row), None, 2.0 * scale[nc + k], &mut row);
            split(&row, &mut self.ax, &mut self.au, &mut self.aq, nc + k);
        }
        for k in 0..nx {
            self.ax[k * nx + k] -= 1.0;
        }
        difference_row(&m, cols, self.probe.0, self.probe.1, 1.0, &mut row);
        split(&row, &mut self.ex, &mut self.eu, &mut self.eq, 0);
        self.tail = self.estimate_tail();
        self.adaa.kernel.find_one_port(&self.bq, self.n, &self.k, &self.devices);
        Ok(())
    }

    /// Capacitors open, inductors shorted: Newton from zero, then source
    /// stepping (16, 128, then 1024 steps). Under `UIC` there is no
    /// operating point to find.
    pub(super) fn solve_operating_point(&mut self) -> Result<(), NetlistError> {
        let (n, nu, nq, nv) = (self.n, self.inputs.len(), self.nq, self.controls.len());
        if self.uic {
            self.dc_w.iter_mut().for_each(|x| *x = 0.0);
            self.dc_v.iter_mut().for_each(|x| *x = 0.0);
            return Ok(());
        }
        let cols = nu + nq;
        let mut rhs = vec![0.0; n * cols];
        for r in 0..n {
            rhs[r * cols..r * cols + nu].copy_from_slice(&self.bu[r * nu..(r + 1) * nu]);
            rhs[r * cols + nu..(r + 1) * cols].copy_from_slice(&self.bq[r * nq..(r + 1) * nq]);
        }
        let w = self.solve_system(self.static_matrix.clone(), &rhs, cols, "operating point")?;
        let mut u0 = vec![0.0; nu];
        for (k, input) in self.inputs.iter_mut().enumerate() {
            u0[k] = if self.driven == Some(k) {
                0.0
            } else {
                match input {
                    super::Input::Source(s) => s.value_at(0.0),
                    super::Input::Expression { tape, stack } => tape.eval(&[], 0.0, stack).value,
                }
            };
        }
        let (mut p0, mut k_dc) = (vec![0.0; nv], vec![0.0; nv * nq]);
        let mut row = vec![0.0; cols];
        for (i, &(a, b)) in self.controls.iter().enumerate() {
            difference_row(&w, cols, a, b, 1.0, &mut row);
            p0[i] = (0..nu).map(|k| row[k] * u0[k]).sum();
            k_dc[i * nq..(i + 1) * nq].copy_from_slice(&row[nu..]);
        }
        // The operating point is solved once, off the audio thread: eight
        // times the per-sample cap from a cold start.
        let newton = NewtonSettings {
            max_iterations: self.settings.newton.max_iterations.saturating_mul(8),
            ..self.settings.newton
        };
        let mut converged = {
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
            self.work.newton(&p0, &k_dc, &mut self.devices, 0.0, &newton).converged
        };
        let mut p = vec![0.0; nv];
        // Source stepping: every source scaled from 0 to its value, each
        // step from the last solution. Off with the continuation fallback.
        let stepping: &[u32] = if self.settings.continuation_steps > 0 {
            &[16, 128, 1024]
        } else {
            &[]
        };
        for &steps in stepping {
            if converged {
                break;
            }
            self.work.v.iter_mut().for_each(|v| *v = 0.0);
            for s in 1..=steps {
                let f = f64::from(s) / f64::from(steps);
                p.iter_mut().zip(&p0).for_each(|(p, p0)| *p = f * p0);
                converged = self.work.newton(&p, &k_dc, &mut self.devices, 0.0, &newton).converged;
                if !converged {
                    break;
                }
            }
        }
        if !converged {
            return Err(NetlistError::NoOperatingPoint);
        }
        let q = &self.work.q;
        for r in 0..n {
            let mut x = 0.0;
            for k in 0..nu {
                x += w[r * cols + k] * u0[k];
            }
            for k in 0..nq {
                x += w[r * cols + nu + k] * q[k];
            }
            self.dc_w[r] = x;
        }
        self.dc_v.copy_from_slice(&self.work.v);
        Ok(())
    }

    /// The circuit linearised at its operating point, for input column
    /// `input`: `(A, B, C, D)` with `x' = A·x + B·u`, `y = C·x + D·u`.
    fn linearised(&self, input: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>, f64) {
        let (nx, nu, nq, nv) = (self.x.len(), self.inputs.len(), self.nq, self.controls.len());
        let mut work = self.work.clone();
        let mut devices = self.devices.clone();
        if self.uic {
            work.v.iter_mut().for_each(|v| *v = 0.0);
        } else {
            work.v.copy_from_slice(&self.dc_v);
        }
        work.evaluate(&mut devices, 0.0);
        let jq = work.jacobian_of_outputs();
        // (I − K·Jq)·L = [Dx | Du·e_input]
        let kj = product(&self.k, jq, nv, nq, nv);
        let mut m = vec![0.0; nv * nv];
        for i in 0..nv {
            for c in 0..nv {
                m[i * nv + c] = if i == c { 1.0 } else { 0.0 } - kj[i * nv + c];
            }
        }
        let mut rhs = vec![0.0; nv * (nx + 1)];
        for i in 0..nv {
            rhs[i * (nx + 1)..i * (nx + 1) + nx].copy_from_slice(&self.dx[i * nx..(i + 1) * nx]);
            rhs[i * (nx + 1) + nx] = self.du[i * nu + input];
        }
        let l = if nv == 0 {
            Vec::new()
        } else {
            solve_columns(&m, nv, &rhs, nx + 1).unwrap_or_else(|| vec![0.0; nv * (nx + 1)])
        };
        let jl = product(jq, &l, nq, nv, nx + 1);
        let aj = product(&self.aq, &jl, nx, nq, nx + 1);
        let ej = product(&self.eq, &jl, 1, nq, nx + 1);
        let mut a = self.ax.clone();
        let mut b = vec![0.0; nx];
        for i in 0..nx {
            for c in 0..nx {
                a[i * nx + c] += aj[i * (nx + 1) + c];
            }
            b[i] = self.au[i * nu + input] + aj[i * (nx + 1) + nx];
        }
        let mut c = self.ex.clone();
        for j in 0..nx {
            c[j] += ej[j];
        }
        let d = self.eu[input] + ej[nx];
        (a, b, c, d)
    }

    /// The input column a response is taken from, and its scale.
    fn response_input(&self) -> Option<(usize, f64)> {
        match self.driven {
            Some(k) => Some((k, 1.0)),
            None => self
                .input_ac
                .iter()
                .position(|&ac| ac != 0.0)
                .map(|k| (k, self.input_ac[k])),
        }
    }

    pub(super) fn response_at(&self, angle: f64) -> Option<Phasor> {
        let (input, scale) = self.response_input()?;
        let nx = self.x.len();
        let (a, b, c, d) = self.linearised(input);
        let mut zi = Phasor::new(angle.cos(), -angle.sin());
        // ADAA (Holters 2019, eq. 13): H̃(z) = ½(1 + z⁻¹)·H(ζ) with
        // ζ⁻¹ = ½(z⁻¹ + z⁻²), the matrices being designed for 1.5·T.
        let adaa = self.antialiasing == Antialiasing::Adaa;
        let half = Phasor::new(0.5, 0.0);
        let outer = half.mul(Phasor::new(1.0, 0.0).add(zi));
        if adaa {
            zi = half.mul(zi.add(zi.mul(zi)));
        }
        // (I − A·z⁻¹)·X = B;  H = D + z⁻¹·C·X.
        let mut m: Vec<Phasor> = (0..nx * nx)
            .map(|i| {
                let one = if i / nx == i % nx { 1.0 } else { 0.0 };
                Phasor::new(one, 0.0).sub(zi.mul(Phasor::new(a[i], 0.0)))
            })
            .collect();
        let mut x: Vec<Phasor> = b.iter().map(|&b| Phasor::new(b, 0.0)).collect();
        if !complex_solve(&mut m, nx, &mut x) {
            return None;
        }
        let mut cx = Phasor::default();
        for j in 0..nx {
            cx = cx.add(Phasor::new(c[j], 0.0).mul(x[j]));
        }
        let mut h = Phasor::new(d, 0.0).add(zi.mul(cx));
        if adaa {
            h = outer.mul(h);
        }
        Some(Phasor::new(scale * h.re, scale * h.im))
    }

    /// The linearised state matrix's spectral radius, by power iteration
    /// (the norm's mean growth, so a complex pair reads its modulus).
    fn estimate_tail(&self) -> Tail {
        let nx = self.x.len();
        if nx == 0 {
            return Tail::None;
        }
        let input = self.response_input().map_or(0, |(k, _)| k);
        let a = if self.inputs.is_empty() {
            self.ax.clone()
        } else {
            self.linearised(input).0
        };
        let mut v: Vec<f64> = (0..nx).map(|i| 1.0 + 0.1 * i as f64).collect();
        let (mut log_growth, mut counted) = (0.0, 0);
        for iteration in 0..512u32 {
            let next = product(&a, &v, nx, nx, 1);
            let norm = next.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm == 0.0 {
                return Tail::samples(iteration + 1);
            }
            if !norm.is_finite() {
                return Tail::Infinite;
            }
            if iteration >= 256 {
                log_growth += norm.ln();
                counted += 1;
            }
            v = next.into_iter().map(|x| x / norm).collect();
        }
        // A pole per design step of 1.5·T under ADAA is its 1/1.5 power
        // per sample.
        let per_sample = log_growth / f64::from(counted) / self.antialiasing.design_step_scale();
        Tail::from_pole(per_sample.exp())
    }

    /// The probe at the operating point.
    pub(super) fn operating_output(&self) -> f64 {
        read(&self.dc_w, self.probe.0) - read(&self.dc_w, self.probe.1)
    }
}
