//! The nonlinear elements as the DK solve sees them: each reads some of
//! the global controlling voltages (with a sign, since `v(a, b)` and
//! `v(b, a)` share one unknown) and writes some outputs — currents
//! injected into the network, or a behavioural source's voltage.

use crate::circuit::devices::diode::Junction;
use crate::circuit::devices::opamp::Prepared;
use crate::circuit::devices::ota::{BipolarPair, Ota};
use crate::circuit::devices::pentode::{koren::Koren as PentodeKoren, Pentode};
use crate::circuit::devices::bjt::{Bjt, GummelPoon};
use crate::circuit::devices::jfet::{self, Jfet};
use crate::circuit::devices::mosfet::{self, Mosfet};
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
    Diode(Junction),
    /// Controls (grid–cathode, plate–cathode); outputs (plate, grid).
    Triode(Tube),
    /// Controls (grid–cathode, screen–cathode, plate–cathode); outputs
    /// (plate, screen).
    Pentode(PentodeKoren),
    /// A macromodel op-amp's gain stage. Controls (`v(+, −)`, the
    /// internal node); one output, the current into the internal node.
    OpAmp(Prepared),
    /// Controls (`v(+, −)`, the bias junction); outputs (the current into
    /// the output node, the bias junction's current less its shunt).
    Ota(BipolarPair),
    /// Controls (base–emitter, base–collector), each junction shunted
    /// like a diode's; outputs (collector, base).
    Bjt(GummelPoon),
    /// Controls (gate–source, gate–drain), the gate junctions shunted;
    /// outputs (drain, gate).
    Jfet(jfet::ShichmanHodges),
    /// Controls (gate–source, drain–source[, bulk–source]); output the
    /// drain current. `bulk` is false with the bulk tied to the source.
    Mosfet { mosfet: mosfet::ShichmanHodges, bulk: bool },
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
            Device::Pentode(t) => {
                let (vg, vs, vp) = (
                    sign_read(&self.controls, v, 0),
                    sign_read(&self.controls, v, 1),
                    sign_read(&self.controls, v, 2),
                );
                let c = t.currents(vg, vs, vp);
                let ((gg, sg), (gs, ss), (gp, sp)) = (self.controls[0], self.controls[1], self.controls[2]);
                q[o] = c.plate;
                q[o + 1] = c.screen;
                jq[o * nv + gg] += c.plate_per_grid_volt * sg;
                jq[o * nv + gs] += c.plate_per_screen_volt * ss;
                jq[o * nv + gp] += c.plate_per_plate_volt * sp;
                jq[(o + 1) * nv + gg] += c.screen_per_grid_volt * sg;
                jq[(o + 1) * nv + gs] += c.screen_per_screen_volt * ss;
            }
            Device::OpAmp(a) => {
                let (vd, vn) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let c = a.current(vd, vn);
                let ((gd, sd), (gn, sn)) = (self.controls[0], self.controls[1]);
                q[o] = c.amps;
                jq[o * nv + gd] += c.per_input_volt * sd;
                jq[o * nv + gn] += c.per_node_volt * sn;
            }
            Device::Ota(a) => {
                let (vd, vb) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let c = a.currents(vd, vb);
                let ((gd, sd), (gb, sb)) = (self.controls[0], self.controls[1]);
                q[o] = c.output;
                q[o + 1] = c.bias - SHUNT * vb;
                jq[o * nv + gd] += c.output_per_input_volt * sd;
                jq[o * nv + gb] += c.output_per_bias_volt * sb;
                jq[(o + 1) * nv + gb] += (c.bias_per_bias_volt - SHUNT) * sb;
            }
            Device::Bjt(t) => {
                let (vbe, vbc) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let c = t.currents(vbe, vbc);
                let ((ge, se), (gc, sc)) = (self.controls[0], self.controls[1]);
                q[o] = c.collector + SHUNT * vbc;
                q[o + 1] = c.base - SHUNT * (vbe + vbc);
                jq[o * nv + ge] += c.collector_per_vbe * se;
                jq[o * nv + gc] += (c.collector_per_vbc + SHUNT) * sc;
                jq[(o + 1) * nv + ge] += (c.base_per_vbe - SHUNT) * se;
                jq[(o + 1) * nv + gc] += (c.base_per_vbc - SHUNT) * sc;
            }
            Device::Jfet(t) => {
                let (vgs, vgd) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let c = t.currents(vgs, vgd);
                let ((gs, ss), (gd, sd)) = (self.controls[0], self.controls[1]);
                q[o] = c.drain + SHUNT * vgd;
                q[o + 1] = c.gate - SHUNT * (vgs + vgd);
                jq[o * nv + gs] += c.drain_per_vgs * ss;
                jq[o * nv + gd] += (c.drain_per_vgd + SHUNT) * sd;
                jq[(o + 1) * nv + gs] += (c.gate_per_vgs - SHUNT) * ss;
                jq[(o + 1) * nv + gd] += (c.gate_per_vgd - SHUNT) * sd;
            }
            Device::Mosfet { mosfet, bulk } => {
                let (vgs, vds) = (sign_read(&self.controls, v, 0), sign_read(&self.controls, v, 1));
                let vbs = if *bulk { sign_read(&self.controls, v, 2) } else { 0.0 };
                let c = mosfet.currents(vgs, vds, vbs);
                let ((gg, sg), (gd, sd)) = (self.controls[0], self.controls[1]);
                q[o] = c.drain;
                jq[o * nv + gg] += c.drain_per_vgs * sg;
                jq[o * nv + gd] += c.drain_per_vds * sd;
                if *bulk {
                    let (gb, sb) = self.controls[2];
                    jq[o * nv + gb] += c.drain_per_vbs * sb;
                }
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

    /// Whether [`closed_form_mean`](Self::closed_form_mean) knows this
    /// device: a one-output law of one control with a closed-form
    /// antiderivative (stateful ADAA's one-port, `dk::adaa`).
    pub fn has_closed_form_mean(&self) -> bool {
        matches!(self.device, Device::Diode(_))
    }

    /// `(mean, secant)` of the device's output over the straight line
    /// from `v0` to `v1` of its one global control: the divided
    /// differences of its antiderivative and of itself, in closed form.
    pub fn closed_form_mean(&self, v0: f64, v1: f64) -> (f64, f64) {
        match &self.device {
            Device::Diode(d) => {
                let (_, sign) = self.controls[0];
                let (a, b) = (sign * v0, sign * v1);
                let mean = d.mean_current(a, b) - SHUNT * 0.5 * (a + b);
                (mean, sign * (d.secant(a, b) - SHUNT))
            }
            _ => (0.0, 0.0),
        }
    }

    /// Bound a Newton step from `old` to `new` (a junction's `pnjlim`).
    pub fn limit(&self, new: &mut [f64], old: &[f64]) {
        let pair = |limit: &dyn Fn([f64; 2], [f64; 2]) -> [f64; 2], new: &mut [f64]| {
            let ((a, sa), (b, sb)) = (self.controls[0], self.controls[1]);
            let l = limit([sa * new[a], sb * new[b]], [self.local(old, 0), self.local(old, 1)]);
            new[a] = sa * l[0];
            new[b] = sb * l[1];
        };
        match &self.device {
            Device::Diode(d) => {
                let (g, sign) = self.controls[0];
                let limited = d.limit_step(sign * new[g], self.local(old, 0));
                new[g] = sign * limited;
            }
            Device::Ota(a) => {
                let (g, sign) = self.controls[1];
                let limited = a.limit_bias_step(sign * new[g], self.local(old, 1));
                new[g] = sign * limited;
            }
            Device::Bjt(t) => pair(&|n, o| t.limit_step(n, o), new),
            Device::Jfet(t) => pair(&|n, o| t.limit_step(n, o), new),
            _ => {}
        }
    }
}

#[inline]
fn sign_read(controls: &[(usize, f64)], v: &[f64], i: usize) -> f64 {
    let (g, sign) = controls[i];
    sign * v[g]
}
