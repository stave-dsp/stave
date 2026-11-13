//! The MXR Phase 90 as a netlist.

use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The MXR Phase 90 (script logo), input jack to output, its LFO frozen:
/// source `vin` at node `in` (DC 0, drive it; the pedal biases itself on
/// its 5.1 V reference), the four JFETs' gates held at `gate_volts` by
/// the source `vg`, output `out` (into an amplifier's 1 MΩ).
///
/// # Models
/// The audio path of the 1974 script Phase 90: the input follower (10k,
/// 10n, 470k to the reference); four first-order allpass stages, each an
/// op-amp with 10k in and 10k feedback and 47n into its + input, which
/// goes to the reference through 22k ‖ a 2N5952's channel — the swept
/// resistor that places each stage's 90° point; the mixer, dry and wet
/// through 150k each into a 2N4125 PNP inverting summer (emitter on the
/// reference, 150k collector to base, 56k to ground), then 47n, 150k
/// and 10k out. The five audio-path LM741s are [`Macromodel::LM741`] on 0–9 V;
/// [`ShichmanHodges::N2N5952`]; [`GummelPoon::N2N4125`]. The LFO (IC6,
/// its 15 µF integrator and Schmitt trigger) is not part of the netlist:
/// `vg` stands for the gate bus it drives through 3M9 (with the 1M bias
/// trimmer and 47n to the reference). Script version: no R15.
///
/// # Derivation
/// Transcribed from General Guitar Gadgets' "P90" schematic (JD Sleep,
/// version 2015June30) and its BOM, checked against madbeanpedals'
/// "NomNom" stock values and R. G. Keen's "The Technology of Phase
/// Shifters and Flangers". The supplies are ideal (9 V, and the 5.1 V
/// zener reference); device capacitances are not modelled. Each stage's
/// pole is `1/(2π·47n·(22k ‖ rds))`, and `rds ≈ 1/(2β(vgs − Vto))` near
/// zero drain-source volts: with the reference at 5.1 V, gates from about
/// 3.1 V (channel off, 22k alone: 154 Hz) up toward 4.5 V (a few hundred
/// ohms: several kHz).
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::mxr_phase90_matches_ngspice`:
/// at 3.4 V on the gates, this netlist and the deck `mxr_phase90_ac.cir`
/// through `Circuit<Dk>`, the linearised response against ngspice's AC
/// analysis within 3e-5 dB from 10 Hz to 20 kHz (measured 9.7e-6); and
/// the deck `mxr_phase90.cir` (300 mV at 1 kHz, the op-amps written as
/// behavioural sources) and this netlist on its stimulus against
/// ngspice's transient, 6e-5 normalized RMS at 16× (measured 3.0e-5),
/// converging as h² from 2×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> MXR Phase 90 (LM741)"`.
///
/// # Aliasing
/// The JFET channels' square law and the op-amps' slewing: mild at a
/// guitar's level; run it at 2× or more.
///
/// # Valid ranges
/// `gate_volts` from about 3 V (channels off) to the 5.1 V reference
/// (vgs ≤ 0; past it the gate junctions conduct); inputs within the
/// 9 V supply. Verified at one frozen gate voltage: the sweep's own
/// time variation (the LFO moving the channels within a period) is not.
///
/// [`ShichmanHodges::N2N5952`]: crate::circuit::devices::jfet::ShichmanHodges::N2N5952
/// [`GummelPoon::N2N4125`]: crate::circuit::devices::bjt::GummelPoon::N2N4125
pub fn mxr_phase90(gate_volts: f64) -> Netlist {
    let mut netlist = netlist_of(&format!(
        "MXR Phase 90
VIN in 0 DC 0 AC 1
VA va 0 9
VR vr 0 5.1
VG g 0 {gate_volts}
R1 in i1 10k
C1 i1 p0 10n
R2 p0 vr 470k
R7 b0 m1 10k
R11 m1 y1 10k
C2 b0 p1 47n
R3 p1 vr 22k
J1 p1 g vr J2N5952
R8 y1 m2 10k
R12 m2 y2 10k
C3 y1 p2 47n
R4 p2 vr 22k
J2 p2 g vr J2N5952
R9 y2 m3 10k
R13 m3 y3 10k
C4 y2 p3 47n
R5 p3 vr 22k
J3 p3 g vr J2N5952
R10 y3 m4 10k
R14 m4 y4 10k
C5 y3 p4 47n
R6 p4 vr 22k
J4 p4 g vr J2N5952
R27 b0 b5 150k
R24 y4 b5 150k
Q5 c5 b5 vr Q2N4125
R26 b5 c5 150k
R28 c5 0 56k
C9 c5 o9 47n
R29 o9 0 150k
R30 o9 out 10k
RL out 0 1meg
.model J2N5952 NJF(Beta=1.314m Lambda=4m Vto=-2.021 Rd=1 Rs=1 Is=33.57f N=1)
.model Q2N4125 PNP(IS=1.41f BF=85.04 BR=5.576 VAF=18.7 VAR=6.5 IKF=80m RB=10 RC=2.5 RE=0.15)
.control
wrdata out v(out)
.endc"
    ));
    let amps = [
        ("u1", ("p0", "b0", "b0")),
        ("u2", ("p1", "m1", "y1")),
        ("u3", ("p2", "m2", "y2")),
        ("u4", ("p3", "m3", "y3")),
        ("u5", ("p4", "m4", "y4")),
    ];
    for (name, pins) in amps {
        op_amp(&mut netlist, name, pins, Macromodel::LM741, (0.0, 9.0));
    }
    netlist
}
