//! The Boss DS-1 Distortion as a netlist.

use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The Boss DS-1, input jack to output: source `vin` at node `in` (DC 0,
/// drive it; the pedal biases itself at 4.5 V), output `out`.
/// Distortion and level at maximum, tone at noon.
///
/// # Models
/// The whole effect path of the DS1-A circuit: the input emitter
/// follower; two high-passes into the shunt-feedback transistor booster
/// (470k ‖ 250p collector to base, 10k collector, 22 Ω emitter); 68n /
/// 100k onto the reference with the 47k / 1N4148 input clamp; the
/// NJM2904's first half a unity buffer, its second the non-inverting gain
/// stage (the 100k distortion pot in feedback, 4k7 + 0.47u to the
/// reference, 100p); 2k2 / 0.47u into an antiparallel 1N4148 pair and
/// 10n on the reference; the passive tone stage (6k8 / 100n low-pass,
/// 22n / 2k2 / 6k8 high-pass, the 20k pot); the 100k level pot; the
/// output follower (47n, 1M bias, 10k emitter) and 1k / 1u / 100k out.
/// Both op-amp halves are [`Macromodel::NJM2904`] on 0–9 V (the part
/// since 2006, whose output reaches its negative rail); the transistors
/// [`GummelPoon::N2N5088`], standing in for the 2SC2240, which has no
/// published card; [`Shockley::N4148`].
///
/// # Derivation
/// The values of ElectroSmash's "Boss DS-1 Distortion Analysis" parts
/// list (the original PCB's labels) in the wiring of Aion FX's "Comet"
/// schematic (p. 8). The supplies are ideal (9 V, and the 4.5 V
/// reference); the JFET bypass switching and device capacitances are not
/// modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::boss_ds1_matches_ngspice`:
/// this netlist through `Circuit<Dk>` against ngspice on the deck
/// `boss_ds1.cir` (200 mV at 1 kHz: the booster saturates, the op-amp
/// rails and the diodes clip; the op-amps written as behavioural
/// sources), 3e-4 normalized RMS at 32× (measured 1.4e-4), converging as
/// h² from 4×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> Boss DS-1 (NJM2904)"`.
///
/// # Aliasing
/// A saturating booster, a railing op-amp and hard diode clipping: run it
/// oversampled.
///
/// # Valid ranges
/// Inputs within the 9 V supply (the macromodel has no common-mode
/// limits; the NJM2904's crossover distortion and its sink-current-
/// dependent low limit are not modelled); tested at the host rate on a
/// 1.5 V hot signal.
///
/// [`GummelPoon::N2N5088`]: crate::circuit::devices::bjt::GummelPoon::N2N5088
/// [`Shockley::N4148`]: crate::circuit::devices::diode::Shockley::N4148
pub fn boss_ds1() -> Netlist {
    let mut netlist = netlist_of(
        "Boss DS-1
VIN in 0 0
VA va 0 9
VB vb 0 4.5
R1 in a 1k
C1 a b1 47n
R2 vb b1 470k
Q1 va b1 e1 Q2N5088
R3 e1 0 10k
C2 e1 x 0.47u
R4 x 0 100k
C3 x b2 47n
R5 b2 0 100k
R7 c2 b2 470k
C4 c2 b2 250p
R8 va c2 10k
Q2 c2 b2 e2 Q2N5088
R9 e2 0 22
C5 c2 p 68n
R10 vb p 100k
R39 p q 47k
D8 0 q D1N4148
RD o m 100k
C7 o m 100p
R13 m h 4.7k
C8 h vb 0.47u
R14 o y 2.2k
C9 y d 0.47u
D4 d vb D1N4148
D5 vb d D1N4148
C10 d vb 10n
R16 d ta 6.8k
C12 ta vb 100n
C11 d tc 22n
R15 tc tb 2.2k
R17 tb vb 6.8k
RT1 ta w 10k
RT2 w tb 10k
RLV w vb 100k
R18 w u 10k
C13 u b3 47n
R19 vb b3 1meg
Q3 va b3 e3 Q2N5088
R20 e3 0 10k
R21 e3 z 1k
C14 z out 1u
R22 out 0 100k
.model Q2N5088 NPN(Is=5.911f Bf=1122 Ne=1.394 Ise=5.911f Ikf=14.92m Br=1.271 Nc=2 Isc=0 Ikr=0 Rc=1.61 Rb=10 Vaf=62.37)
.model D1N4148 D(Is=2.52n Rs=.568 N=1.752)
.control
wrdata out v(out)
.endc",
    );
    // Not yet named: the buffer half's output, and the gain half's.
    let _ = netlist.add_node("ob");
    op_amp(&mut netlist, "u1a", ("q", "ob", "ob"), Macromodel::NJM2904, (0.0, 9.0));
    op_amp(&mut netlist, "u1b", ("ob", "m", "o"), Macromodel::NJM2904, (0.0, 9.0));
    netlist
}
