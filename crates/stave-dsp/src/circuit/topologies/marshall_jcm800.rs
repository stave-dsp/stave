//! The Marshall JCM800 2203 preamp as a netlist.

use super::{netlist_of, triode};
use crate::circuit::devices::triode::dempwolf::Dempwolf;
use crate::circuit::netlist::Netlist;

/// The JCM800 2203 preamp, high input to the master volume: source `vin`
/// at node `in` (DC 0, drive it), output `o` (the treble wiper, loaded by
/// the master volume full up). Gain and tone pots at a resistance
/// fraction of 0.5.
///
/// # Models
/// The 2203's four-triode preamp: V1B (68k stopper, 1M input leak, 2k7 ‖
/// 680n cathode, 100k plate); 22n into the 470k ‖ 470p treble peaker and
/// the 1M gain pot with its 1n bright cap; V1A, the cold clipper (10k
/// cathode unbypassed, biased near cutoff); 22n into 470k ‖ 470p and
/// V2A's 470k grid leak; V2A (820 Ω unbypassed, 100k plate) DC-coupled to
/// the V2B cathode follower (100k cathode); the 470p / 22n / 22n / 33k
/// tone stack and the 1M master volume. Every triode is
/// [`Dempwolf::RSD_1`], a measured 12AX7 whose one smooth law covers
/// cutoff, plate current and grid current.
///
/// # Derivation
/// Transcribed from Marshall drawing 2203-01-60-02, checked against Rob
/// Robinette's "How the Marshall JCM800 Works", whose node voltages give
/// the ideal supplies (335 V for V1, 350 V for V2, behind R14 and R15).
/// Interelectrode capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::marshall_jcm800_2203_matches_ngspice`:
/// this netlist (stave's triode elements) through `Circuit<Dk>` against
/// ngspice on the deck `marshall_jcm800_2203.cir` (0.5 V at 1 kHz: V1A
/// cuts off and V2A's grid conducts every cycle; the triodes written as
/// behavioural sources), 2e-4 normalized RMS at 16× (measured 1.0e-4),
/// converging as h² from 8×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> JCM800 2203 preamp"`.
///
/// # Aliasing
/// Triode cutoff and grid conduction: run it oversampled.
///
/// # Valid ranges
/// Guitar-level inputs and well past (tested at the host rate on a 3 V
/// hot signal).
pub fn marshall_jcm800_2203() -> Netlist {
    let mut netlist = netlist_of(
        "Marshall JCM800 2203 preamp
VIN in 0 0
VB5 b5 0 335
VB4 b4 0 350
R45 in 0 1meg
R47 in g1 68k
R7 b5 p1 100k
R1 k1 0 2.7k
C1 k1 0 680n
C3 p1 a1 22n
R46 a1 gt 470k
C26 a1 gt 470p
RGT gt g2 500k
RGB g2 0 500k
C27 gt g2 1n
R8 b5 p2 100k
R2 k2 0 10k
C4 p2 a2 22n
R9 a2 g3 470k
C6 a2 g3 470p
R10 g3 0 470k
R44 b4 p3 100k
R11 k3 0 820
R12 k4 0 100k
C8 k4 t 470p
RT1 t o 125k
RT2 o bt 125k
R13 k4 n 33k
C9 n bt 22n
RB bt j 500k
RM1 j w 12.5k
RM2 w 0 12.5k
C10 n w 22n
RMV o 0 1meg
.control
wrdata out v(o)
.endc",
    );
    let tube = Dempwolf::RSD_1;
    triode(&mut netlist, "v1b", ("p1", "g1", "k1"), tube);
    triode(&mut netlist, "v1a", ("p2", "g2", "k2"), tube);
    triode(&mut netlist, "v2a", ("p3", "g3", "k3"), tube);
    triode(&mut netlist, "v2b", ("b4", "p3", "k4"), tube);
    netlist
}
