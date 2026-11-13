//! The ProCo RAT as a netlist.

use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The ProCo RAT, input jack to volume pot: source `vin` at node `in`
/// (DC 0, drive it; the pedal biases itself at 4.5 V), output `out`.
/// Distortion and volume at maximum, filter at zero (brightest).
///
/// # Models
/// The whole effect path of the stock RAT: input 22n with 1M to the
/// reference and 1k / 1n into the LM308; the non-inverting gain stage
/// with the 100k distortion pot ‖ 100p in feedback and two ground legs
/// (47 Ω + 2.2u, 560 Ω + 4.7u); 4.7u and 1k into an antiparallel 1N914
/// pair to ground; the 1k5 / 3n3 filter; the 2N5458 source follower
/// (1M gate leak, 10k source) into the 100k volume; the 2M2 input
/// pulldown. The op-amp is [`Macromodel::LM308`] (its datasheet's 30 pF
/// compensation figures) on 0–9 V; [`ShichmanHodges::N2N5458`];
/// [`Shockley::N914`].
///
/// # Derivation
/// Transcribed from Aion FX's "Helios Classic Distortion" schematic (p. 7)
/// in its stock options (the SWEEP mod at zero, the MODE switch's 1N914
/// pair, the LEDs left out), checked against ElectroSmash's "ProCo RAT
/// Analysis". The supplies are ideal (9 V, and the 4.5 V reference);
/// device capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::proco_rat_matches_ngspice`:
/// this netlist through `Circuit<Dk>` against ngspice on the deck
/// `proco_rat.cir` (100 mV at 1 kHz into a gain of about 2300: the
/// LM308 slews and the diodes clip, the op-amp written as behavioural
/// sources), 1.5e-5 normalized RMS at 64× (measured 8.2e-6), converging
/// as h² from 32×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> ProCo RAT (LM308)"`.
///
/// # Aliasing
/// Hard diode clipping and op-amp slewing: run it oversampled.
///
/// # Valid ranges
/// Inputs within the 9 V supply (the macromodel has no common-mode
/// limits); tested at the host rate on a 1.5 V hot signal.
///
/// [`ShichmanHodges::N2N5458`]: crate::circuit::devices::jfet::ShichmanHodges::N2N5458
/// [`Shockley::N914`]: crate::circuit::devices::diode::Shockley::N914
pub fn proco_rat() -> Netlist {
    let mut netlist = netlist_of(
        "ProCo RAT
VIN in 0 0
RPD in 0 2.2meg
VA va 0 9
VB vb 0 4.5
C1 in a 22n
R1 a vb 1meg
R2 a p 1k
C2 p 0 1n
RD o m 100k
C4 o m 100p
R3 m k1 47
C5 k1 0 2.2u
R4 m k2 560
C6 k2 0 4.7u
C7 o q 4.7u
R5 q d 1k
D2 d 0 D1N914
D3 0 d D1N914
R6 d f 1.5k
C8 f 0 3.3n
C9 f g 22n
R7 g 0 1meg
J1 va g s J2N5458
R8 s 0 10k
C10 s out 1u
RV out 0 100k
.model D1N914 D(Is=2.52n Rs=.568 N=1.752)
.model J2N5458 NJF(VTO=-4.00 BETA=2.34E-03 LAMBDA=5.16M RD=3.80E+01 RS=6.52E+01 IS=6.74E-15)
.control
wrdata out v(out)
.endc",
    );
    op_amp(
        &mut netlist,
        "u1",
        ("p", "m", "o"),
        Macromodel::LM308,
        (0.0, 9.0),
    );
    netlist
}
