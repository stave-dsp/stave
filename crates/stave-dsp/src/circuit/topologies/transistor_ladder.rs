//! The transistor ladder filter (Moog, US 3,475,623) as a netlist: a
//! driven pair on a tail current, four rungs of transistor pairs with a
//! capacitor across each rung's emitters, and the resonance fed back
//! from the output to the driven pair. Data, not code: solve it with
//! [`Circuit<Dk>`](crate::circuit::Circuit). The fast structural model
//! of the same circuit is
//! [`primitives::filters::ladder::Huovilainen`](crate::primitives::filters::ladder::Huovilainen),
//! held to this netlist.

use crate::circuit::devices::bjt::GummelPoon;
use crate::circuit::devices::diode::THERMAL_VOLTS_27C;
use crate::circuit::netlist::{ElementKind, Netlist, Node, Probe, Source};

/// The transistor ladder's parts: what [`netlist`](Self::netlist) builds.
///
/// # Models
/// The Moog transistor ladder's core (US patent 3,475,623): a driven
/// differential pair on an ideal tail current, four rungs of pairs whose
/// bases sit on 1.5 V steps, a capacitor across each rung's emitters,
/// the top rung's collectors on the supply. The output is the voltage
/// across the top capacitor, `v(c4r, c4l)`; the resonance is that
/// voltage times `resonance` on the driven pair's second base. The
/// biasing is idealised — DC sources for the rung bases and an ideal
/// tail, the resonance buffer an ideal controlled source — so what
/// remains is the ladder itself.
///
/// # Derivation
/// The schematic as a netlist, every transistor a Gummel–Poon device
/// ([`GummelPoon::N2N3904`] by default), solved by
/// [`Circuit<Dk>`](crate::circuit::Circuit). With ideal transistors it
/// reduces to Huovilainen's four `tanh` stages with corner
/// `I / (8π·Vt·C)` ([`ideal_cutoff_hz`](Self::ideal_cutoff_hz)).
///
/// # Verified
/// `crates/stave-test/tests/filters_match_ngspice.rs::transistor_ladder_matches_ngspice`:
/// the deck `spice/transistor_ladder.cir` (resonance 3, 40 mV at
/// 300 Hz) and this netlist driven by the same sine through
/// `Circuit<Dk>` at 4×, within 2e-4 normalized RMS of ngspice (measured
/// 6.7e-5, falling as h² to 8×); and
/// `crates/stave-test/tests/filters_match_ngspice.rs::transistor_ladder_ac_matches_ngspice`:
/// the small-signal response within 3e-5 dB of ngspice's AC analysis,
/// self-oscillation at resonance 3.98 (ideal 4) and 994 Hz (ideal
/// 1026: the 2N3904's base current loses about 1 % a rung).
///
/// # Cost
/// Datasheet `"Circuit<Dk> transistor ladder"` at 48 kHz: ten
/// Gummel–Poon devices, twenty controlling voltages in Newton.
///
/// # Aliasing
/// None of its own: the pair's `tanh` aliases as a memoryless curve
/// would, softened by the poles after it; run it oversampled.
///
/// # Valid ranges
/// Tail current 1 µA–10 mA (the rungs stay forward active on the
/// 1.5 V steps), resonance 0 to past self-oscillation (about 4),
/// differential input within a few hundred millivolts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransistorLadder {
    /// The tail current that sets the cutoff (A).
    pub tail_amps: f64,
    /// The capacitor across each rung (F).
    pub capacitor_farads: f64,
    /// The feedback gain from the output to the driven pair: 0 is a
    /// plain four-pole lowpass, about 4 self-oscillates.
    pub resonance: f64,
    /// Every one of the ten transistors.
    pub transistor: GummelPoon,
}

impl TransistorLadder {
    /// The deck `stave-test/spice/transistor_ladder.cir` without its
    /// resonance: 100 µA, 150 nF (1026 Hz ideally), 2N3904s.
    pub const DEFAULT: Self = Self {
        tail_amps: 100e-6,
        capacitor_farads: 150e-9,
        resonance: 0.0,
        transistor: GummelPoon::N2N3904,
    };

    /// The cutoff of the ideal ladder, `I / (8π·Vt·C)` at 27 °C: what
    /// the circuit's beta, base currents and Early effect move a little.
    pub fn ideal_cutoff_hz(&self) -> f64 {
        self.tail_amps / (8.0 * core::f64::consts::PI * THERMAL_VOLTS_27C * self.capacitor_farads)
    }

    /// The netlist: input `vin` (0 V at rest, AC magnitude 1, for
    /// [`Dk::drive`](crate::circuit::methods::dk::Dk::drive) and
    /// [`Dk::analog_response`](crate::circuit::methods::dk::Dk::analog_response)),
    /// the probe across the top capacitor. Node and element names are the
    /// deck's.
    pub fn netlist(&self) -> Netlist {
        let mut n = Netlist::new();
        n.title = "transistor ladder".into();
        let (vin, fb, tail, vee, vcc) = (
            n.add_node("in"),
            n.add_node("fb"),
            n.add_node("tail"),
            n.add_node("vee"),
            n.add_node("vcc"),
        );
        let q = |c, b, e| ElementKind::Bjt {
            collector: c,
            base: b,
            emitter: e,
            bjt: self.transistor,
        };
        add(
            &mut n,
            "vin",
            ElementKind::VoltageSource {
                a: vin,
                b: Node::GROUND,
                source: Source {
                    ac: 1.0,
                    ..Source::dc(0.0)
                },
            },
        );
        // Collector pairs c1..c4: c1 the driven pair's, ck the (k−1)th rung's.
        let pairs: Vec<_> = (1..=4)
            .map(|k| (n.add_node(&format!("c{k}l")), n.add_node(&format!("c{k}r"))))
            .collect();
        add(&mut n, "q1", q(pairs[0].0, vin, tail));
        add(&mut n, "q2", q(pairs[0].1, fb, tail));
        add(
            &mut n,
            "itail",
            ElementKind::CurrentSource {
                a: tail,
                b: vee,
                source: Source::dc(self.tail_amps),
            },
        );
        for k in 0..4 {
            let (l, r) = pairs[k];
            add(
                &mut n,
                &format!("c{}", k + 1),
                ElementKind::Capacitor {
                    a: l,
                    b: r,
                    farads: self.capacitor_farads,
                    initial_volts: None,
                },
            );
            let base = n.add_node(&format!("b{}", k + 1));
            let (cl, cr) = pairs.get(k + 1).copied().unwrap_or((vcc, vcc));
            add(&mut n, &format!("q{}", 2 * k + 3), q(cl, base, l));
            add(&mut n, &format!("q{}", 2 * k + 4), q(cr, base, r));
            add(
                &mut n,
                &format!("vb{}", k + 1),
                ElementKind::VoltageSource {
                    a: base,
                    b: Node::GROUND,
                    source: Source::dc(1.5 * (k + 1) as f64),
                },
            );
        }
        let ground = Node::GROUND;
        add(
            &mut n,
            "vcc",
            ElementKind::VoltageSource {
                a: vcc,
                b: ground,
                source: Source::dc(15.0),
            },
        );
        add(
            &mut n,
            "vee",
            ElementKind::VoltageSource {
                a: vee,
                b: ground,
                source: Source::dc(-15.0),
            },
        );
        let (top_l, top_r) = pairs[3];
        add(
            &mut n,
            "efb",
            ElementKind::VoltageControlledVoltage {
                a: fb,
                b: ground,
                control: (top_r, top_l),
                gain: self.resonance,
            },
        );
        n.probe = Some(Probe {
            plus: top_r,
            minus: top_l,
        });
        n
    }
}

/// Add an element whose name the builder knows is new.
pub(super) fn add(n: &mut Netlist, name: &str, kind: ElementKind) {
    n.add(name, kind).expect("the builder's names are unique");
}

impl Default for TransistorLadder {
    fn default() -> Self {
        Self::DEFAULT
    }
}
