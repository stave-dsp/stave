//! The nonlinear elements as the DK solve sees them: each reads some of
//! the global controlling voltages (with a sign, since `v(a, b)` and
//! `v(b, a)` share one unknown) and writes some outputs — currents
//! injected into the network, or a behavioural source's voltage.

use crate::circuit::devices::diode::Shockley;
use crate::circuit::devices::triode::{Triode, Tube};
use crate::circuit::netlist::expr::{Dual, Tape, MAX_CONTROLS};

/// The conductance each junction lends the linear network, taken back
/// out of its device output (`q = i(v) − SHUNT·v`): exact, but a node
/// reached only through junctions (a diode string, a bridge) keeps a
/// well-conditioned MNA matrix instead of a singular one patched by
/// `gmin`, whose `1/gmin` gains would cost the solve its precision.
pub(crate) const SHUNT: f64 = 1e-3;

#[derive(Clone, Debug)]
pub(crate) enum Device {
    /// One control (anode to cathode), one output: its current less the
    /// part `shunt · v` the linear network carries (see [`SHUNT`]).
    Diode(Shockley),
    /// Controls (grid–cathode, plate–cathode); outputs (plate, grid).
    Triode(Tube),
    /// Controls as the tape numbers them; one output.
    Expression { tape: Tape, stack: Vec<Dual> },
}

/// A device wired into the subsystem.
#[derive(Clone, Debug)]
pub(crate) struct Nonlinear {
    pub device: Device,
    /// `(global control, sign)` per local control.
    pub controls: Vec<(usize, f64)>,
    /// The first of its outputs in `q`.
    pub output: usize,
}

impl Nonlinear {
    #[inline]
    fn local(&self, v: &[f64], i: usize) -> f64 {
        let (g, sign) = self.controls[i];
        sign * v[g]
    }

    /// Write outputs into `q` and add their partials into `jq`
    /// (`nq`×`nv`, zeroed by the caller).
    pub fn evaluate(&mut self, v: &[f64], q: &mut [f64], jq: &mut [f64], nv: usize, time: f64) {
        let o = self.output;
        match &mut self.device {
            Device::Diode(d) => {
                let (g, sign) = self.controls[0];
                let volts = sign * v[g];
                let (i, slope) = d.current_and_conductance(volts);
                q[o] = i - SHUNT * volts;
                jq[o * nv + g] += (slope - SHUNT) * sign;
            }
            Device::Triode(t) => {
                let (vgk, vpk) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let c = t.currents(vgk, vpk);
                let ((gg, sg), (gp, sp)) = (self.controls[0], self.controls[1]);
                q[o] = c.plate;
                q[o + 1] = c.grid;
                jq[o * nv + gg] += c.plate_per_grid_volt * sg;
                jq[o * nv + gp] += c.plate_per_plate_volt * sp;
                jq[(o + 1) * nv + gg] += c.grid_per_grid_volt * sg;
            }
            Device::Expression { tape, stack } => {
                let mut local = [0.0; MAX_CONTROLS];
                for (i, &(g, sign)) in self.controls.iter().enumerate() {
                    local[i] = sign * v[g];
                }
                let d = tape.eval(&local, time, stack);
                q[o] = d.value;
                for (i, &(g, sign)) in self.controls.iter().enumerate() {
                    jq[o * nv + g] += d.slope[i] * sign;
                }
            }
        }
    }

    /// Bound a Newton step from `old` to `new` (a junction's `pnjlim`).
    pub fn limit(&self, new: &mut [f64], old: &[f64]) {
        if let Device::Diode(d) = &self.device {
            let (g, sign) = self.controls[0];
            let limited = d.limit_step(sign * new[g], self.local(old, 0));
            new[g] = sign * limited;
        }
    }
}

#[inline]
fn sign_read(controls: &[(usize, f64)], v: &[f64], i: usize) -> f64 {
    let (g, sign) = controls[i];
    sign * v[g]
}
