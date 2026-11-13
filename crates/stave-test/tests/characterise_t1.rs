//! T1 of the characterisation matrix (DSP.md R17): the whole grid —
//! the multisine at every offset from the knee (C3), tones at 82 Hz to
//! 8 kHz across the level grid (C4–C6, C10), SMPTE and CCIF pairs (C7)
//! and bursts (C8) — oversampled and at 44.1, 48 and 96 kHz (C11: the
//! same budgets at every host rate). Ignored by default: the references
//! job runs it (`cargo test -p stave-test --test characterise_t1 --
//! --ignored`), beside the ngspice regeneration it is held to.

#[allow(dead_code)]
mod circuits;

use circuits::{budgets, CIRCUITS};
use stave_measure::aliasing::{anmr, SteppedTone};
use stave_measure::perceptual::nmr::AUDIBLE_ANMR_DB;
use stave_test::characterise::model::{circuit as solver, knee};
use stave_test::characterise::{deck, Characterise, Plan, Tier};

fn circuit(name: &str) {
    budgets(name, Tier::T1, Characterise::ngspice(name).tier(Tier::T1).circuit())
        .run()
        .assert();
}

/// The knees the plans were built at are still the circuits' knees,
/// to 0.2 dB (the plan rounds them to 0.1 dB).
#[test]
#[ignore = "T1: the references job runs it"]
fn knees_are_current() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    for c in CIRCUITS.iter().filter(|c| !c.supply) {
        let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{}.plan", c.name))).unwrap())
            .unwrap();
        let committed = plan.knee.unwrap();
        let now = knee(&deck(c.name), &plan.input, plan.settle_seconds);
        let moved = 20.0 * (now.amplitude / committed.amplitude).log10();
        println!("{}: knee {:.4e} V ({:?}), moved {moved:+.2} dB", c.name, now.amplitude, now.criterion);
        assert!(moved.abs() <= 0.2, "{}: the knee moved {moved:+.2} dB: re-bless the plan", c.name);
    }
}

macro_rules! t1 {
    ($($name:ident),*) => {$(
        #[test]
        #[ignore = "T1: the references job runs it"]
        fn $name() {
            circuit(stringify!($name));
        }
    )*};
}

t1!(
    diode_clipper,
    opamp_clipper,
    tube_screamer,
    common_cathode,
    common_cathode_grid_current,
    triode_stage,
    push_pull,
    push_pull_pentode,
    power_supply,
    fuzz_face,
    rangemaster,
    ce_stage,
    jfet_booster,
    mosfet_booster,
    ota_vca,
    fuzz_face_static,
    rangemaster_static,
    ce_stage_static,
    jfet_booster_static,
    mosfet_booster_static
);

/// The hand topologies' whole matrices.
mod hand {
    use super::circuits::hand;
    use stave_test::characterise::Tier;

    macro_rules! t1 {
        ($($name:ident: $case:literal),*) => {$(
            #[test]
            #[ignore = "T1: the references job runs it"]
            fn $name() {
                hand($case, Tier::T1);
            }
        )*};
    }

    t1!(
        diode_clipper: "diode_clipper/hand",
        opamp_clipper: "opamp_clipper/hand",
        common_cathode: "common_cathode/hand",
        common_cathode_grid_current: "common_cathode_grid_current/hand",
        power_supply: "power_supply/hand"
    );
}

/// C10 by audibility (DSP.md, "Aliasing budget by audibility"): each
/// circuit's `Circuit<Dk>` at 44.1 kHz, no oversampling — the worst
/// host rate — with a tone on every semitone of the piano (27.5 Hz to
/// 4186 Hz; the 8192-point window merges neighbouring keys in the lowest
/// octave) at +12 and +20 dB re its knee: the worst step's ANMR
/// (Kahles et al. 2019) held to −10 dB, inaudible. A circuit that
/// aliases audibly at the host rate is a finding, held to its recorded
/// worst + 3 dB (it needs oversampling or ADAA to ship): see
/// [`PIANO`].
#[test]
#[ignore = "T1: the references job runs it"]
fn anmr_over_the_piano() {
    let rate = 44_100.0;
    let results: Vec<(&str, f64, f64, f64)> = std::thread::scope(|scope| {
        let jobs: Vec<_> = CIRCUITS
            .iter()
            .filter(|c| !c.supply)
            .map(|c| {
                scope.spawn(move || {
                    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
                    let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{}.plan", c.name))).unwrap()).unwrap();
                    let deck = deck(c.name);
                    let knee = plan.knee.unwrap();
                    let worst = |offset: f64| {
                        let sweep = SteppedTone {
                            amplitude: knee.at(offset),
                            // Past the slowest settling (≤ 50 ms) in one window.
                            warmup_windows: 1,
                            ..SteppedTone::piano(rate, 88)
                        };
                        let base = solver(&deck, &plan.input, rate);
                        sweep
                            .bins()
                            .into_iter()
                            .map(|bin| {
                                let mut c = base.clone();
                                let n = sweep.window;
                                let total = (sweep.warmup_windows + 1) * n;
                                let mut out = Vec::with_capacity(n);
                                for i in 0..total {
                                    let x = sweep.amplitude * (std::f64::consts::TAU * (bin * (i % n)) as f64 / n as f64).sin();
                                    let y = if i == 0 { c.output() } else { c.tick_f64(x) };
                                    if i >= total - n {
                                        out.push(y);
                                    }
                                }
                                (anmr(&out, bin, rate), bin as f64 * rate / n as f64)
                            })
                            .fold((f64::NEG_INFINITY, 0.0), |a, b| if b.0 > a.0 { b } else { a })
                    };
                    let (a12, hz12) = worst(12.0);
                    let (a20, hz20) = worst(20.0);
                    let (w, hz) = if a12 > a20 { (a12, hz12) } else { (a20, hz20) };
                    println!("{}: worst ANMR {a12:.1} dB at +12, {a20:.1} dB at +20 (at {hz:.0} Hz)", c.name);
                    (c.name, a12, a20, w)
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let mut over = Vec::new();
    for (name, a12, a20, worst) in results {
        let budget = PIANO.iter().find(|p| p.0 == name).map_or(AUDIBLE_ANMR_DB, |p| p.1);
        if worst > budget {
            over.push(format!("{name}: {a12:.1} / {a20:.1} dB at +12 / +20, budget {budget:.1} dB"));
        }
    }
    assert!(over.is_empty(), "ANMR over budget:\n  {}", over.join("\n  "));
}

/// The circuits that alias audibly at 44.1 kHz with no oversampling
/// (findings, DSP.md R19): `(circuit, budget, recorded worst)`, the
/// budget 3 dB above the recording. Every other circuit is held to
/// −10 dB: the common cathode (−65.3 / −28.8 dB at +12 / +20), the Fuzz
/// Face (−116.8 / −70.4; −81.7 at +20 on its static deck) and the
/// Rangemaster (−64.2 / −28.8; −24.3 static). Measured 2026-11-13: every
/// worst step is the top of the piano, 4.2 kHz, where a hot tone's sixth
/// harmonic and above fold back below 20 kHz.
const PIANO: &[(&str, f64, f64)] = &[
    ("diode_clipper", 54.0, 51.0), // 38.3 / 51.0 dB at +12 / +20
    ("opamp_clipper", 49.7, 46.7), // 35.7 / 46.7 dB at +12 / +20
    ("tube_screamer", 39.0, 36.0), // 4.8 / 36.0 dB at +12 / +20
    ("common_cathode_grid_current", 40.0, 37.0), // -0.6 / 37.0 dB at +12 / +20
    ("triode_stage", 30.7, 27.7), // -49.6 / 27.7 dB at +12 / +20
    ("push_pull", 45.1, 42.1), // -20.8 / 42.1 dB at +12 / +20
    ("push_pull_pentode", 30.2, 27.2), // -8.1 / 27.2 dB at +12 / +20
    ("ce_stage", 45.3, 42.3), // 31.6 / 42.3 dB at +12 / +20
    ("jfet_booster", 36.0, 33.0), // -6.6 / 33.0 dB at +12 / +20
    ("mosfet_booster", 58.7, 55.7), // 42.4 / 55.7 dB at +12 / +20
    ("ce_stage_static", 45.3, 42.3), // 31.6 / 42.3 dB at +12 / +20
    ("jfet_booster_static", 36.0, 33.0), // -6.6 / 33.0 dB at +12 / +20
    ("mosfet_booster_static", 59.1, 56.1), // 42.5 / 56.1 dB at +12 / +20
    ("ota_vca", 40.2, 37.2), // -7.6 / 37.2 dB at +12 / +20
];
