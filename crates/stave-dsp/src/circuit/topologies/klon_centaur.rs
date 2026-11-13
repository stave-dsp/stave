//! The Klon Centaur as a netlist.

use super::tone_stacks::legs;
use super::{netlist_of, op_amp};
use crate::circuit::devices::opamp::Macromodel;
use crate::circuit::netlist::Netlist;

/// The Klon Centaur, input jack to volume pot: source `vin` at node `in`
/// (DC 0, drive it; the pedal biases itself at 4.5 V), output `out`.
/// `gain` and `treble` are the pots' positions, 0..=1 (0.5 is noon);
/// volume at maximum.
///
/// # Models
/// The whole effect path: the TL072 input buffer; the dirty path — 100n,
/// 10k ‖ 68n into the non-inverting gain stage (422k ‖ 390p feedback,
/// 2k + 15k ‖ 82n to its inverting input through gang A of the dual
/// gain pot, whose wiper sits on the reference), 1u / 1k into the
/// 1N34A pair to ground, 1u, then 47k into the summing node and 2n2 +
/// 22k to gang B's wiper; the clean paths — feed-forward network 1 (1k5,
/// 1u to ground, 15k into the summing node) and network 2 (5k1 ‖ 68n
/// from the buffer to gang B's top, 1k5 and 390n + 1k to the reference,
/// gang B's wiper into the summing node through 27k ‖ (27n + 12k)); the
/// inverting summer (392k ‖ 820p); the active treble shelf (100k / 100k,
/// 1k8, the 10k treble pot, 3n9, 4k7); 4u7 and 560 Ω into the 10k volume.
/// `gain` puts each gang's wiper `gain` × 100k from its top: at 0 gang A
/// shorts the gain stage's input to the reference and gang B passes
/// network 2 whole — the clean blend alone; at 1 the reverse. `treble`
/// at 1 puts the treble wiper at the 1k8 end (the most boost). The first
/// TL072 runs on 0–9 V and the second on the charge pump's −9 and 18 V;
/// all four halves are [`Macromodel::TL072_SLOS080J`] (SLOS080J's table
/// figures, symmetric slew, as the decks write them; the curve-fitted
/// [`Macromodel::TL072`] slews asymmetrically); [`Shockley::N34A`].
///
/// # Derivation
/// Transcribed from Aion FX's "Refractor" schematic (p. 8), a
/// part-for-part replica, and checked node by node against ElectroSmash's
/// "Klon Centaur Analysis" (its gain stage, feed-forward networks,
/// summing amplifier and tone control figures; the gangs' sense from its
/// text: as the gain stage gets louder network 2 gets quieter). The
/// supplies are ideal (the charge pump's ripple and the anti-pop
/// resistors of the bypass left out); device capacitances are not
/// modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::klon_centaur_matches_ngspice`:
/// the clean/dry blend — this netlist and the deck
/// `klon_centaur_clean.cir` at gain 0 (within 1e-8 dB, measured 2.3e-9),
/// and `klon_centaur_ac_max.cir` at gain 1 (within 1e-5 dB, measured
/// 2.2e-6), through `Circuit<Dk>`, the linearised response against
/// ngspice's AC analysis from 10 Hz to 20 kHz; and at noon, 300 mV at
/// 1 kHz, the deck `klon_centaur.cir` (the op-amps written as behavioural
/// sources) and this netlist on its stimulus against ngspice's
/// transient, 1e-4 normalized RMS at 8× (measured 4.2e-5), converging as
/// h² from 2×.
///
/// # Cost
/// Datasheet `"Circuit<Dk> Klon Centaur (TL072)"`.
///
/// # Aliasing
/// Germanium clipping and the gain stage's rails: run it oversampled.
///
/// # Valid ranges
/// `gain` and `treble` 0..=1 (asserted); each pot leg at least
/// [`POT_END_OHMS`](super::tone_stacks::POT_END_OHMS). Inputs within the
/// 9 V supply (the macromodel has no common-mode limits, so the TL072's
/// phase reversal near its negative rail is not modelled); tested at the
/// host rate on a 1.5 V hot signal.
///
/// [`Shockley::N34A`]: crate::circuit::devices::diode::Shockley::N34A
pub fn klon_centaur(gain: f64, treble: f64) -> Netlist {
    let (top, bottom) = legs(100e3, gain);
    let (toward_input, toward_output) = legs(10e3, 1.0 - treble);
    let mut netlist = netlist_of(&format!(
        "Klon Centaur
VIN in 0 DC 0 AC 1
VA va 0 9
VP vp 0 18
VN vn 0 -9
VR vr 0 4.5
R1 in a 10k
C1 a p1 100n
R2 vr p1 2meg
C3 bf c 100n
R6 c d 10k
C5 c d 68n
RGA1 d vr {top}
RGA2 vr ga {bottom}
R10 ga gb 2k
R11 gb m2 15k
C7 gb m2 82n
R12 o2 m2 422k
C8 o2 m2 390p
C9 o2 v1 1u
R13 v1 v 1k
D2 v 0 D1N34A
D3 0 v D1N34A
C10 v u 1u
R16 u s 47k
C11 u u2 2.2n
R15 u2 sb 22k
R5 bf p 5.1k
C4 bf p 68n
R8 p vr 1.5k
C6 p p9 390n
R9 p9 vr 1k
RGB1 p sb {top}
RGB2 sb vr {bottom}
R17 s sb 27k
C12 s s18 27n
R18 s18 sb 12k
R7 c x 1.5k
C16 x 0 1u
R19 x s 15k
R20 o3 s 392k
C13 o3 s 820p
R22 o3 t 100k
R21 o3 k1 1.8k
RTA k1 tw {toward_input}
RTB tw k3 {toward_output}
C14 tw t 3.9n
R23 k3 o4 4.7k
R24 o4 t 100k
C15 o4 y 4.7u
R25 y out 560
RV out 0 10k
.model D1N34A D(IS=2.6u RS=6.5 N=1.6 BV=25 IBV=0.003)
.control
wrdata out v(out)
.endc"
    ));
    let part = Macromodel::TL072_SLOS080J;
    op_amp(&mut netlist, "u1a", ("p1", "bf", "bf"), part, (0.0, 9.0));
    op_amp(&mut netlist, "u1b", ("d", "m2", "o2"), part, (0.0, 9.0));
    op_amp(&mut netlist, "u2a", ("vr", "s", "o3"), part, (-9.0, 18.0));
    op_amp(&mut netlist, "u2b", ("vr", "t", "o4"), part, (-9.0, 18.0));
    netlist
}
