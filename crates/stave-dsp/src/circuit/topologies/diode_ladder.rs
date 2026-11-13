//! The diode ladder filter (the TB-303's core, as Stinchcombe analyses
//! it) as a netlist: a driven pair on a tail current, three rungs of
//! junction pairs and a top pair of transistors, a capacitor across each
//! rung and half of one across the driven pair. Data, not code: solve it
//! with [`Circuit<Dk>`](crate::circuit::Circuit). The fast structural
//! model of the same circuit is
//! [`primitives::filters::ladder::Stinchcombe`](crate::primitives::filters::ladder::Stinchcombe),
//! held to this netlist.

use super::transistor_ladder::add;
use crate::circuit::devices::bjt::GummelPoon;
use crate::circuit::devices::diode::{Shockley, THERMAL_VOLTS_27C};
use crate::circuit::netlist::{ElementKind, Netlist, Node, Probe, Source};

/// The diode ladder's parts: what [`netlist`](Self::netlist) builds.
///
/// # Models
/// The TB-303's diode ladder core as Tim Stinchcombe analyses it
/// (*Analysis of the Moog transistor ladder and derivative filters*,
/// 2008): a driven differential pair on an ideal tail current, three
/// rungs of junction pairs, and a top pair of transistors with their
/// bases on one bias (acting as the fourth rung's junctions); a
/// capacitor across each rung, half that value across the driven pair's
/// collectors. The output is `v(r3, l3)` across the top capacitor; the
/// resonance is that voltage times `resonance` on the driven pair's
/// second base. Biasing is idealised (DC sources, an ideal tail and
/// buffer). The junctions are a diode card of the caller's choosing (by
/// default the 2N3904's base–emitter law, `IS` and `N = 1`): the 303's
/// own diodes are not modelled.
///
/// # Derivation
/// The schematic as a netlist, solved by
/// [`Circuit<Dk>`](crate::circuit::Circuit). With ideal junctions every
/// rung passes `(I/2)·tanh(Δv / 2Vt)` and the small-signal response is
/// Stinchcombe's `2 / (s⁴ + 8s³ + 20s² + 16s + 2)`, `s` in units of
/// `I / (4·Vt·C)`: it self-oscillates at resonance 17, at `√2` times
/// that frequency ([`ideal_pitch_hz`](Self::ideal_pitch_hz)).
///
/// # Verified
/// `crates/stave-test/tests/filters_match_ngspice.rs::diode_ladder_matches_ngspice`:
/// the deck `spice/diode_ladder.cir` (resonance 10, 40 mV at 300 Hz)
/// and this netlist driven by the same sine through `Circuit<Dk>` at
/// 4×, within 2e-4 normalized RMS of ngspice (measured 2.6e-5); and
/// `crates/stave-test/tests/filters_match_ngspice.rs::diode_ladder_ac_matches_ngspice`:
/// the small-signal response within 3e-5 dB of ngspice's AC analysis,
/// self-oscillation at resonance 17.00 and 1433 Hz (ideal 1450).
///
/// # Cost
/// Datasheet `"Circuit<Dk> diode ladder"` at 48 kHz: four Gummel–Poon
/// devices and six junctions in Newton.
///
/// # Aliasing
/// None of its own; run it oversampled.
///
/// # Valid ranges
/// Tail current 1 µA–10 mA (the driven pair keeps its collectors above
/// its emitters with the top bias at 4 V), resonance 0 to past
/// self-oscillation (about 17), differential input within a few hundred
/// millivolts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiodeLadder {
    /// The tail current that sets the cutoff (A).
    pub tail_amps: f64,
    /// The capacitor across each rung (F); the driven pair's is half.
    pub capacitor_farads: f64,
    /// The feedback gain from the output to the driven pair: 0 is a
    /// plain four-pole lowpass, about 17 self-oscillates.
    pub resonance: f64,
    /// The driven pair and the top pair.
    pub transistor: GummelPoon,
    /// The six rung junctions.
    pub junction: Shockley,
}

impl DiodeLadder {
    /// The 2N3904's base–emitter junction as a diode (`IS = 6.734f`,
    /// `N = 1`): the deck's `DQ`.
    pub const JUNCTION: Shockley = Shockley {
        saturation_amps: 6.734e-15,
        emission: 1.0,
        thermal_volts: THERMAL_VOLTS_27C,
        series_ohms: 0.0,
        breakdown: None,
        capacitance: crate::circuit::devices::capacitance::Depletion::NONE,
        transit_seconds: 0.0,
    };

    /// The deck `stave-test/spice/diode_ladder.cir` without its
    /// resonance: 100 µA, 150 nF rungs (75 nF at the bottom), 2N3904s.
    pub const DEFAULT: Self = Self {
        tail_amps: 100e-6,
        capacitor_farads: 150e-9,
        resonance: 0.0,
        // The decks' 2N3904 card is its DC part (they predate device
        // capacitances, DSP.md R11).
        transistor: GummelPoon::N2N3904.without_charge(),
        junction: Self::JUNCTION,
    };

    /// The ideal ladder's self-oscillation pitch, `√2 · I / (8π·Vt·C)`
    /// at 27 °C.
    pub fn ideal_pitch_hz(&self) -> f64 {
        core::f64::consts::SQRT_2 * self.tail_amps
            / (8.0 * core::f64::consts::PI * THERMAL_VOLTS_27C * self.capacitor_farads)
    }

    /// The netlist: input `vin` (0 V at rest, AC magnitude 1), the probe
    /// across the top capacitor. Node and element names are the deck's.
    pub fn netlist(&self) -> Netlist {
        let mut n = Netlist::new();
        n.title = "diode ladder".into();
        let ground = Node::GROUND;
        let (vin, fb, tail, vee, vcc, bt) = (
            n.add_node("in"),
            n.add_node("fb"),
            n.add_node("tail"),
            n.add_node("vee"),
            n.add_node("vcc"),
            n.add_node("bt"),
        );
        let q = |c, b, e| ElementKind::Bjt {
            collector: c,
            base: b,
            emitter: e,
            substrate: Node::GROUND,
            bjt: self.transistor,
        };
        let source = |a, volts| ElementKind::VoltageSource {
            a,
            b: ground,
            source: Source::dc(volts),
        };
        add(
            &mut n,
            "vin",
            ElementKind::VoltageSource {
                a: vin,
                b: ground,
                source: Source {
                    ac: 1.0,
                    ..Source::dc(0.0)
                },
            },
        );
        let levels: Vec<_> = (0..4)
            .map(|k| (n.add_node(&format!("l{k}")), n.add_node(&format!("r{k}"))))
            .collect();
        add(&mut n, "q1", q(levels[0].0, vin, tail));
        add(&mut n, "q2", q(levels[0].1, fb, tail));
        add(
            &mut n,
            "itail",
            ElementKind::CurrentSource {
                a: tail,
                b: vee,
                source: Source::dc(self.tail_amps),
            },
        );
        for (k, &(l, r)) in levels.iter().enumerate() {
            let farads = if k == 0 {
                0.5 * self.capacitor_farads
            } else {
                self.capacitor_farads
            };
            add(
                &mut n,
                &format!("c{k}"),
                ElementKind::Capacitor {
                    a: l,
                    b: r,
                    farads,
                    initial_volts: None,
                },
            );
            if k > 0 {
                let (below_l, below_r) = levels[k - 1];
                for (name, anode, cathode) in [(2 * k - 1, l, below_l), (2 * k, r, below_r)] {
                    add(
                        &mut n,
                        &format!("d{name}"),
                        ElementKind::Diode {
                            anode,
                            cathode,
                            diode: self.junction,
                        },
                    );
                }
            }
        }
        let (top_l, top_r) = levels[3];
        add(&mut n, "q3", q(vcc, bt, top_l));
        add(&mut n, "q4", q(vcc, bt, top_r));
        add(&mut n, "vbt", source(bt, 4.0));
        add(&mut n, "vcc", source(vcc, 15.0));
        add(&mut n, "vee", source(vee, -15.0));
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

impl Default for DiodeLadder {
    fn default() -> Self {
        Self::DEFAULT
    }
}
