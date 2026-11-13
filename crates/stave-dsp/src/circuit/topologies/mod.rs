//! Named circuits. Two kinds live here: hand-derived solutions of a
//! schematic, each its own type (`DiodeClipper`, `CommonCathode` …,
//! checked against `Circuit<Dk>` on the same netlist), and classic
//! circuits as data — a function returning the [`Netlist`] of a named
//! unit, with the values of the drawing it cites, solved by any method:
//!
//! ```
//! use stave_dsp::circuit::methods::dk::Dk;
//! use stave_dsp::circuit::topologies;
//! use stave_dsp::circuit::Circuit;
//!
//! let mut muff = Circuit::<Dk>::new(&topologies::big_muff_rams_head()).unwrap();
//! muff.drive("vin").unwrap();
//! muff.prepare(48_000.0);
//! let peak = (0..2_400)
//!     .map(|n| muff.tick_f64(0.2 * (std::f64::consts::TAU * 220.0 * n as f64 / 48e3).sin()))
//!     .fold(0.0f64, |m, v| m.max(v.abs()));
//! assert!(peak > 0.5, "a Big Muff turns 200 mV into most of a volt: {peak}");
//! ```
//!
//! Each classic circuit's ngspice deck is in `stave-test/spice/`, its
//! verification in `stave-test/tests/classics_match_ngspice.rs`.

pub mod big_muff;
pub mod bucket_brigade;
pub mod common_cathode;
pub mod coupler;
pub mod diode_clipper;
pub mod diode_ladder;
pub mod marshall_jcm800;
pub mod moog_ladder;
pub mod opamp_clipper;
pub mod power_supply;
pub mod proco_rat;
pub mod push_pull;
pub mod tone_stack;
pub mod tone_stacks;
pub mod transformer;
pub mod transistor_ladder;
pub mod triode_stage;
pub mod tube_screamer;

pub use big_muff::{big_muff_rams_head, big_muff_tone_stack};
pub use marshall_jcm800::marshall_jcm800_2203;
pub use moog_ladder::moog_ladder_904a;
pub use proco_rat::proco_rat;
pub use tone_stacks::{fender_5f6a_tone_stack, fender_ab763_tone_stack, marshall_2203_tone_stack};
pub use tube_screamer::tube_screamer_ts808;

use crate::circuit::devices::opamp::{Amp, Macromodel};
use crate::circuit::devices::triode::{dempwolf::Dempwolf, Tube};
use crate::circuit::netlist::{ElementKind, Netlist, Node};

/// A topology's own SPICE text as a netlist. The texts are constants of
/// this module, read by the tests, so a failure here is a bug in stave.
fn netlist_of(text: &str) -> Netlist {
    match Netlist::parse_spice(text) {
        Ok(netlist) => netlist,
        Err(e) => panic!("a topology's netlist does not read: {e}"),
    }
}

fn node(netlist: &Netlist, name: &str) -> Node {
    netlist
        .node(name)
        .unwrap_or_else(|| panic!("a topology names a node it does not have: {name}"))
}

/// An op-amp `name` with inputs `(plus, minus)` and `output`, between
/// `supply` rails.
fn op_amp(
    netlist: &mut Netlist,
    name: &str,
    (plus, minus, output): (&str, &str, &str),
    part: Macromodel,
    supply: (f64, f64),
) {
    let kind = ElementKind::OpAmp {
        plus: node(netlist, plus),
        minus: node(netlist, minus),
        output: node(netlist, output),
        amp: Amp::Macromodel(part),
        supply,
    };
    if let Err(e) = netlist.add(name, kind) {
        panic!("{e}");
    }
}

/// A triode `name` at `(plate, grid, cathode)`.
fn triode(
    netlist: &mut Netlist,
    name: &str,
    (plate, grid, cathode): (&str, &str, &str),
    tube: Dempwolf,
) {
    let kind = ElementKind::Triode {
        plate: node(netlist, plate),
        grid: node(netlist, grid),
        cathode: node(netlist, cathode),
        tube: Tube::Dempwolf(tube),
        // The classic decks write the tube as its static law (DSP.md R11).
        capacitances: crate::circuit::devices::triode::Interelectrode::NONE,
    };
    if let Err(e) = netlist.add(name, kind) {
        panic!("{e}");
    }
}
