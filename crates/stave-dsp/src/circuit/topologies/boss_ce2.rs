//! The Boss CE-2 Chorus's BBD filters as a netlist.

use super::netlist_of;
use crate::circuit::netlist::Netlist;

/// The Boss CE-2's anti-alias and reconstruction filters, the BBD an
/// ideal unity copy: source `vin` at node `in` (DC 0, AC 1; drive it),
/// output `q3e` (the reconstruction follower's emitter).
///
/// # Models
/// The two transistor stages around the CE-2's MN3007: each a third-order
/// low-pass (10k – 3n3 – 10k – 8n2 bootstrapped to the emitter – 10k –
/// 470p) closed by a 2N5088 emitter follower with 10k to ground; the
/// pre-filter fed through 33n onto the 4.5 V reference by 100k and
/// driving the BBD's input through 4k7, the post-filter fed by the BBD's
/// output through 33n and 330k. The MN3007 between them is a unity
/// voltage copy (an ideal VCVS: no delay, no clock, no loss — the
/// chip is [`BucketBrigade`](super::bucket_brigade::BucketBrigade)'s
/// job). Both transistors are [`GummelPoon::N2N5088`].
///
/// # Derivation
/// Transcribed from madbeanpedals' "PorkBarrel24" schematic (p. 13), a
/// CE-2 clone "with no circuit changes". The supplies are ideal (9 V; the
/// 4.5 V reference); IC1A, which drives the pre-filter, is an ideal
/// source; the BBD's 56k output pull-down and the blend network are left
/// out; device capacitances are not modelled.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::boss_ce2_filters_match_ngspice`:
/// this netlist and the deck `boss_ce2_filters.cir` through
/// `Circuit<Dk>`, the linearised response against ngspice's AC analysis
/// from 10 Hz to 50 kHz within 1e-5 dB (measured 3.3e-6, at 50 kHz where
/// the response is 109 dB down); and the characterisation matrix
/// (DSP.md R17) `crates/stave-test/tests/characterise.rs::boss_ce2_filters`
/// (operating point, response, BLA, harmonics and aliasing against
/// ngspice; budgets in the characterisation table, three times measured:
/// at 4× the operating point within 7.9e-7 V and the response within
/// 4.4e-5 dB).
///
/// # Cost
/// Datasheet `"Circuit<Dk> CE-2 BBD filters"`.
///
/// # Aliasing
/// Two emitter followers at audio levels: mild; run it at 2× or more when
/// it is driven hot.
///
/// # Valid ranges
/// Inputs well inside the 9 V supply (the followers cut off below about
/// −3.9 V of swing at their emitters); host rates from 44.1 kHz.
///
/// [`GummelPoon::N2N5088`]: crate::circuit::devices::bjt::GummelPoon::N2N5088
pub fn boss_ce2_filters() -> Netlist {
    netlist_of(
        "Boss CE-2 BBD filters
VIN in 0 DC 0 AC 1
VC vc 0 9
VB vb 0 4.5
C8 in a 33n
R14 a vb 100k
R15 a b 10k
C9 b 0 3.3n
R16 b c 10k
C10 c q2e 8.2n
R17 c d 10k
C11 d 0 470p
Q2 vc d q2e Q2N5088
R18 q2e 0 10k
R19 q2e bbd 4.7k
EBBD bo 0 bbd 0 1
C13 bo f 33n
R21 f vb 330k
R22 f g 10k
C14 g 0 3.3n
R23 g h 10k
C15 h q3e 8.2n
R24 h k 10k
C16 k 0 470p
Q3 vc k q3e Q2N5088
R25 q3e 0 10k
.model Q2N5088 NPN(Is=5.911f Bf=1122 Ne=1.394 Ise=5.911f Ikf=14.92m Br=1.271 Nc=2 Isc=0 Ikr=0 Rc=1.61 Rb=10 Vaf=62.37)
.control
wrdata out v(q3e)
.endc",
    )
}
