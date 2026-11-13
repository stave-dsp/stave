//! N6's datasheet lines: what a knob move costs a running circuit (a
//! low-rank update of its prepared matrices, here on every sample) beside
//! the circuit untouched and beside what a move cost before N6 — a
//! rebuild (`Circuit::new`, `drive`, `prepare`), here once per 64-sample
//! block. Per move: the moved line less the untouched one; per rebuild:
//! 64 × (the rebuilt line less the untouched one).

use super::sheet::bench;
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::netlist::{ElementKind, Netlist, Taper, Variable};
use stave_dsp::circuit::{Circuit, Method, Pot, Smoothing};
use stave_dsp::Stage;

const TONE_STACK: &str = "tone stack, pots at noon
V1 i 0 0
C1 i t 250p
RT1 t o 125000
RT2 o b 125000
R4 i n 100000
C2 n b 100n
RB b j 500000
RM1 j w 12500
RM2 w 0 12500
C3 n w 47n
.control
wrdata out v(o)
.endc";

/// The tone stack, its treble pot declared (1 mΩ ends, for the WDF).
fn tone_stack() -> Netlist {
    let mut n = Netlist::parse_spice(TONE_STACK).unwrap();
    n.add_variable(Variable::pot("treble", ["rt2", "rt1"], Taper::Linear).with_end_ohms(1e-3))
        .unwrap();
    n
}

/// The Tube Screamer drive deck at 0.5, its drive pot declared.
fn screamer() -> Netlist {
    let mut n = Netlist::parse_spice(include_str!("../../../stave-test/spice/tube_screamer_drive_50.cir")).unwrap();
    n.add_variable(Variable::rheostat("rd", 500e3, Taper::AUDIO).with_end_ohms(50.0))
        .unwrap();
    n
}

/// What the two methods share here.
trait Knob: Stage {
    fn knob(&mut self, name: &str) -> Pot;
    fn turn(&mut self, pot: Pot, position: f64);
}

macro_rules! knob {
    ($m:ty) => {
        impl Knob for Circuit<$m> {
            fn knob(&mut self, name: &str) -> Pot {
                self.drive("v1").unwrap();
                self.set_smoothing(Smoothing::NONE);
                self.pot(name).unwrap()
            }
            fn turn(&mut self, pot: Pot, position: f64) {
                self.set_position(pot, position);
            }
        }
    };
}
knob!(Dk);
knob!(Wdf);

fn built<M: Method>(netlist: &Netlist, pot: &str, sr: f32) -> (Circuit<M>, Pot)
where
    Circuit<M>: Knob,
{
    let mut c = Circuit::<M>::new(netlist).unwrap();
    let pot = c.knob(pot);
    c.prepare(sr);
    (c, pot)
}

/// `netlist` with resistor `element` at `ohms`.
fn with_ohms(netlist: &Netlist, element: &str, ohms: f64) -> Netlist {
    let mut n = netlist.clone();
    if let ElementKind::Resistor { ohms: r, .. } = &mut n.element_mut(element).unwrap().kind {
        *r = ohms;
    }
    n
}

fn lines<M: Method>(
    label: &str,
    netlist: &Netlist,
    pot: &str,
    offset: f32,
    leg: (&str, impl Fn(f64) -> f64),
    sr: f32,
    input: &[f32],
) where
    Circuit<M>: Knob,
{
    let (mut still, _) = built::<M>(netlist, pot, sr);
    bench(label, input, |x| still.tick(offset + x));
    let (mut moved, knob) = built::<M>(netlist, pot, sr);
    bench(&format!("{label}, pot moved every sample"), input, |x| {
        moved.turn(knob, 0.5 + f64::from(x));
        moved.tick(offset + x)
    });
    let (mut rebuilt, _) = built::<M>(netlist, pot, sr);
    let mut n = 0usize;
    bench(&format!("{label}, rebuilt every 64 samples"), input, |x| {
        if n % 64 == 0 {
            let position = 0.5 + f64::from(x);
            rebuilt = built::<M>(&with_ohms(netlist, leg.0, leg.1(position)), pot, sr).0;
        }
        n += 1;
        rebuilt.tick(offset + x)
    });
}

pub fn datasheet(sr: f32, input: &[f32]) {
    println!("  knob moves (N6): untouched, a low-rank update every sample, a rebuild every 64");
    let stack = tone_stack();
    let treble_leg = ("rt2", |x: f64| 1e-3 + x * (250e3 - 2e-3));
    lines::<Dk>("Circuit<Dk> tone stack, treble pot", &stack, "treble", 0.0, treble_leg, sr, input);
    lines::<Wdf>("Circuit<Wdf> tone stack, treble pot", &stack, "treble", 0.0, treble_leg, sr, input);
    let screamer = screamer();
    let drive_leg = ("rd", |x: f64| 50.0 + 500e3 * Taper::AUDIO.fraction(x));
    lines::<Dk>("Circuit<Dk> Tube Screamer, drive pot", &screamer, "rd", 4.5, drive_leg, sr, input);
    lines::<Wdf>("Circuit<Wdf> Tube Screamer, drive pot", &screamer, "rd", 4.5, drive_leg, sr, input);
}
