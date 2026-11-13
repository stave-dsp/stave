//! The Electro-Harmonix Big Muff Pi as a netlist.

use super::netlist_of;
use super::tone_stacks::legs;
use crate::circuit::netlist::Netlist;

/// The Big Muff Pi with the "Gilmour '73 Ram's Head" values, input jack
/// to volume pot: source `vin` at node `in` (DC 0, drive it), output
/// `out`. Sustain and volume at maximum, tone at noon.
///
/// # Models
/// The whole effect path of the Ram's Head Big Muff Pi: four silicon NPN
/// common-emitter stages on 9 V — the input booster (470k ‖ 470p shunt
/// feedback), two clipping stages (the same, each with an antiparallel
/// 1N914 pair in series with 100n from collector to base), the passive
/// tone stack (33k / 12n low-pass and 3n9 / 33k high-pass mixed by the
/// 100k pot), and the fixed-bias output booster — with the 2M2 input
/// pulldown. 2N3904s ([`GummelPoon::N2N3904`]) where the originals had
/// BC239 / 2N5133-class parts; [`Shockley::N914`] diodes.
///
/// # Derivation
/// Transcribed from Aion FX's "Halo Distortion/Sustainer" schematic
/// (p. 7, Kit Rae's Ram's Head values), checked against ElectroSmash's
/// "Big Muff Pi Analysis" (the V3, same topology). The supply is an
/// ideal 9 V (the 1N5817, 100 Ω and filter caps are left out); device
/// capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::big_muff_rams_head_matches_ngspice`:
/// this netlist through `Circuit<Dk>` against ngspice on the deck
/// `big_muff_rams_head.cir` (200 mV at 220 Hz, every stage clipping),
/// 1e-4 normalized RMS at 16× (measured 6.7e-5), converging as h² from
/// 8×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> Big Muff (Ram's Head)"`.
///
/// # Aliasing
/// Hard diode and transistor clipping: run it oversampled.
///
/// # Valid ranges
/// Guitar-level inputs and well past (the solver is tested at the host
/// rate on a 3 V hot signal).
///
/// [`GummelPoon::N2N3904`]: crate::circuit::devices::bjt::GummelPoon::N2N3904
/// [`Shockley::N914`]: crate::circuit::devices::diode::Shockley::N914
pub fn big_muff_rams_head() -> Netlist {
    netlist_of(
        "Big Muff Pi, Ram's Head
VIN in 0 0
RPD in 0 2.2meg
VA va 0 9
R2 in a 33k
C1 a b1 100n
R3 b1 0 100k
R4 c1 b1 470k
C2 c1 b1 470p
R6 va c1 10k
Q1 c1 b1 e1 Q2N3904
R5 e1 0 100
C3 c1 s 100n
RSUS s r7 100k
R7 r7 0 560
C4 s a2 100n
R8 a2 b2 10k
R9 b2 0 100k
R10 c2 b2 470k
C5 c2 b2 470p
R12 va c2 10k
Q2 c2 b2 e2 Q2N3904
R11 e2 0 100
C6 b2 d2 100n
D2 d2 c2 D1N914
D3 c2 d2 D1N914
C7 c2 a3 100n
R13 a3 b3 10k
R14 b3 0 100k
R15 c3 b3 470k
C8 c3 b3 470p
R17 va c3 10k
Q3 c3 b3 e3 Q2N3904
R16 e3 0 100
C9 b3 d3 100n
D4 d3 c3 D1N914
D5 c3 d3 D1N914
R19 c3 lp 33k
C11 lp 0 12n
C10 c3 hp 3.9n
R18 hp 0 33k
RT1 lp w 50k
RT2 w hp 50k
C12 w b4 100n
R20 va b4 470k
R21 b4 0 100k
R22 va c4 10k
Q4 c4 b4 e4 Q2N3904
R23 e4 0 2.7k
C13 c4 out 100n
RV out 0 100k
.model Q2N3904 NPN(Is=6.734f Bf=416.4 Ne=1.259 Ise=6.734f Ikf=66.78m Br=.7371 Nc=2 Isc=0 Ikr=0 Rc=1 Rb=10 Vaf=74.03)
.model D1N914 D(Is=2.52n Rs=.568 N=1.752)
.control
wrdata out v(out)
.endc",
    )
}

/// The Big Muff Pi's tone stack on its own (Ram's Head values), loaded as
/// in the pedal: input `vin` at node `i`, output `w` (the tone pot's
/// wiper). `tone` is the wiper's position, 0..=1, from the low-pass end
/// (dark) to the high-pass end (bright); 0.5 is noon, the mid scoop.
///
/// # Models
/// The passive tone network of [`big_muff_rams_head`]: the 33k / 12n
/// low-pass and the 3n9 / 33k high-pass mixed by the 100k pot, loaded by
/// 100n into the output booster's 470k ‖ 100k bias divider (the
/// transistor's own input impedance left out), driven by an ideal source
/// (Q3's collector, about 10k, in the pedal).
///
/// # Derivation
/// Aion FX's "Halo Distortion/Sustainer" schematic (p. 7), as
/// [`big_muff_rams_head`]; ElectroSmash's analysis measures the V3's
/// notch (−13.5 dB near 1 kHz at noon).
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::big_muff_tone_stack_matches_ngspice`:
/// `Circuit<Dk>`'s response at noon within 1e-10 dB of ngspice's AC
/// analysis of the deck `big_muff_tone_stack.cir`.
///
/// # Cost
/// Datasheet `"Circuit<Dk> tone stack"` (a network of the same size).
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// `tone` 0..=1 (asserted); each pot leg at least
/// [`POT_END_OHMS`](super::tone_stacks::POT_END_OHMS).
pub fn big_muff_tone_stack(tone: f64) -> Netlist {
    let (low, high) = legs(100e3, tone);
    netlist_of(&format!(
        "Big Muff Pi tone stack
VIN i 0 DC 0 AC 1
R19 i lp 33k
C11 lp 0 12n
C10 i hp 3.9n
R18 hp 0 33k
RT1 lp w {low}
RT2 w hp {high}
C12 w b4 100n
R20 b4 0 470k
R21 b4 0 100k
.control
wrdata out vm(w)
.endc"
    ))
}
