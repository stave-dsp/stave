//! The Moog 904-A transistor ladder core as a netlist.

use super::netlist_of;
use crate::circuit::netlist::Netlist;

/// The Moog 904-A transistor ladder core on a 100 µA tail with the
/// range-3 capacitors (0.075 µF, a corner near 2 kHz), and ideal
/// regeneration `k`: source `vin` at node `sig` (DC 0, drive it), output
/// `out`, the voltage across the top capacitor. `k = 0` is the open-loop
/// four-pole core; it self-oscillates near `k = 4`.
///
/// # Models
/// The ladder of the 904-A voltage-controlled low-pass filter: the bottom
/// differential pair Q7/Q8 on an ideal tail current (the exponential
/// converter frozen, the 47 Ω balance trim left out), the rungs
/// Q9–Q14 and the top pair Q15/Q16, a capacitor across each of the four
/// rungs, the rung bases from the drawing's 68–220–220–220–680 Ω divider
/// on +12 V, the bottom bases at 3.7 V. Every transistor is the drawing's
/// 2N3392 ([`GummelPoon::N2N3392`]). The output buffer (Q17–Q21) is left
/// out, and its regeneration path through R22, whose 2N4058 PNPs have no
/// published card, is an ideal source: the bottom pair's differential
/// drive is `v(sig) − k · v(out)`, split ±half about 3.7 V — the
/// Stilson–Smith and Huovilainen idealisation. (The patent's ladder with
/// idealised bias, any transistor and tail, is
/// [`TransistorLadder`](super::transistor_ladder::TransistorLadder); this
/// is the 904-A drawing's.)
///
/// # Derivation
/// Transcribed from R. A. Moog Co. drawing 1149 ("904-A Voltage
/// Controlled Low Pass Filter", 1967, rev. C 1970), with US patent
/// 3,475,623 and T. Stinchcombe's "Analysis of the Moog Transistor Ladder
/// and Derivative Filters". Device capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::moog_ladder_904a_matches_ngspice`:
/// at `k = 0` `Circuit<Dk>`'s small-signal response within 5e-5 dB of
/// ngspice's AC analysis (`moog_ladder_ac.cir`), and at `k = 3`, driven
/// 150 mV past the pair's linear range, 1e-5 normalized RMS at 4×
/// (measured 7.2e-6)
/// against ngspice's transient (`moog_ladder_feedback.cir`).
///
/// # Cost
/// Datasheet `"Circuit<Dk> Moog ladder (k = 3)"`.
///
/// # Aliasing
/// The differential pair's tanh and the resonance: run it oversampled.
///
/// # Valid ranges
/// `0 ≤ k < 4` for a stable filter; inputs to a few hundred millivolts
/// (the pair saturates past about 100 mV).
///
/// [`GummelPoon::N2N3392`]: crate::circuit::devices::bjt::GummelPoon::N2N3392
pub fn moog_ladder_904a(k: f64) -> Netlist {
    netlist_of(&format!(
        "Moog 904-A ladder core
VCC v12 0 12
VBB bb 0 3.7
VIN sig 0 DC 0 AC 1
BP inp bb V = {{0.5*(V(sig) - {k}*V(l4,r4))}}
BN inn bb V = {{-0.5*(V(sig) - {k}*V(l4,r4))}}
R29 v12 n4 68
R28 n4 n3 220
R27 n3 n2 220
R26 n2 n1 220
R25 n1 0 680
Q7 l1 inp t Q2N3392
Q8 r1 inn t Q2N3392
IT t 0 100u
C1 l1 r1 0.075u
Q9 l2 n1 l1 Q2N3392
Q10 r2 n1 r1 Q2N3392
C2 l2 r2 0.075u
Q11 l3 n2 l2 Q2N3392
Q12 r3 n2 r2 Q2N3392
C3 l3 r3 0.075u
Q13 l4 n3 l3 Q2N3392
Q14 r4 n3 r3 Q2N3392
C4 l4 r4 0.075u
Q15 v12 n4 l4 Q2N3392
Q16 v12 n4 r4 Q2N3392
EOUT out 0 l4 r4 1
.model Q2N3392 NPN(IS=12.03f ISE=4.958p ISC=0 BF=254.8 BR=4.379 IKF=0.1072 IKR=0 VAF=37.37 VAR=12.5 RE=0.1 RC=1 RB=10 NF=1 NR=1 NE=1.971 NC=2)
.control
wrdata out v(out)
.endc"
    ))
}
