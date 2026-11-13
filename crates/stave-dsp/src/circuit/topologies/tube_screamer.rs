//! The Ibanez TS-808 Tube Screamer as a netlist.

use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The TS-808 Tube Screamer, input jack to output: source `vin` at node
/// `in` (DC 0, drive it; the pedal biases itself at 4.5 V), output `out`.
/// Drive and level at maximum, tone at noon.
///
/// # Models
/// The whole effect path of the 1979 TS-808: the 2N5088 input follower;
/// the JRC4558 clipping stage (51k + 500k ‖ 51p ‖ an antiparallel 1N914
/// pair in feedback, 4k7 + 47n to the reference); the tone stage (1k /
/// 220n, 10k to the reference, the 20k tone pot between the second
/// half's inputs with 220n + 220 Ω from its wiper, 1k feedback); the
/// level pot; the 2N5088 output follower (100 Ω, 1u, 10k); the 2M2
/// input pulldown. Both op-amp halves are [`Macromodel::JRC4558`] on
/// 0–9 V; [`GummelPoon::N2N5088`]; [`Shockley::N914`].
///
/// # Derivation
/// Transcribed from Aion FX's "Stratus Classic Overdrive" schematic (p. 6,
/// TS-808 values), checked against ElectroSmash's "Tube Screamer
/// Analysis" and R. G. Keen's "The Technology of the Tube Screamer". The
/// supplies are ideal (9 V, and 4.5 V for the 10k/10k + 47u reference);
/// device capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::tube_screamer_ts808_matches_ngspice`:
/// this netlist through `Circuit<Dk>` against ngspice on the deck
/// `tube_screamer_ts808.cir` (300 mV at 1 kHz, the op-amps written as
/// behavioural sources), 3e-5 normalized RMS at 16× (measured 1.4e-5),
/// converging as h² from 8×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> Tube Screamer TS-808"`.
///
/// # Aliasing
/// Diode clipping and op-amp slewing: run it oversampled.
///
/// # Valid ranges
/// Inputs within the 9 V supply (the macromodel has no common-mode
/// limits); tested at the host rate on a 1.5 V hot signal.
///
/// [`GummelPoon::N2N5088`]: crate::circuit::devices::bjt::GummelPoon::N2N5088
/// [`Shockley::N914`]: crate::circuit::devices::diode::Shockley::N914
pub fn tube_screamer_ts808() -> Netlist {
    let mut netlist = netlist_of(
        "Tube Screamer TS-808
VIN in 0 0
RPD in 0 2.2meg
VA va 0 9
VB vb 0 4.5
C1 in a 22n
R1 a b1 1k
R2 vb b1 510k
Q1 va b1 e1 Q2N5088
R3 e1 0 10k
C2 e1 p1 1u
R4 vb p1 10k
RF o1 m1 551k
CF o1 m1 51p
D1 o1 m1 D1N914
D2 m1 o1 D1N914
R5 m1 s 4.7k
C4 s vb 47n
R7 o1 t 1k
C5 t 0 220n
R8 vb t 10k
RT1 t w 10k
RT2 w m2 10k
C6 w x 220n
R9 x 0 220
R10 o2 m2 1k
C7 o2 y 1u
R11 y l 1k
RLV l vb 100k
C8 l b2 100n
R12 vb b2 510k
Q2 va b2 e2 Q2N5088
R13 e2 0 10k
R14 e2 z 100
C9 z out 1u
R15 out 0 10k
.model Q2N5088 NPN(Is=5.911f Bf=1122 Ne=1.394 Ise=5.911f Ikf=14.92m Br=1.271 Nc=2 Isc=0 Ikr=0 Rc=1.61 Rb=10 Vaf=62.37)
.model D1N914 D(Is=2.52n Rs=.568 N=1.752)
.control
wrdata out v(out)
.endc",
    );
    op_amp(
        &mut netlist,
        "u1a",
        ("p1", "m1", "o1"),
        Macromodel::JRC4558,
        (0.0, 9.0),
    );
    op_amp(
        &mut netlist,
        "u1b",
        ("t", "m2", "o2"),
        Macromodel::JRC4558,
        (0.0, 9.0),
    );
    netlist
}
