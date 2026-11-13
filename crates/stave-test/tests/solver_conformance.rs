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

    fn run(name: &str, text: &str, source: &str, input: impl FnMut(usize) -> f64) {
        run_netlist(name, &Netlist::parse_spice(text).unwrap(), source, RATE as usize, input);
    }

    /// `frames` samples of `input` into `source`.
    fn run_netlist(name: &str, netlist: &Netlist, source: &str, frames: usize, mut input: impl FnMut(usize) -> f64) {
        let mut c = Circuit::<Dk>::new(netlist).unwrap();
        c.drive(source).unwrap();
        c.prepare(RATE as f32);
        for n in 0..frames {
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

    // The classic circuits (DSP.md R24), their topologies hot at the host
    // rate: every clipping stage, slew limit, cutoff and grid conduction
    // reached each cycle, for a tenth of a second: eleven periods of the
    // 110 Hz tone (the ladder's sixteen transistors keep a longer run slow).
    const CLASSIC_FRAMES: usize = RATE as usize / 10;

    #[test]
    fn big_muff_converges_at_the_host_rate() {
        use stave_dsp::circuit::topologies::big_muff_rams_head;
        run_netlist("Big Muff (Ram's Head)", &big_muff_rams_head(), "vin", CLASSIC_FRAMES, |n| hot(n, 3.0));
    }

    #[test]
    fn tube_screamer_ts808_converges_at_the_host_rate() {
        use stave_dsp::circuit::topologies::tube_screamer_ts808;
        run_netlist("Tube Screamer TS-808", &tube_screamer_ts808(), "vin", CLASSIC_FRAMES, |n| hot(n, 1.5));
    }

    #[test]
    fn proco_rat_converges_at_the_host_rate() {
        use stave_dsp::circuit::topologies::proco_rat;
        run_netlist("ProCo RAT", &proco_rat(), "vin", CLASSIC_FRAMES, |n| hot(n, 1.5));
    }

    #[test]
    fn marshall_jcm800_converges_at_the_host_rate() {
        use stave_dsp::circuit::topologies::marshall_jcm800_2203;
        run_netlist("JCM800 2203 preamp", &marshall_jcm800_2203(), "vin", CLASSIC_FRAMES, |n| hot(n, 3.0));
    }

    #[test]
    fn moog_ladder_converges_at_the_host_rate() {
        use stave_dsp::circuit::topologies::moog_ladder_904a;
        run_netlist("Moog ladder (k = 3)", &moog_ladder_904a(3.0), "vin", CLASSIC_FRAMES, |n| hot(n, 0.3));
    }

    /// DSP.md A1: the circuits with stateful ADAA at the host rate, under
    /// the hot signals above and four times hotter — finite, bounded by
    /// their rails, no solve at its cap (the quadrature's extra solves
    /// included).
    #[test]
    fn circuits_with_adaa_converge_at_the_host_rate() {
        use stave_dsp::circuit::methods::adaa::Antialiasing;
        let opamp = deck("opamp_clipper").replace("V1 in 0 DC 0 SIN(0 0.5 1k)", "V1 in 0 0");
        let clipper = deck("diode_clipper").replace("V1 in 0 SIN(0 1 1k)", "V1 in 0 0");
        let cases: [(&str, &str, &str, f64, f64, f64); 5] = [
            ("diode clipper", &clipper, "v1", 0.0, 4.0, 2.0),
            ("op-amp clipper", &opamp, "v1", 0.0, 1.0, 10.0),
            ("common cathode", &deck("common_cathode"), "v1", 0.0, 8.0, 400.0),
            ("fuzz face", &deck("fuzz_face"), "vin", 0.0, 2.0, 20.0),
            ("Tube Screamer", &deck("tube_screamer"), "v1", 4.5, 1.0, 10.0),
        ];
        for (name, text, source, offset, volts, bound) in cases {
            for hotter in [1.0, 4.0] {
                let mut c = Circuit::<Dk>::new(&Netlist::parse_spice(text).unwrap()).unwrap();
                c.drive(source).unwrap();
                c.set_antialiasing(Antialiasing::Adaa);
                c.prepare(RATE as f32);
                for n in 0..RATE as usize {
                    let y = c.tick_f64(offset + hotter * hot(n, volts));
                    assert!(y.is_finite() && y.abs() < bound, "{name} ×{hotter}: sample {n} = {y}");
                }
                let h = c.health();
                println!(
                    "{name} ×{hotter} (DK, ADAA {:?}): {:.2} iterations/solve, {:.2} solves/sample, {} capped, {} continuations",
                    c.adaa_mean().unwrap(),
                    h.newton.mean_iterations(),
                    h.newton.solves as f64 / RATE,
                    h.newton.capped,
                    h.continuations
                );
                assert_eq!(h.newton.capped, 0, "{name} ×{hotter} (DK, ADAA) hit its iteration cap");
            }
        }
        for hotter in [1.0, 4.0] {
            let mut s = stave_dsp::DiodeClipper::new(stave_dsp::DiodeClipperConfig {
                input_ohms: 2.2e3,
                capacitor_farads: 10e-9,
                diode: stave_dsp::Shockley::N4148,
                solver: stave_dsp::PairSolver::default(),
            });
            s.set_antialiasing(Antialiasing::Adaa);
            stave_dsp::Stage::prepare(&mut s, RATE as f32);
            for n in 0..RATE as usize {
                let y = stave_dsp::Stage::tick(&mut s, (hotter * hot(n, 4.0)) as f32);
                assert!(y.is_finite() && y.abs() < 2.0, "WDF clipper ×{hotter}: sample {n} = {y}");
            }
            let st = s.stats();
            println!(
                "diode clipper ×{hotter} (WDF, ADAA): {:.2} iterations/solve, {} capped",
                st.iterations as f64 / st.solves as f64,
                st.capped
            );
            assert_eq!(st.capped, 0);
        }
    }
}
