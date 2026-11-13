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
use stave_test::characterise::model::knee;
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
    ota_vca
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
        diode_clipper: "diode_clipper/wdf",
        opamp_clipper: "opamp_clipper/hand",
        common_cathode: "common_cathode/hand",
        common_cathode_grid_current: "common_cathode_grid_current/hand",
        power_supply: "power_supply/hand"
    );
}
