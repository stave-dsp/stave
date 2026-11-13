//! Real amplifiers' passive treble/bass/mid tone stacks as netlists, each
//! with the values of the drawing it names. (The network's closed form,
//! for any values, is [`PassiveToneStack`](super::tone_stack::PassiveToneStack).)

use super::netlist_of;
use crate::circuit::netlist::Netlist;

/// The resistance a pot's wiper keeps from either end: carbon pots never
/// reach zero ohms, and a zero-ohm resistor would short two nodes.
pub const POT_END_OHMS: f64 = 1.0;

/// A pot of `ohms` with its wiper at `position` (0..=1, the fraction of
/// the track between the wiper and the end named first), as the two legs
/// `(position · ohms, (1 − position) · ohms)`, each at least
/// [`POT_END_OHMS`].
pub(super) fn legs(ohms: f64, position: f64) -> (f64, f64) {
    assert!(
        (0.0..=1.0).contains(&position),
        "a pot position is 0..=1: {position}"
    );
    (
        (position * ohms).max(POT_END_OHMS),
        ((1.0 - position) * ohms).max(POT_END_OHMS),
    )
}

/// The Yeh–Smith network with a mid pot wired as a potentiometer (C3 on
/// its wiper): input `vin` at `i`, output `o` (the treble wiper).
fn yeh_smith(
    title: &str,
    c1: &str,
    c2: &str,
    c3: &str,
    slope: &str,
    pots: [f64; 3],
    positions: [f64; 3],
) -> Netlist {
    let [treble_ohms, bass_ohms, mid_ohms] = pots;
    let [treble, bass, mid] = positions;
    let (t_bottom, t_top) = legs(treble_ohms, treble);
    let (bass_in, _) = legs(bass_ohms, bass);
    let (m_bottom, m_top) = legs(mid_ohms, mid);
    netlist_of(&format!(
        "{title}
VIN i 0 DC 0 AC 1
C1 i t {c1}
RT1 t o {t_top}
RT2 o b {t_bottom}
R4 i n {slope}
C2 n b {c2}
RB b j {bass_in}
RM1 j w {m_top}
RM2 w 0 {m_bottom}
C3 n w {c3}
.control
wrdata out vm(o)
.endc"
    ))
}

/// The Fender Bassman 5F6-A tone stack: 250p, 20n bass, 20n mid, 56k
/// slope; treble 250k, bass 1M, mid 25k (a potentiometer). Positions
/// are resistance fractions, 0..=1 each (treble: wiper to the foot; bass:
/// in circuit; mid: wiper to ground). Input `vin` at node `i`, output
/// `o`, unloaded.
///
/// # Models
/// The 5F6-A's passive tone network alone, with its drawing's values:
/// driven by an ideal source (the amp drives it from a cathode follower)
/// and unloaded (the amp loads it with 0.02u into the phase inverter's
/// 1M).
///
/// # Derivation
/// Fender's 5F6-A schematic drawing and Yeh & Smith, "Discretization of
/// the '59 Fender Bassman Tone Stack" (DAFx-06), Fig. 1, which agree on
/// 250p / 20n / 20n / 56k. Rob Robinette's "How the TMB Tone Stack Works"
/// states production units as 0.1u / 100k, the AB763's bass cap and
/// slope, with no drawing or measured unit behind it; the two primary
/// sources win.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::real_tone_stacks_match_ngspice`:
/// `Circuit<Dk>`'s response and [`PassiveToneStack`](super::tone_stack::PassiveToneStack)'s
/// closed form, at the deck `tone_stack_fender_5f6a.cir`'s positions,
/// within 1e-10 dB of ngspice's AC analysis.
///
/// # Cost
/// Datasheet `"Circuit<Dk> tone stack"` (the same network).
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// Positions 0..=1 (asserted); each pot leg at least [`POT_END_OHMS`].
pub fn fender_5f6a_tone_stack(treble: f64, bass: f64, mid: f64) -> Netlist {
    yeh_smith(
        "Fender 5F6-A tone stack",
        "250p",
        "20n",
        "20n",
        "56k",
        [250e3, 1e6, 25e3],
        [treble, bass, mid],
    )
}

/// The Marshall JCM800 2203 tone stack: 470p, 22n bass, 22n mid, 33k
/// slope; treble 250k, bass 1M, mid 25k (a potentiometer). Positions as
/// [`fender_5f6a_tone_stack`]'s. Input `vin` at node `i`, output `o`,
/// unloaded.
///
/// # Models
/// The 2203's passive tone network alone, with its drawing's values
/// (the c.1969+ 1959 Super Lead has the same): driven by an ideal source
/// (the amp drives it from a cathode follower) and unloaded (the amp
/// loads it with the 1M master volume, as
/// [`marshall_jcm800_2203`](super::marshall_jcm800::marshall_jcm800_2203)
/// has it).
///
/// # Derivation
/// Marshall drawing 2203-01-60-02: C8 470p, C9 22n, C10 22n, R13 33k,
/// VR5 250k, VR7 1M, VR6 25k.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::real_tone_stacks_match_ngspice`:
/// `Circuit<Dk>` and the closed form at the deck
/// `tone_stack_marshall_2203.cir`'s positions, within 1e-10 dB of ngspice.
///
/// # Cost
/// Datasheet `"Circuit<Dk> tone stack"` (the same network).
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// Positions 0..=1 (asserted); each pot leg at least [`POT_END_OHMS`].
pub fn marshall_2203_tone_stack(treble: f64, bass: f64, mid: f64) -> Netlist {
    yeh_smith(
        "Marshall JCM800 2203 tone stack",
        "470p",
        "22n",
        "22n",
        "33k",
        [250e3, 1e6, 25e3],
        [treble, bass, mid],
    )
}

/// The Fender AB763 tone stack (Deluxe Reverb): 250p, 0.1u bass, 0.047u
/// mid, 100k slope; treble 250k, bass 250k, no mid pot but a fixed 6.8k
/// with the mid cap on its top. Positions as
/// [`fender_5f6a_tone_stack`]'s. Input `vin` at node `i`, output `o`,
/// unloaded.
///
/// # Models
/// The AB763 Deluxe Reverb's passive tone network alone, with its
/// drawing's values: driven by an ideal source (the amp drives it from
/// V1A's plate, about 38k) and unloaded (the amp loads it with the 1M
/// volume pot). The Twin Reverb's AB763 has a 10k mid pot instead.
///
/// # Derivation
/// Fender's "Deluxe Reverb-Amp AB763" drawing; Rob Robinette's "How the
/// AB763 Deluxe Reverb Works" gives the same values. Yeh & Smith's
/// network with the mid pot's wiper at its top.
///
/// # Verified
/// `crates/stave-test/tests/classics_match_ngspice.rs::real_tone_stacks_match_ngspice`:
/// `Circuit<Dk>` and the closed form at the deck
/// `tone_stack_fender_ab763.cir`'s positions, within 1e-10 dB of ngspice.
///
/// # Cost
/// Datasheet `"Circuit<Dk> tone stack"` (the same network).
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// Positions 0..=1 (asserted); each pot leg at least [`POT_END_OHMS`].
pub fn fender_ab763_tone_stack(treble: f64, bass: f64) -> Netlist {
    let (t_bottom, t_top) = legs(250e3, treble);
    let (bass_in, _) = legs(250e3, bass);
    netlist_of(&format!(
        "Fender AB763 tone stack
VIN i 0 DC 0 AC 1
C1 i t 250p
RT1 t o {t_top}
RT2 o b {t_bottom}
R4 i n 100k
C2 n b 100n
RB b j {bass_in}
RM j 0 6.8k
C3 n j 47n
.control
wrdata out vm(o)
.endc"
    ))
}
