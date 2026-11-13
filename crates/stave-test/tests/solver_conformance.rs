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
        diode: Shockley::N4148,
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

/// E3: the hand-derived pair with EL34s, under 12 dB of feedback.
#[test]
fn push_pull_pentode_feedback_converges_at_the_host_rate() {
    let mut s = stave_dsp::PushPull::new();
    s.set_pentode(stave_dsp::circuit::topologies::push_pull::PentodePair::EL34);
    s.prepare(RATE as f32);
    s.set_drive_db(36.0);
    s.set_feedback_db(12.0);
    for n in 0..RATE as usize {
        s.tick(hot(n, 1.0) as f32);
    }
    let st = s.stats();
    report("EL34 push-pull feedback", st.solves, st.iterations, st.capped);
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

/// `Circuit<Dk>` on the same circuits, from their netlists, at the host
/// rate under the same hot signals: no solve may end at its cap, and the
/// continuation fallback's use is printed.
mod dk {
    use super::{hot, RATE};
    use stave_dsp::circuit::methods::dk::Dk;
    use stave_dsp::circuit::netlist::Netlist;
    use stave_dsp::circuit::Circuit;

    fn deck(name: &str) -> String {
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
        std::fs::read_to_string(path).unwrap()
    }

    fn run(name: &str, text: &str, source: &str, mut input: impl FnMut(usize) -> f64) {
        let mut c = Circuit::<Dk>::new(&Netlist::parse_spice(text).unwrap()).unwrap();
        c.drive(source).unwrap();
        c.prepare(RATE as f32);
        for n in 0..RATE as usize {
            assert!(c.tick_f64(input(n)).is_finite(), "{name}: sample {n}");
        }
        let h = c.health();
        println!(
            "{name} (DK): {:.2} iterations/solve, {} capped, {} continuations",
            h.newton.mean_iterations(),
            h.newton.capped,
            h.continuations
        );
        assert_eq!(h.newton.capped, 0, "{name} (DK) hit its iteration cap");
    }

    #[test]
    fn diode_clipper_converges_at_the_host_rate() {
        let text = deck("diode_clipper").replace("V1 in 0 SIN(0 1 1k)", "V1 in 0 0");
        run("diode clipper", &text, "v1", |n| hot(n, 4.0));
    }

    #[test]
    fn opamp_clipper_converges_at_the_host_rate() {
        let text = deck("opamp_clipper")
            .replace("V1 in 0 DC 0 SIN(0 0.5 1k)", "V1 in 0 0")
            .replace("RF out fb 51k", "RF out fb 551k");
        run("op-amp clipper", &text, "v1", |n| hot(n, 1.0));
    }

    #[test]
    fn common_cathode_converges_at_the_host_rate() {
        let text = deck("common_cathode_grid_current").replace("V1 in 0 SIN(0 3 1k)", "V1 in 0 0");
        run("common cathode", &text, "v1", |n| hot(n, 8.0));
    }

    /// E4: the Tube Screamer on its JRC4558 macromodel, input hot about
    /// the 4.5 V reference (the diodes and the rails both reached).
    #[test]
    fn tube_screamer_converges_at_the_host_rate() {
        run("Tube Screamer (JRC4558)", &deck("tube_screamer"), "v1", |n| {
            4.5 + hot(n, 1.0)
        });
    }

    /// E4: the gain-of-11 stage slewing and clipping on its rails.
    #[test]
    fn opamp_slew_converges_at_the_host_rate() {
        run("op-amp slew (JRC4558)", &deck("opamp_slew"), "v1", |n| hot(n, 3.0));
    }

    /// E5: the LM13700 VCA, the signal hot into the tanh's saturation.
    #[test]
    fn ota_vca_converges_at_the_host_rate() {
        run("OTA VCA (LM13700)", &deck("ota_vca"), "vin", |n| hot(n, 20.0));
    }

    /// E3: the EL34 pair, grids swung ±54 V about the class point —
    /// cutoff, crossover and grid conduction every cycle.
    #[test]
    fn push_pull_pentode_converges_at_the_host_rate() {
        run("EL34 push-pull", &deck("push_pull_pentode"), "vx", |n| hot(n, 36.0));
    }

    #[test]
    fn power_supply_converges_under_a_swinging_load() {
        run("power supply", &deck("power_supply"), "il", |n| {
            0.05 + 0.04 * (std::f64::consts::TAU * 7.0 * n as f64 / RATE).sin()
        });
    }

    // The transistor circuits (DSP.md E1, E2), each well past the level
    // its ngspice deck is driven at.

    #[test]
    fn fuzz_face_converges_at_the_host_rate() {
        run("fuzz face", &deck("fuzz_face"), "vin", |n| hot(n, 2.0));
    }

    #[test]
    fn rangemaster_converges_at_the_host_rate() {
        run("rangemaster", &deck("rangemaster"), "vin", |n| hot(n, 3.0));
    }

    #[test]
    fn ce_stage_converges_at_the_host_rate() {
        run("CE stage", &deck("ce_stage"), "vin", |n| hot(n, 2.0));
    }

    #[test]
    fn jfet_booster_converges_at_the_host_rate() {
        run("JFET booster", &deck("jfet_booster"), "vin", |n| hot(n, 4.0));
    }

    #[test]
    fn mosfet_booster_converges_at_the_host_rate() {
        run("MOSFET booster", &deck("mosfet_booster"), "vin", |n| hot(n, 8.0));
    }
}
