//! T0 of the characterisation matrix (DSP.md R17, "Measurement design"):
//! every circuit with an ngspice deck held to ngspice on C1 (operating
//! point), C2 (the small-signal response, magnitude and phase), C3 (the
//! best linear approximation at −40, 0 and +12 dB re its knee), C4–C6
//! (a tone near 1 kHz at −20, 0 and +12: gain, each harmonic H2–H15 in
//! magnitude and phase, THD+N) and C10 (aliasing), oversampled and at
//! 48 kHz — each with its own budget. T1, the whole grid at 44.1, 48 and
//! 96 kHz too, is `characterise_t1.rs`, run by the references job.
//!
//! The plans (`spice/<name>.plan`) are written here from the circuits in
//! `circuits/`: `STAVE_BLESS=1 cargo test -p stave-test --test
//! characterise plans` finds each knee and settling time on
//! `Circuit<Dk>` and rewrites them; then `scripts/ngspice-simulate.py`
//! simulates them into `spice/<name>.table`.
//!
//! Budgets are the measured error with a margin of two to three — a
//! regression trips them — except where a model fails a characteristic
//! outright; those are recorded at the measured value with the finding
//! beside them (DSP.md R17).

#[allow(dead_code)]
mod circuits;

use circuits::{budgets, CIRCUITS};
use stave_test::characterise::model::{knee, settle_seconds};
use stave_test::characterise::{deck, Characterise, Plan, Tier};

fn bless() -> bool {
    std::env::var_os("STAVE_BLESS").is_some()
}

/// Every committed plan is the one its circuit's input, knee and
/// settling time give now (so a change to the suite or the plan builder
/// shows as a stale plan); `STAVE_BLESS=1` re-derives knee and settling
/// and rewrites them.
#[test]
fn plans_are_current() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    let mut stale = Vec::new();
    for c in CIRCUITS.iter() {
        let path = dir.join(format!("{}.plan", c.name));
        let committed = std::fs::read_to_string(&path).ok();
        let input = (c.input)();
        if bless() {
            let deck = deck(c.name);
            let settle = c.settle.unwrap_or_else(|| settle_seconds(&deck, &input));
            let plan = if c.supply {
                Plan::supply(c.name, input, settle)
            } else {
                Plan::circuit(c.name, input.clone(), knee(&deck, &input, settle), settle)
            };
            std::fs::write(&path, plan.describe()).unwrap();
            println!("{}: wrote {} runs", c.name, plan.runs.len());
            continue;
        }
        let Some(text) = &committed else {
            stale.push(format!("{}: no plan", c.name));
            continue;
        };
        let old = Plan::parse(text).unwrap();
        assert_eq!(old.input, input, "{}: the plan's input is not the circuit's", c.name);
        let now = if c.supply {
            Plan::supply(c.name, old.input, old.settle_seconds)
        } else {
            Plan::circuit(c.name, old.input, old.knee.unwrap(), old.settle_seconds)
        };
        // Line endings as committed: a Windows checkout may hand back CRLF.
        if text.replace("\r\n", "\n") != now.describe() {
            stale.push(c.name.to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "stale plans: {stale:?} (STAVE_BLESS=1 cargo test -p stave-test --test characterise \
         plans, then scripts/ngspice-simulate.py)"
    );
}

/// `name`'s T0 matrix on `Circuit<Dk>`, with its budgets.
fn circuit(name: &str) {
    budgets(name, Tier::T0, Characterise::ngspice(name).tier(Tier::T0).circuit())
        .run()
        .assert();
}

#[test]
fn diode_clipper() {
    circuit("diode_clipper");
}

#[test]
fn opamp_clipper() {
    circuit("opamp_clipper");
}

#[test]
fn tube_screamer() {
    circuit("tube_screamer");
}

#[test]
fn common_cathode() {
    circuit("common_cathode");
}

#[test]
fn common_cathode_grid_current() {
    circuit("common_cathode_grid_current");
}

#[test]
fn triode_stage() {
    circuit("triode_stage");
}

#[test]
fn push_pull() {
    circuit("push_pull");
}

#[test]
fn push_pull_pentode() {
    circuit("push_pull_pentode");
}

#[test]
fn power_supply() {
    circuit("power_supply");
}

#[test]
fn fuzz_face() {
    circuit("fuzz_face");
}

#[test]
fn rangemaster() {
    circuit("rangemaster");
}

#[test]
fn ce_stage() {
    circuit("ce_stage");
}

#[test]
fn jfet_booster() {
    circuit("jfet_booster");
}

#[test]
fn mosfet_booster() {
    circuit("mosfet_booster");
}

#[test]
fn ota_vca() {
    circuit("ota_vca");
}

/// `Circuit<Wdf>` on every circuit's plan at `Circuit<Dk>`'s T0
/// budgets: the wave digital method (trees under an R-type root, the
/// devices a K-method root) held to ngspice on the whole matrix — both
/// methods discretise by the bilinear transform, so the same budgets hold.
#[test]
fn every_circuit_on_wdf() {
    for c in CIRCUITS.iter() {
        budgets(c.name, Tier::T0, Characterise::ngspice(c.name).tier(Tier::T0).wdf())
            .named(format!("{}/wdf", c.name))
            .run()
            .assert();
    }
}

/// The hand topologies on the same plans: each ships as its own type,
/// so each is held to ngspice directly, not only to `Circuit<Dk>`.
mod hand {
    use super::circuits::hand;
    use stave_test::characterise::Tier;

    #[test]
    fn diode_clipper() {
        hand("diode_clipper/hand", Tier::T0);
    }

    #[test]
    fn opamp_clipper() {
        hand("opamp_clipper/hand", Tier::T0);
    }

    #[test]
    fn common_cathode() {
        hand("common_cathode/hand", Tier::T0);
    }

    #[test]
    fn common_cathode_grid_current() {
        hand("common_cathode_grid_current/hand", Tier::T0);
    }

    #[test]
    fn power_supply() {
        hand("power_supply/hand", Tier::T0);
    }
}
