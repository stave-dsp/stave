//! The Scattering Iterative Method (Proverbio, Bernardini & Sarti,
//! "Toward the wave digital real-time emulation of audio circuits with
//! multiple nonlinearities", EUSIPCO 2020; its parallel form, Bernardini,
//! Sarti et al., JAES 2022): every one-port
//! nonlinearity is a wave port of the R-type root with its own port
//! resistance and its own scalar solve, and the root's scattering
//! carries each one's reflected wave to the others, sweep after sweep,
//! until the waves stop moving.
//!
//! Each sweep is a Jacobi step — all ports from the last sweep's waves —
//! so the ports are independent within it (lanes, threads). For passive
//! one-ports the sweep is a contraction; each port's resistance is a
//! hundred times the Thévenin resistance the network shows it, which
//! keeps the coupling between ports, and so the number of sweeps, small,
//! and each port's own reflection is solved with it.

use super::{read_col, Pin, Wdf};
use crate::circuit::devices::diode::Junction;
use crate::circuit::methods::dk::devices::{Device, SHUNT};
use crate::circuit::methods::dk::matrix::solve_columns;
use crate::circuit::netlist::NetlistError;

/// How the devices at a [`Wdf`]'s root are solved.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum NonlinearRoot {
    /// Werner's multi-port root: the devices' controlling voltages
    /// `v = p + K·q(v)` from the R-type adaptor, by damped Newton with
    /// junction limiting and continuation (a scalar Newton for one
    /// controlling voltage). Any device.
    #[default]
    Newton,
    /// The Scattering Iterative Method: one scalar wave solve per
    /// one-port nonlinearity, iterated through the R-type root. Diodes
    /// only; a sweep cap that ends unconverged is counted in the solve's
    /// `capped`, never hidden.
    Scattering,
}

/// One diode as a wave port of the root.
#[derive(Copy, Clone, Debug)]
pub(super) struct Port {
    junction: Junction,
    /// Anode and cathode pins.
    pub plus: Pin,
    pub minus: Pin,
    /// The global control it reads, and its sign.
    control: (usize, f64),
    pub ohms: f64,
    volts: f64,
    /// The wave it reflects into the root (`v − R·i`).
    wave: f64,
    next: f64,
}

/// Each diode's port resistance over the Thévenin resistance the network
/// shows it. A port much stiffer than its network passes little of its
/// wave to the other ports — two diodes on one node pair couple by about
/// `2/COUPLING` a sweep — while its own reflection, nearly total, is
/// solved with it rather than swept.
const COUPLING: f64 = 100.0;

fn unsupported(reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: "netlist".into(),
        reason: reason.into(),
    }
}

impl Wdf {
    /// Choose how the root's devices are solved; re-adapts and returns
    /// to the operating point. [`NonlinearRoot::Scattering`] is refused
    /// for a netlist with any device but diodes.
    pub fn set_nonlinear_root(&mut self, root: NonlinearRoot) -> Result<(), NetlistError> {
        let mut ports = Vec::new();
        if root == NonlinearRoot::Scattering {
            for d in &self.devices {
                let Device::Diode(junction) = d.device else {
                    return Err(unsupported(
                        "the scattering iterative method solves one-port nonlinearities (diodes); \
                         this netlist has a multi-port device (transistor, tube, op-amp, OTA or \
                         behavioural source): use NonlinearRoot::Newton",
                    ));
                };
                let (g, sign) = d.controls[0];
                let (a, b) = self.controls[g];
                let (plus, minus) = if sign > 0.0 { (a, b) } else { (b, a) };
                ports.push(Port {
                    junction,
                    plus,
                    minus,
                    control: (g, sign),
                    ohms: 1.0,
                    volts: 0.0,
                    wave: 0.0,
                    next: 0.0,
                });
            }
        }
        self.root = root;
        self.scattering = ports;
        self.discretise()?;
        self.reset();
        Ok(())
    }

    /// The root matrix `x` (`n`×`n`, the trees' ports stamped) made the
    /// scattering form's: each diode's port resistance set to the
    /// Thévenin resistance the network shows it, its shunt (DK's
    /// conditioning conductance, part of its device output there) taken
    /// out and its port conductance put in. Returns each port's Norton
    /// column `(pin, ±g)`.
    pub(super) fn scattering_matrix(&mut self, x: &mut [f64]) -> Result<Vec<[(Pin, f64); 2]>, NetlistError> {
        let n = self.n;
        let stamp = |x: &mut [f64], a: Pin, b: Pin, g: f64| {
            for (r, sr) in [(a, 1.0), (b, -1.0)] {
                for (c, sc) in [(a, 1.0), (b, -1.0)] {
                    if let (Some(r), Some(c)) = (r, c) {
                        x[r * n + c] += sr * sc * g;
                    }
                }
            }
        };
        let mut rhs = vec![0.0; n * self.scattering.len().max(1)];
        let cols = self.scattering.len().max(1);
        for (j, port) in self.scattering.iter().enumerate() {
            if let Some(r) = port.plus {
                rhs[r * cols + j] += 1.0;
            }
            if let Some(r) = port.minus {
                rhs[r * cols + j] -= 1.0;
            }
        }
        let w = solve_columns(x, n, &rhs, cols).ok_or(NetlistError::Singular { analysis: "wave digital" })?;
        for (j, port) in self.scattering.iter_mut().enumerate() {
            let ohms = read_col(&w, cols, port.plus, j) - read_col(&w, cols, port.minus, j);
            let ohms = if ohms.is_finite() && ohms > 0.0 { ohms } else { 1.0 / SHUNT };
            port.ohms = COUPLING * ohms;
        }
        let mut columns = Vec::with_capacity(self.scattering.len());
        for port in &self.scattering {
            let g = 1.0 / port.ohms;
            stamp(x, port.plus, port.minus, g - SHUNT);
            columns.push([(port.plus, g), (port.minus, -g)]);
        }
        Ok(columns)
    }

    /// Each port's wave from its voltage (the operating point's on reset).
    pub(super) fn reset_scattering(&mut self) {
        for port in &mut self.scattering {
            let (g, sign) = port.control;
            port.volts = sign * self.work.v[g];
            let (i, _) = port.junction.current_and_conductance(port.volts);
            port.wave = port.volts - port.ohms * i;
        }
    }

    /// One sample's sweeps: `self.p` holds the waves arriving at the
    /// ports from everything but the ports themselves, `self.k` the
    /// root's port-to-port scattering. Leaves the reflected waves in
    /// `self.z[from..]`.
    pub(super) fn scattering_root(&mut self, from: usize) {
        let s = self.settings.newton;
        let nd = self.scattering.len();
        let (mut sweeps, mut largest) = (0u32, f64::INFINITY);
        let mut solved = true;
        while sweeps < s.max_iterations && (largest.is_nan() || largest >= s.tolerance) {
            largest = 0.0;
            for j in 0..nd {
                // The wave arriving from the others (the last sweep's), and
                // the port's own instantaneous reflection `s·a`, which no
                // choice of port resistance removes when nonlinear ports
                // share a node pair: solved with the port, not swept.
                let row = &self.k[j * nd..(j + 1) * nd];
                let own = row[j];
                let others = self.p[j]
                    + row
                        .iter()
                        .zip(&self.scattering)
                        .enumerate()
                        .filter(|&(k, _)| k != j)
                        .map(|(_, (k, q))| k * q.wave)
                        .sum::<f64>();
                let port = &mut self.scattering[j];
                let (junction, ohms) = (port.junction, port.ohms);
                // α = others + s·a, a = 2v − α  ⇒  (1 − s)·v + R·(1 + s)·i(v) = others.
                let result = s.solve_scalar(
                    port.volts,
                    |v| {
                        let (i, g) = junction.current_and_conductance(v);
                        ((1.0 - own) * v + ohms * (1.0 + own) * i - others, (1.0 - own) + ohms * (1.0 + own) * g)
                    },
                    |new, old| junction.limit_step(new, old),
                );
                solved &= result.converged;
                port.volts = result.value;
                let arriving = (others + 2.0 * own * port.volts) / (1.0 + own);
                port.next = 2.0 * port.volts - arriving;
                largest = largest.max((port.next - port.wave).abs());
            }
            for port in &mut self.scattering {
                port.wave = port.next;
            }
            sweeps += 1;
        }
        self.health.record(sweeps, solved && largest < s.tolerance, largest, largest);
        for (z, port) in self.z[from..].iter_mut().zip(&self.scattering) {
            *z = port.wave;
        }
    }
}
