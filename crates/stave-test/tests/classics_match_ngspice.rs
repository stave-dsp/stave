//! Classic circuits as verified decks and topologies (DSP.md R24): each
//! unit's ngspice deck (`spice/*.cir`, sources cited in its header) runs
//! through `Circuit<Dk>` as ngspice runs it, and the named netlist in
//! `circuit::topologies` — stave's own op-amp and triode elements where
//! the deck writes them as behavioural sources — runs on the deck's
//! stimulus against the same reference. Budgets sit a little above the
//! measured error (each test's doc gives it); every solve's health is
//! printed and no solve may hit its cap.

use std::path::PathBuf;

use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::netlist::{ElementKind, Netlist};
use stave_dsp::circuit::topologies;
use stave_dsp::circuit::Circuit;
use stave_test::reference::{assert_converges, Case, Metric, Outcome, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// `netlist` at `oversample` × 48 kHz, read at the reference `name`'s
/// instants (sample 0 is the operating point).
fn transient(name: &str, netlist: &Netlist, oversample: u32) -> (Vec<f64>, Health) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Dk>::new(netlist).unwrap();
    circuit.prepare(rate as f32);
    let (mut n, mut out) = (0u64, circuit.output());
    let run = reference
        .abscissa()
        .iter()
        .map(|&t| {
            while n < (t * rate).round() as u64 {
                out = circuit.step(0.0);
                n += 1;
            }
            out
        })
        .collect();
    (run, circuit.health())
}

fn healthy(name: &str, health: &Health) {
    println!(
        "  DK solve: {:.2} iterations/solve, {} of {} capped, {} continuations",
        health.newton.mean_iterations(),
        health.newton.capped,
        health.newton.solves,
        health.continuations
    );
    assert_eq!(health.newton.capped, 0, "{name} hit its iteration cap");
}

/// The topology with the deck's stimulus on its input source `source`.
fn stimulated(mut topology: Netlist, name: &str, source: &str) -> Netlist {
    let deck = deck(name);
    let Some(ElementKind::VoltageSource {
        source: stimulus, ..
    }) = deck.element(source).map(|e| &e.kind)
    else {
        panic!("{name}: no voltage source {source}")
    };
    let Some(ElementKind::VoltageSource { source: input, .. }) =
        topology.element_mut(source).map(|e| &mut e.kind)
    else {
        panic!("the topology has no voltage source {source}")
    };
    *input = stimulus.clone();
    topology
}

/// `netlist` (the deck, or a topology on its stimulus) against the deck
/// `name`'s reference at `oversample`, with the solve's health.
fn run(name: &str, label: &str, netlist: &Netlist, oversample: u32, case: Case) -> Outcome {
    let (run, health) = transient(name, netlist, oversample);
    let outcome = case
        .named(format!("{name}/dk {oversample}x, {label}"))
        .compare(&run);
    healthy(name, &health);
    outcome
}

/// The Big Muff Pi (Ram's Head), 200 mV at 220 Hz: every stage clips.
/// The deck at 8× (measured 2.9e-4); the topology, the deck's netlist,
/// at 8× and 16×, its error falling as h² (2.9e-4 to 6.7e-5; 1.6e-5 at
/// 32×, 3.7e-6 at 64×: what is left is the trapezoidal rule's).
#[test]
fn big_muff_rams_head_matches_ngspice() {
    let name = "big_muff_rams_head";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 8, case(4e-4)).assert();
    let topology = stimulated(topologies::big_muff_rams_head(), name, "vin");
    let coarse = run(name, "topology", &topology, 8, case(4e-4));
    let fine = run(name, "topology", &topology, 16, case(1e-4));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The Big Muff's tone stack alone at noon: the topology's and the deck's
/// `Circuit<Dk>` response against ngspice's AC analysis.
#[test]
fn big_muff_tone_stack_matches_ngspice() {
    let name = "big_muff_tone_stack";
    for (label, netlist) in [("topology", topologies::big_muff_tone_stack(0.5)), ("the deck", deck(name))] {
        let circuit = Circuit::<Dk>::new(&netlist).unwrap();
        Case::ngspice(name)
            .named(format!("{name}/dk, {label}"))
            .budget(Metric::MaxDb, 1e-10)
            .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
            .assert();
    }
}

/// The TS-808, 300 mV at 1 kHz: the deck's behavioural JRC4558s at 16×
/// (measured 1.4e-5), then the topology's `OpAmp` elements at 8× and
/// 16×, falling as h² (5.4e-5 to 1.4e-5).
#[test]
fn tube_screamer_ts808_matches_ngspice() {
    let name = "tube_screamer_ts808";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 16, case(3e-5)).assert();
    let topology = stimulated(topologies::tube_screamer_ts808(), name, "vin");
    let coarse = run(name, "topology", &topology, 8, case(1e-4));
    let fine = run(name, "topology", &topology, 16, case(3e-5));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The RAT, 100 mV at 1 kHz into a gain of about 2300: the LM308 slews
/// at 0.3 V/µs and the 1N914s clip. A slewing integrator needs a fine
/// grid: the deck at 32× (measured 3.7e-5), the topology at 32× and 64×,
/// falling as h² (3.7e-5 to 8.2e-6; 2.7e-3 at 4×, 1.5e-4 at 16×).
#[test]
fn proco_rat_matches_ngspice() {
    let name = "proco_rat";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 32, case(6e-5)).assert();
    let topology = stimulated(topologies::proco_rat(), name, "vin");
    let coarse = run(name, "topology", &topology, 32, case(6e-5));
    let fine = run(name, "topology", &topology, 64, case(1.5e-5));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The JCM800 2203 preamp, 0.5 V at 1 kHz: the deck's behavioural
/// Dempwolf triodes at 8× (measured 5.2e-4), then the topology's triode
/// elements at 8× and 16×, falling as h² (5.2e-4 to 1.0e-4). Past 32× it
/// meets the reference's own floor, 6.7e-5: the cold clipper's edges
/// swing 60 V within a 48 kHz sample, where ngspice's 0.25 µs steps are
/// interpolated.
#[test]
fn marshall_jcm800_2203_matches_ngspice() {
    let name = "marshall_jcm800_2203";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 8, case(7e-4)).assert();
    let topology = stimulated(topologies::marshall_jcm800_2203(), name, "vin");
    let coarse = run(name, "topology", &topology, 8, case(7e-4));
    let fine = run(name, "topology", &topology, 16, case(2e-4));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The real tone stacks: the topology's and the deck's `Circuit<Dk>`
/// response (the bilinear prewarp, exact for a linear circuit) and Yeh &
/// Smith's closed form with the same values, within 1e-10 dB of
/// ngspice's AC analysis.
#[test]
fn real_tone_stacks_match_ngspice() {
    use stave_dsp::{PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents};
    let stacks = [
        (
            "tone_stack_fender_5f6a",
            topologies::fender_5f6a_tone_stack(0.5, 0.5, 0.5),
            (250e-12, 20e-9, 20e-9, 56e3, 1e6, 25e3),
            (0.5, 0.5, 0.5),
        ),
        (
            "tone_stack_marshall_2203",
            topologies::marshall_2203_tone_stack(0.5, 0.5, 0.5),
            (470e-12, 22e-9, 22e-9, 33e3, 1e6, 25e3),
            (0.5, 0.5, 0.5),
        ),
        // The AB763's fixed 6.8k is the network's mid pot with the mid
        // cap on its top: a 6.8k "pot" at position 1.
        (
            "tone_stack_fender_ab763",
            topologies::fender_ab763_tone_stack(0.5, 0.5),
            (250e-12, 100e-9, 47e-9, 100e3, 250e3, 6.8e3),
            (0.5, 0.5, 1.0),
        ),
    ];
    for (name, topology, (c1, c2, c3, slope, bass, mid), (t, l, m)) in stacks {
        for (label, netlist) in [("topology", topology), ("the deck", deck(name))] {
            let circuit = Circuit::<Dk>::new(&netlist).unwrap();
            Case::ngspice(name)
                .named(format!("{name}/dk, {label}"))
                .budget(Metric::MaxDb, 1e-10)
                .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
                .assert();
        }
        let mut stack = PassiveToneStack::new(PassiveToneStackComponents {
            treble_capacitance_farads: c1,
            bass_capacitance_farads: c2,
            midrange_capacitance_farads: c3,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: bass,
            midrange_potentiometer_ohms: mid,
            slope_resistance_ohms: slope,
        });
        stack.set_controls(PassiveToneControlPositions::new(t, l, m));
        Case::ngspice(name)
            .named(format!("{name}, closed form"))
            .budget(Metric::MaxDb, 1e-10)
            .pointwise(|hz| stack.circuit_magnitude_at(hz))
            .assert();
    }
}

/// The Moog 904-A ladder core: open loop, `Circuit<Dk>`'s small-signal
/// response against ngspice's AC analysis (measured 1.9e-5 dB: the two
/// operating points' last digits); with regeneration k = 3 and driven
/// 150 mV, the deck and the topology against ngspice's transient
/// at 16× (measured 1.3e-5, falling as h²: 8.0e-4 at 2×, 2.0e-4 at 4×,
/// 5.0e-5 at 8×, 3.4e-6 at 32×). Then the loop's sense, which matching
/// ngspice can't see when deck and topology share it (DSP.md R26): the
/// open-loop core is non-inverting (DC gain +1.014), so the regeneration
/// `v(sig) − k·v(out)` is negative feedback — the closed loop at k = 3
/// settles at G / (1 + 3G), 0.251 — and it self-oscillates where a
/// ladder should, at k ≈ 4 (measured k = 3.951 at 1980 Hz; the ideal
/// corner `I/(8π·Vt·C)` is 2052 Hz).
#[test]
fn moog_ladder_904a_matches_ngspice() {
    let ac = [("topology", topologies::moog_ladder_904a(0.0)), ("the deck", deck("moog_ladder_ac"))];
    for (label, netlist) in ac {
        let circuit = Circuit::<Dk>::new(&netlist).unwrap();
        Case::ngspice("moog_ladder_ac")
            .named(format!("moog_ladder_ac/dk, {label}"))
            .budget(Metric::MaxDb, 5e-5)
            .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
            .assert();
    }
    let name = "moog_ladder_feedback";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 16, case(2e-5)).assert();
    let topology = stimulated(topologies::moog_ladder_904a(3.0), name, "vin");
    let coarse = run(name, "topology", &topology, 8, case(8e-5));
    let fine = run(name, "topology", &topology, 16, case(2e-5));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();

    let dc_gain = |k: f64| {
        let circuit = Circuit::<Dk>::new(&topologies::moog_ladder_904a(k)).unwrap();
        let at = circuit.analog_response(1.0).unwrap();
        assert!(at.im.abs() < 1e-2 * at.re.abs(), "k = {k}: not at DC yet");
        at.re
    };
    let open = dc_gain(0.0);
    let closed = dc_gain(3.0);
    println!("904-A: DC gain {open:.4} open loop, {closed:.4} at k = 3");
    assert!((1.0..1.03).contains(&open), "open-loop DC gain {open}: the core must not invert");
    assert!((closed / (open / (1.0 + 3.0 * open)) - 1.0).abs() < 1e-3, "k = 3 is not negative feedback: {closed}");
    let (onset, pitch) = onset_and_pitch(&topologies::moog_ladder_904a(0.0));
    println!("904-A: self-oscillates at k = {onset:.4}, {pitch:.1} Hz");
    assert!((3.9..4.1).contains(&onset), "onset at k = {onset}");
    assert!((1900.0..2100.0).contains(&pitch), "pitch {pitch} Hz");
}

/// Where a ladder's open loop (regeneration 0) reaches −180°: the
/// regeneration at which it self-oscillates (one over the gain there) and
/// its pitch (that frequency), from `Circuit<Dk>`'s small-signal response.
fn onset_and_pitch(netlist: &Netlist) -> (f64, f64) {
    let circuit = Circuit::<Dk>::new(netlist).unwrap();
    let at = |hz: f64| circuit.analog_response(hz).unwrap();
    // A lowpass of positive DC gain is negative real at −180°: its
    // imaginary part turns from negative to positive there.
    let (mut lo, mut hi) = (100.0f64, 100.0f64);
    while at(hi).im < 0.0 {
        lo = hi;
        hi *= 1.01;
        assert!(hi < 1e5, "no −180° crossing");
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if at(mid).im < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let pitch = 0.5 * (lo + hi);
    (1.0 / at(pitch).magnitude(), pitch)
}

/// `netlist`'s linearised response against the AC deck `name`'s
/// ngspice analysis, within `budget` dB.
fn response(name: &str, label: &str, netlist: &Netlist, budget: f64) {
    let circuit = Circuit::<Dk>::new(netlist).unwrap();
    Case::ngspice(name)
        .named(format!("{name}/dk, {label}"))
        .budget(Metric::MaxDb, budget)
        .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
        .assert();
}

/// The Boss CE-2's BBD filters, the two 2N5088 stages with the BBD a
/// unity copy: the topology's and the deck's linearised response against
/// ngspice's AC analysis, 10 Hz to 50 kHz (measured 3.3e-6 dB, at 50 kHz,
/// 109 dB down; the passband's error is below 1e-9 dB).
#[test]
fn boss_ce2_filters_match_ngspice() {
    let name = "boss_ce2_filters";
    response(name, "topology", &topologies::boss_ce2_filters(), 1e-5);
    response(name, "the deck", &deck(name), 1e-5);
}

/// The MXR Phase 90 with its gates frozen at 3.4 V: the linearised
/// response of the topology and the AC deck against ngspice (measured
/// 9.7e-6 dB); then 300 mV at 1 kHz, the deck's behavioural LM741s at 16×
/// (measured 3.0e-5) and the topology's `OpAmp` elements at 8× and 16×
/// against ngspice's transient, falling as h² (1.3e-4 to 3.0e-5; 7.3e-6
/// at 32×, 1.6e-6 at 64×).
#[test]
fn mxr_phase90_matches_ngspice() {
    let ac = "mxr_phase90_ac";
    response(ac, "topology", &topologies::mxr_phase90(3.4), 3e-5);
    response(ac, "the deck", &deck(ac), 3e-5);
    let name = "mxr_phase90";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 16, case(6e-5)).assert();
    let topology = stimulated(topologies::mxr_phase90(3.4), name, "vin");
    let coarse = run(name, "topology", &topology, 8, case(3e-4));
    let fine = run(name, "topology", &topology, 16, case(6e-5));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The MXR Phase 90 swept: its gates driven by a 4 Hz sine from 3.1 to
/// 4.3 V (one whole sweep in 0.25 s, standing in for the LFO), 300 mV at
/// 1 kHz in, so the notches cross the tone and the channels move within
/// each of its periods. The deck's behavioural LM741s at 16× (measured
/// 9.2e-6), then the topology, its `vg` carrying the deck's sweep, at 8×
/// and 16× against ngspice's transient, falling as h² (3.9e-5 to 9.2e-6;
/// 1.6e-4 at 4×, 2.1e-6 at 32×): the time-varying case holds as the
/// frozen one does.
#[test]
fn mxr_phase90_swept_matches_ngspice() {
    let name = "mxr_phase90_sweep";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 16, case(2e-5)).assert();
    let topology = stimulated(stimulated(topologies::mxr_phase90(3.4), name, "vin"), name, "vg");
    let coarse = run(name, "topology", &topology, 8, case(8e-5));
    let fine = run(name, "topology", &topology, 16, case(2e-5));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The Boss DS-1, 200 mV at 1 kHz: the booster saturates, the NJM2904
/// rails and the 1N4148s clip — edges a coarse grid smears, so it needs
/// a fine one: the deck's behavioural op-amps at 32× (measured 1.4e-4),
/// then the topology's `OpAmp` elements at 16× and 32×, falling as h²
/// (6.9e-4 to 1.4e-4; 3.1e-3 at 8×, 3.6e-5 at 64×).
#[test]
fn boss_ds1_matches_ngspice() {
    let name = "boss_ds1";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 32, case(3e-4)).assert();
    let topology = stimulated(topologies::boss_ds1(), name, "vin");
    let coarse = run(name, "topology", &topology, 16, case(1.5e-3));
    let fine = run(name, "topology", &topology, 32, case(3e-4));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The Klon Centaur's clean/dry blend: at gain 0 (the clean networks
/// alone, gang A shorting the gain stage's input; measured 2.3e-9 dB) and
/// gain 1 (the dirty path with network 1, gang B shorting network 2;
/// 2.2e-6 dB, at 10 Hz), the topology's and the decks' linearised
/// response against ngspice's AC analysis; then at noon, 300 mV at 1 kHz,
/// the deck's behavioural TL072s at 8× (measured 4.2e-5) and the
/// topology's `OpAmp` elements at 4× and 8× against ngspice's transient,
/// falling as h² (1.7e-4 to 4.2e-5; 1.0e-5 at 16×, 5.0e-7 at 64×).
#[test]
fn klon_centaur_matches_ngspice() {
    for (name, gain, budget) in [("klon_centaur_clean", 0.0, 1e-8), ("klon_centaur_ac_max", 1.0, 1e-5)] {
        response(name, "topology", &topologies::klon_centaur(gain, 0.5), budget);
        response(name, "the deck", &deck(name), budget);
    }
    let name = "klon_centaur";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 8, case(1e-4)).assert();
    let topology = stimulated(topologies::klon_centaur(0.5, 0.5), name, "vin");
    let coarse = run(name, "topology", &topology, 4, case(4e-4));
    let fine = run(name, "topology", &topology, 8, case(1e-4));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}

/// The MXR Distortion+: at distortion 0.5 the linearised response of the
/// topology and the AC deck against ngspice (measured 1.5e-6 dB); then
/// both pots at maximum, 100 mV at 1 kHz into a gain of 214 (the 741
/// rails and slews, the germanium pair clips), the deck's behavioural
/// LM741 at 32× (measured 1.05e-4) and the topology's `OpAmp` element at
/// 16× and 32× against ngspice's transient, falling as h² (4.4e-4 to
/// 1.05e-4; 6.2e-3 at 4×, 1.8e-3 at 8×, 2.5e-5 at 64×).
#[test]
fn mxr_distortion_plus_matches_ngspice() {
    let ac = "mxr_distortion_plus_ac";
    response(ac, "topology", &topologies::mxr_distortion_plus(0.5, 1.0), 5e-6);
    response(ac, "the deck", &deck(ac), 5e-6);
    let name = "mxr_distortion_plus";
    let case = |budget| Case::ngspice(name).budget(Metric::NormalizedRms, budget);
    run(name, "the deck", &deck(name), 32, case(2e-4)).assert();
    let topology = stimulated(topologies::mxr_distortion_plus(1.0, 1.0), name, "vin");
    let coarse = run(name, "topology", &topology, 16, case(1e-3));
    let fine = run(name, "topology", &topology, 32, case(2e-4));
    assert_converges(&coarse, &fine, 3.0);
    fine.assert();
}
