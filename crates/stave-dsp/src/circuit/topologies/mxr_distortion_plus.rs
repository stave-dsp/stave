//! The MXR Distortion+ as a netlist.

use super::tone_stacks::legs;
use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The MXR Distortion+ (M-104), input jack to output: source `vin` at
/// node `in` (DC 0, drive it; the pedal biases itself at 4.5 V), output
/// `out` (into an amplifier's 1 MΩ). `distortion` and `output` are the
/// pots' positions, 0..=1, as fractions of the track.
///
/// # Models
/// The whole pedal: C1 1n at the input to ground; C2 10n and R1 10k into
/// a 741's + input, R2 1M to the 4.5 V reference (R6/R7 1M from the 9 V
/// supply, C6 1u); the non-inverting stage, R4 1M from output to the
/// inverting input and C3 47n + R3 4k7 + the 1M distortion rheostat from
/// there to ground (gain 1 + 1M/(4k7 + R), 3.5 to 46.6 dB, its low end
/// rolled off below 1/(2π · 47n · (4k7 + R))); C4 1u and R5 10k into the
/// antiparallel germanium pair and C5 1n to ground; the 10k output pot a
/// divider. The op-amp is [`Macromodel::LM741`] on 0–9 V (the M-104
/// shipped LM741CN and µA741CP parts); the diodes [`Shockley::N34A`],
/// standing in for the MXR's 1N270, which has no published card.
///
/// # Derivation
/// The values of ElectroSmash's "MXR Distortion + Analysis" schematic,
/// with Aion FX's "Aphelion" documentation (p. 7, the Distortion+ values)
/// for the wiring and its note that the Distortion+ had no feedback
/// capacitor. `distortion` shorts that fraction of the rheostat
/// (1 is 0 Ω, full gain) and `output` puts the wiper that fraction of the
/// track up from ground; the pots' reverse-log and audio tapers are the
/// caller's to apply. The supply is ideal; device capacitances are not
/// modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::mxr_distortion_plus_matches_ngspice`:
/// at distortion 0.5 this netlist and the deck
/// `mxr_distortion_plus_ac.cir` through `Circuit<Dk>`, the linearised
/// response against ngspice's AC analysis from 10 Hz to 20 kHz within
/// 5e-6 dB (measured 1.5e-6); and at both pots' maximum, 100 mV at 1 kHz
/// (the 741 rails and slews, the germanium pair clips), the deck
/// `mxr_distortion_plus.cir` (the op-amp written as behavioural sources)
/// and this netlist on its stimulus against ngspice's transient, 2e-4
/// normalized RMS at 32× (measured 1.05e-4), converging as h² from 4×
/// (6.2e-3). Its R17 matrix and R18 fingerprint run at 32×
/// (`characterise.rs`).
///
/// # Cost
/// Datasheet `"Circuit<Dk> MXR Distortion+ (LM741)"`.
///
/// # Aliasing
/// A railing, slewing op-amp into hard germanium clipping: run it
/// oversampled.
///
/// # Valid ranges
/// `distortion` and `output` 0..=1 (asserted); each pot leg at least
/// [`POT_END_OHMS`](super::tone_stacks::POT_END_OHMS). Inputs within the
/// 9 V supply (the macromodel has no common-mode limits); tested at the
/// host rate on a 1.5 V hot signal.
///
/// [`Shockley::N34A`]: crate::circuit::devices::diode::Shockley::N34A
pub fn mxr_distortion_plus(distortion: f64, output: f64) -> Netlist {
    // The rheostat: the unshorted part of its track.
    let (_, rheostat) = legs(1e6, distortion);
    let (bottom, top) = legs(10e3, output);
    let mut netlist = netlist_of(&format!(
        "MXR Distortion+
VIN in 0 DC 0 AC 1
VA va 0 9
R6 va vb 1meg
R7 vb 0 1meg
C6 vb 0 1u
C1 in 0 1n
C2 in a 10n
R1 a p 10k
R2 vb p 1meg
R4 o m 1meg
C3 m g 47n
R3 g h 4.7k
RD h 0 {rheostat}
C4 o q 1u
R5 q d 10k
C5 d 0 1n
D1 d 0 D1N34A
D2 0 d D1N34A
RV1 d out {top}
RV2 out 0 {bottom}
RL out 0 1meg
.model D1N34A D(IS=2.6u RS=6.5 N=1.6 BV=25 IBV=0.003)
.control
wrdata out v(out)
.endc"
    ));
    op_amp(&mut netlist, "u1", ("p", "m", "o"), Macromodel::LM741, (0.0, 9.0));
    netlist
}
