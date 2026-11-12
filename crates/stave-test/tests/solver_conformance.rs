//! Every Newton-solved topology converges at the host rate under a hot
//! signal: no solve may end at its iteration cap. (The ngspice tests run
//! oversampled, where per-sample moves are small; a solver that only
//! converges there is a solver that fails in products.)

use stave_dsp::{
    CommonCathode, CommonCathodeConfig, Dempwolf, DiodeClipper, DiodeClipperConfig, OpAmpClipper,
    OpAmpClipperConfig, PairSolver, PowerSupply, PowerSupplyConfig, Shockley, Stage,
};

const RATE: f64 = 48_000.0;

fn hot(n: usize, volts: f64) -> f64 {
    let t = n as f64 / RATE;
    volts
        * ((std::f64::consts::TAU * 110.0 * t).sin()
            + 0.5 * (std::f64::consts::TAU * 2_750.0 * t).sin())
}

fn report(name: &str, solves: u64, iterations: u64, capped: u64) {
    println!(
        "{name}: {:.2} iterations/solve, {capped} capped",
        iterations as f64 / solves as f64
    );
    assert_eq!(capped, 0, "{name} hit its iteration cap");
}

#[test]
fn diode_clipper_converges_at_the_host_rate() {
    let mut s = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::D1N4148,
        solver: PairSolver::default(),
    });
    s.prepare(RATE as f32);
    for n in 0..RATE as usize {
        s.tick(hot(n, 4.0) as f32);
    }
    let st = s.stats();
    report("diode clipper", st.solves, st.iterations, st.capped);
}

#[test]
fn opamp_clipper_converges_at_the_host_rate() {
    let mut s = OpAmpClipper::new(OpAmpClipperConfig {
        feedback_ohms: 551e3,
        ..OpAmpClipperConfig::TS808
    });
    s.prepare(RATE as f32);
    for n in 0..RATE as usize {
        s.tick_f64(hot(n, 1.0));
    }
    let st = s.stats();
    report("op-amp clipper", st.solves, st.iterations, st.capped);
}

#[test]
fn common_cathode_converges_at_the_host_rate() {
    let mut s = CommonCathode::new(CommonCathodeConfig {
        tube: Dempwolf::RSD_1,
        source_ohms: 33e3,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    s.prepare(RATE as f32);
    for n in 0..RATE as usize {
        s.tick(hot(n, 8.0) as f32);
    }
    let st = s.stats();
    report("common cathode", st.solves, st.iterations, st.capped);
}

#[test]
fn power_supply_converges_under_a_swinging_load() {
    let mut s = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
    s.prepare(RATE as f32);
    for n in 0..RATE as usize {
        s.tick_f64(0.05 + 0.04 * (std::f64::consts::TAU * 7.0 * n as f64 / RATE).sin());
    }
    let st = s.stats();
    report("power supply", st.solves, st.iterations, st.capped);
}

#[test]
fn push_pull_feedback_converges_at_the_host_rate() {
    let mut s = stave_dsp::PushPull::new();
    s.prepare(RATE as f32);
    s.set_drive_db(18.0);
    s.set_feedback_db(12.0);
    for n in 0..RATE as usize {
        s.tick(hot(n, 1.0) as f32);
    }
    let st = s.stats();
    report("push-pull feedback", st.solves, st.iterations, st.capped);
}
