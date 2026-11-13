use super::*;
use crate::circuit::methods::dk::Dk;
use crate::circuit::methods::wdf::Wdf;
use crate::circuit::netlist::{ElementKind, Netlist, Taper, Variable};
use crate::units::Farads;

/// The Yeh–Smith tone stack (the `tone_stack_duncan_fender_mid` deck): treble and mid
/// pots, the bass pot as a rheostat, C3 variable.
const TONE_STACK: &str = "* tone stack
VI i 0 DC 0 AC 1
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

fn tone_stack(end_ohms: f64) -> Netlist {
    let mut n = Netlist::parse_spice(TONE_STACK).unwrap();
    n.add_variable(Variable::pot("treble", ["rt2", "rt1"], Taper::Linear).with_end_ohms(end_ohms))
        .unwrap();
    n.add_variable(Variable::pot("mid", ["rm2", "rm1"], Taper::AUDIO).with_end_ohms(end_ohms))
        .unwrap();
    // The deck's RB sits on a 1 M track.
    n.add_variable(Variable::rheostat("rb", 1e6 - 2.0 * end_ohms, Taper::Linear).with_end_ohms(end_ohms))
        .unwrap();
    n.add_variable(Variable::capacitor("c3")).unwrap();
    n
}

/// The Tube Screamer's clipping stage on its macromodel op-amp, the
/// drive pot an audio-taper rheostat in series with 51k.
fn screamer() -> Netlist {
    let mut n = Netlist::parse_spice(
        "* TS
.param cc=1e-9 vos=0.5e-3 vhi=8 vlo=1 vmid=4.5 vk=0.02 kc=10
.param gm={6.283185307179586*3e6*1e-9} isl={1e6*1e-9} g0={gm/100e3}
V1 in 0 SIN(4.5 0.3 1k)
BG 0 n I = {isl*tanh(gm*(V(in,fb)+vos)/isl) + g0*vmid}
RG n 0 {1/g0}
CC n 0 {cc}
EO out 0 n 0 1
R1 fb s 4.7k
C1 s 0 47n
RF out x 51k
RD x fb 250k
CF out fb 51p
D1 out fb D1N914
D2 fb out D1N914
.model D1N914 D(Is=2.52n Rs=.568 N=1.752)
.control
wrdata out v(out)
.endc",
    )
    .unwrap();
    n.add_variable(Variable::rheostat("rd", 500e3, Taper::AUDIO).with_end_ohms(50.0))
        .unwrap();
    n
}

/// `netlist` with every variable branch at the values `variables` holds.
fn at_values(netlist: &Netlist, variables: &Variables) -> Netlist {
    let mut n = netlist.clone();
    for (b, &value) in variables.branches.iter().zip(&variables.values) {
        match &mut n.element_mut(&b.element).unwrap().kind {
            ElementKind::Resistor { ohms, .. } => *ohms = value,
            ElementKind::Capacitor { farads, .. } => *farads = value,
            _ => unreachable!(),
        }
    }
    n
}

/// The largest difference relative to each matrix's largest entry.
fn worst<'a>(a: impl IntoIterator<Item = &'a [f64]>, b: impl IntoIterator<Item = &'a [f64]>) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| {
            assert_eq!(a.len(), b.len());
            let scale = b.iter().fold(1e-300f64, |m, x| m.max(x.abs()));
            a.iter().zip(b).fold(0.0f64, |m, (x, y)| m.max((x - y).abs() / scale))
        })
        .fold(0.0, f64::max)
}

#[test]
fn tapers_hit_their_midpoints_and_invert() {
    assert!((Taper::AUDIO.fraction(0.5) - 0.1).abs() < 1e-15);
    assert!((Taper::REVERSE_AUDIO.fraction(0.5) - 0.9).abs() < 1e-15);
    assert_eq!(Taper::Linear.fraction(0.3), 0.3);
    for taper in [Taper::Linear, Taper::AUDIO, Taper::REVERSE_AUDIO, Taper::Exponential { midpoint: 0.5 }] {
        assert_eq!((taper.fraction(0.0), taper.fraction(1.0)), (0.0, 1.0));
        for x in [0.0, 0.1, 0.37, 0.5, 0.9, 1.0] {
            assert!((taper.position(taper.fraction(x)) - x).abs() < 1e-12, "{taper:?} at {x}");
        }
    }
}

#[test]
fn a_pot_reads_its_position_back_from_its_legs() {
    let dk = Dk::new(&tone_stack(0.0)).unwrap();
    let treble = dk.pot("treble").unwrap();
    assert_eq!(dk.position(treble), 0.5);
    let mid = dk.pot("mid").unwrap();
    assert!((dk.position(mid) - Taper::AUDIO.position(0.5)).abs() < 1e-12);
    assert!(dk.variable_capacitor("c3").is_ok());
    assert!(dk.pot("c3").is_err(), "a capacitor is not a pot");
    assert!(dk.pot("nothing").is_err());
}

fn moves(dk_or_wdf: &mut impl Knobs) {
    let treble = dk_or_wdf.pot_of("treble");
    let mid = dk_or_wdf.pot_of("mid");
    let bass = dk_or_wdf.pot_of("rb");
    dk_or_wdf.position(treble, 0.93);
    dk_or_wdf.position(mid, 0.12);
    dk_or_wdf.position(bass, 0.71);
    dk_or_wdf.farads("c3", 22e-9);
}

/// What the two methods share, for one test body.
trait Knobs {
    fn pot_of(&self, name: &str) -> Pot;
    fn position(&mut self, pot: Pot, x: f64);
    fn farads(&mut self, name: &str, f: f64);
}

impl Knobs for Dk {
    fn pot_of(&self, name: &str) -> Pot {
        self.pot(name).unwrap()
    }
    fn position(&mut self, pot: Pot, x: f64) {
        self.set_position(pot, x);
    }
    fn farads(&mut self, name: &str, f: f64) {
        let c = self.variable_capacitor(name).unwrap();
        self.set_farads(c, Farads::new(f));
    }
}

impl Knobs for Wdf {
    fn pot_of(&self, name: &str) -> Pot {
        self.pot(name).unwrap()
    }
    fn position(&mut self, pot: Pot, x: f64) {
        self.set_position(pot, x);
    }
    fn farads(&mut self, name: &str, f: f64) {
        let c = self.variable_capacitor(name).unwrap();
        self.set_farads(c, Farads::new(f));
    }
}

#[test]
fn dk_updated_equals_dk_rebuilt() {
    let netlist = tone_stack(0.0);
    let mut dk = Dk::new(&netlist).unwrap();
    dk.drive("vi").unwrap();
    dk.prepare(48_000.0);
    moves(&mut dk);
    dk.settle();
    let mut fresh = Dk::new(&at_values(&netlist, &dk.varying.variables)).unwrap();
    fresh.drive("vi").unwrap();
    fresh.prepare(48_000.0);
    let e = worst(dk.matrices(), fresh.matrices());
    println!("DK tone stack, update vs rebuild: {e:.1e}");
    assert!(e < 1e-11, "{e}");
    // From the operating point (zero here), the outputs agree too.
    dk.reset();
    fresh.reset();
    let mut largest = 0.0f64;
    for n in 0..2_000 {
        let x = (n as f64 * 0.05).sin();
        largest = largest.max((dk.step(x) - fresh.step(x)).abs());
    }
    assert!(largest < 1e-11, "{largest}");
}

#[test]
fn wdf_updated_equals_wdf_rebuilt() {
    let netlist = tone_stack(1.0);
    let mut wdf = Wdf::new(&netlist).unwrap();
    assert!(!wdf.structure().is_tree());
    wdf.drive("vi").unwrap();
    wdf.prepare(48_000.0);
    moves(&mut wdf);
    wdf.settle();
    let mut fresh = Wdf::new(&at_values(&netlist, &wdf.varying.variables)).unwrap();
    fresh.drive("vi").unwrap();
    fresh.prepare(48_000.0);
    let (a, b) = (wdf.matrices(), fresh.matrices());
    let e = worst(a.iter().map(Vec::as_slice), b.iter().map(Vec::as_slice));
    println!("WDF tone stack, update vs rebuild: {e:.1e}");
    assert!(e < 1e-11, "{e}");
    wdf.reset();
    fresh.reset();
    let mut largest = 0.0f64;
    for n in 0..2_000 {
        let x = (n as f64 * 0.05).sin();
        largest = largest.max((wdf.step(x) - fresh.step(x)).abs());
    }
    assert!(largest < 1e-11, "{largest}");
}

#[test]
fn nonlinear_circuits_update_to_their_rebuilds() {
    let netlist = screamer();
    for x in [0.0, 0.25, 0.8, 1.0] {
        let mut dk = Dk::new(&netlist).unwrap();
        let drive = dk.pot("rd").unwrap();
        dk.set_position(drive, x);
        dk.settle();
        let fresh = Dk::new(&at_values(&netlist, &dk.varying.variables)).unwrap();
        let e = worst(dk.matrices(), fresh.matrices());
        println!("DK Tube Screamer drive {x}: {e:.1e}");
        assert!(e < 1e-10, "DK at {x}: {e}");
        let mut wdf = Wdf::new(&netlist).unwrap();
        let drive = wdf.pot("rd").unwrap();
        wdf.set_position(drive, x);
        wdf.settle();
        let fresh = Wdf::new(&at_values(&netlist, &wdf.varying.variables)).unwrap();
        let (a, b) = (wdf.matrices(), fresh.matrices());
        let e = worst(a.iter().map(Vec::as_slice), b.iter().map(Vec::as_slice));
        println!("WDF Tube Screamer drive {x}: {e:.1e}");
        assert!(e < 1e-10, "WDF at {x}: {e}");
    }
}

#[test]
fn prepare_resolves_the_operating_point_a_resistor_moved() {
    // A divider biasing a diode: the pot moves the DC point.
    let text = "* bias
V1 s 0 9
RA s w 50k
RB w 0 50k
R1 w d 10k
D1 d 0 DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(d)
.endc";
    let mut n = Netlist::parse_spice(text).unwrap();
    n.add_variable(Variable::pot("bias", ["rb", "ra"], Taper::Linear)).unwrap();
    let mut dk = Dk::new(&n).unwrap();
    let bias = dk.pot("bias").unwrap();
    dk.set_position(bias, 0.1);
    dk.prepare(48_000.0);
    let fresh = Dk::new(&at_values(&n, &dk.varying.variables)).unwrap();
    assert!((dk.output() - fresh.output()).abs() < 1e-12, "{} vs {}", dk.output(), fresh.output());
    assert!(dk.output() < 0.5, "the diode nearly off: {}", dk.output());
}

#[test]
fn a_glide_refreshes_at_most_once_per_interval() {
    let mut dk = Dk::new(&tone_stack(0.0)).unwrap();
    dk.drive("vi").unwrap();
    dk.prepare(48_000.0);
    dk.set_smoothing(Smoothing {
        time: crate::units::ms(5.0),
        interval: 16,
    });
    let treble = dk.pot("treble").unwrap();
    dk.set_position(treble, 1.0);
    let mut refreshes = 0;
    let mut samples = 0u32;
    while dk.varying.variables.moving() {
        if dk.varying.variables.advance() {
            refreshes += 1;
        }
        samples += 1;
        assert!(samples < 48_000, "the glide arrives");
    }
    assert_eq!(dk.varying.variables.current(treble.0), 1.0);
    assert!(refreshes <= samples.div_ceil(16) + 1, "{refreshes} refreshes in {samples} samples");
    println!("a full-range glide: {samples} samples, {refreshes} refreshes");
}

#[test]
fn a_set_without_smoothing_lands_at_the_next_sample() {
    let mut dk = Dk::new(&tone_stack(0.0)).unwrap();
    dk.drive("vi").unwrap();
    dk.prepare(48_000.0);
    dk.set_smoothing(Smoothing::NONE);
    let treble = dk.pot("treble").unwrap();
    dk.set_position(treble, 0.2);
    dk.step(0.0);
    assert_eq!(dk.position(treble), 0.2);
    assert!(!dk.varying.variables.moving());
    let r = dk.variable_resistor("rb");
    assert!(r.is_err(), "rb is a rheostat: a pot");
}
