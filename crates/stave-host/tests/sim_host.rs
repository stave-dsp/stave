//! The deterministic simulated host (OVERHAUL.md V3) against a real
//! stave plugin (Tilt) through the full VST3 path: seeded call orders,
//! block sizes, sample rates, buffer shapes, parameter and state
//! traffic, legal and not. A failure prints its seed;
//!
//!   STAVE_SIM_SEED=<seed> cargo test -p stave-host --test sim_host
//!
//! replays exactly that run. `STAVE_SIM_SEEDS=<n>` widens the sweep
//! (CI's nightly runs more).

use stave_host::plugin::sim::{simulate, SimConfig};
use stave_model::check::validate;
use stave_model::lifecycle::{Abstract, Compliant, Lifecycle};

fn tilt() -> stave_host::plugin::EntryPoints {
    stave_host::entry_points!(stave_tilt_vst3)
}

fn seeds() -> Vec<u64> {
    let env = |name| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
    if let Some(seed) = env("STAVE_SIM_SEED") {
        return vec![seed];
    }
    (0..env("STAVE_SIM_SEEDS").unwrap_or(48)).collect()
}

#[test]
#[cfg_attr(miri, ignore = "hundreds of full plugin lifecycles")]
fn every_seed_keeps_every_law() {
    for seed in seeds() {
        if let Err(failure) = simulate(tilt(), seed, SimConfig::default()) {
            panic!("{failure}");
        }
    }
}

#[test]
#[cfg_attr(miri, ignore = "full plugin lifecycles")]
fn a_compliant_host_keeps_every_law() {
    let config = SimConfig {
        illegal_per_16: 0,
        ..SimConfig::default()
    };
    for seed in 0..16 {
        let report = simulate(tilt(), seed, config).unwrap_or_else(|f| panic!("{f}"));
        // Trace validation: the run is a behaviour of the lifecycle model.
        let start = Abstract::from(&Lifecycle::active(48_000.0, 512));
        if let Err(d) = validate(&Compliant, start, &report.calls) {
            panic!("seed {seed}: the host's own trace left the model: {d:?}");
        }
    }
}

/// One seed, one run: the same calls and the same output, bit for bit.
#[test]
#[cfg_attr(miri, ignore = "full plugin lifecycles")]
fn a_seed_replays_exactly() {
    let a = simulate(tilt(), 7, SimConfig::default()).expect("seed 7 passes");
    let b = simulate(tilt(), 7, SimConfig::default()).expect("seed 7 passes");
    assert_eq!(a, b);
    assert!(a.events > 0, "the plugin's event trace was read and validated");
    let c = simulate(tilt(), 8, SimConfig::default()).expect("seed 8 passes");
    assert_ne!(a.digest, c.digest, "different seeds, different runs");
}
