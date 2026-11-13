//! The characterised circuits: each deck's input (the source the plan
//! drives, at its operating-point DC level) and anything held.

use stave_test::characterise::{Budget, Characterise, Input, Rates, Tier};

pub struct Circuit {
    pub name: &'static str,
    pub input: fn() -> Input,
    /// A supply: its input is the load current, from switch-on.
    pub supply: bool,
    /// A fixed settling time; otherwise half the linearised tail.
    pub settle: Option<f64>,
}

const fn circuit(name: &'static str, input: fn() -> Input) -> Circuit {
    Circuit {
        name,
        input,
        supply: false,
        settle: None,
    }
}

pub static CIRCUITS: [Circuit; 20] = [
    circuit("diode_clipper", || Input::new("v1", 0.0)),
    circuit("opamp_clipper", || Input::new("v1", 0.0)),
    // The TS808 stage rides on the 4.5 V reference.
    circuit("tube_screamer", || Input::new("v1", 4.5)),
    circuit("common_cathode", || Input::new("v1", 0.0)),
    circuit("common_cathode_grid_current", || Input::new("v1", 0.0)),
    // Fixed bias: the grid at the 12AX7's −1.5 V through the stopper.
    circuit("triode_stage", || Input::new("v1", -1.5)),
    // The class point: the swing x at zero.
    circuit("push_pull", || Input::new("vx", 0.0)),
    circuit("push_pull_pentode", || Input::new("vx", 0.0)),
    // The 20 mA standing load; the plan adds to it. From switch-on, the
    // choke rings for about 0.3 s.
    Circuit {
        name: "power_supply",
        input: || Input::new("il", 0.02),
        supply: true,
        settle: Some(0.35),
    },
    circuit("fuzz_face", || Input::new("vin", 0.0)),
    circuit("rangemaster", || Input::new("vin", 0.0)),
    circuit("ce_stage", || Input::new("vin", 0.0)),
    circuit("jfet_booster", || Input::new("vin", 0.0)),
    circuit("mosfet_booster", || Input::new("vin", 0.0)),
    // The same transistor decks on their cards' DC part (DSP.md R11).
    circuit("fuzz_face_static", || Input::new("vin", 0.0)),
    circuit("rangemaster_static", || Input::new("vin", 0.0)),
    circuit("ce_stage_static", || Input::new("vin", 0.0)),
    circuit("jfet_booster_static", || Input::new("vin", 0.0)),
    circuit("mosfet_booster_static", || Input::new("vin", 0.0)),
    // The VCA with its control held at 5 V (about 0.28 mA of bias).
    circuit("ota_vca", || Input::new("vin", 0.0).holding("vc", 5.0)),
];

/// The solver's rate for a case at `tier`: the oversampling its waveform
/// case runs at (`matches_ngspice.rs`, `dk_matches_ngspice.rs`,
/// `transistors_match_ngspice.rs`) — 16× where an op-amp's feedback pole
/// is faster than a 4× step (8× for the Tube Screamer's T0, for time),
/// 1× for a memoryless circuit's T0 (no step to refine: its "oversampled"
/// run is its 48 kHz one; T1's 4× measures it free of aliasing), 16× for
/// the transistor decks whose cards' charge storage puts poles far above
/// the audio band (DSP.md R11), else 4×.
pub fn oversample(case: &str, tier: Tier) -> u32 {
    match (case.split('/').next().unwrap(), tier) {
        ("tube_screamer", Tier::T0) => 8,
        ("opamp_clipper" | "tube_screamer", _) => 16,
        ("fuzz_face" | "ce_stage" | "mosfet_booster", _) => 16,
        ("triode_stage" | "push_pull" | "push_pull_pentode", Tier::T0) => 1,
        _ => 4,
    }
}

/// Each case's budgets at `tier` (cases are `<circuit>` for
/// `Circuit<Dk>` and `<circuit>/<topology>` for a hand topology).
pub fn budgets(case: &str, tier: Tier, c: Characterise) -> Characterise {
    let mut c = c.oversampled(oversample(case, tier));
    for &(check, rates, first, second) in budgets::table(case, tier) {
        let b = if second.is_nan() {
            Budget::db(first)
        } else {
            Budget::db(first).degrees(second)
        };
        c = match rates {
            Rates::Oversampled => c.budget(check, b),
            Rates::Shipping => c.shipping(check, b),
        };
    }
    c
}

mod budgets;

/// A fresh hand stage at a rate.
type Make = Box<dyn Fn(f64) -> Box<dyn FnMut(f64) -> f64>>;

/// A hand topology's case (`<circuit>/<topology>`) at `tier`, run and
/// asserted: each ships as its own type, so each is held to ngspice on
/// its circuit's plan directly.
pub fn hand(case: &str, tier: Tier) {
    use stave_dsp::{
        CommonCathode, CommonCathodeConfig, Dempwolf, DiodeClipper, DiodeClipperConfig, Koren,
        OpAmpClipper, OpAmpClipperConfig, PairSolver, PowerSupply, PowerSupplyConfig, Shockley,
        Stage,
    };
    fn cathode<T: stave_dsp::circuit::devices::triode::Triode + Copy + 'static>(
        tube: T,
        source_ohms: f64,
    ) -> (Make, f64) {
        let make = move |rate: f64| {
            let mut c = CommonCathode::new(CommonCathodeConfig {
                tube,
                source_ohms,
                supply_volts: 300.0,
                plate_load_ohms: 100e3,
                cathode_ohms: 1.5e3,
                bypass_farads: 22e-6,
                newton: Default::default(),
            });
            c.prepare(rate as f32);
            c
        };
        let plate = make(48_000.0).plate_volts();
        // The hand stage reports its plate in f32 volts.
        let stage: Make = Box::new(move |rate| {
            let mut c = make(rate);
            Box::new(move |x| f64::from(c.tick(x as f32)))
        });
        (stage, plate)
    }
    let circuit = case.split('/').next().unwrap();
    let catalogue = match case {
        "diode_clipper/hand" => "stave_dsp::circuit::topologies::diode_clipper::DiodeClipper",
        "opamp_clipper/hand" => "stave_dsp::circuit::topologies::opamp_clipper::OpAmpClipper",
        "common_cathode/hand" | "common_cathode_grid_current/hand" => "stave_dsp::circuit::topologies::common_cathode::CommonCathode",
        "power_supply/hand" => "stave_dsp::circuit::topologies::power_supply::PowerSupply",
        other => panic!("no hand topology {other}"),
    };
    let base = Characterise::ngspice(circuit).named(case).tier(tier).catalogue(catalogue);
    let c = match case {
        "diode_clipper/hand" => base
            .stage(|rate| {
                let mut c = DiodeClipper::new(DiodeClipperConfig {
                    input_ohms: 2.2e3,
                    capacitor_farads: 10e-9,
                    diode: Shockley::N4148,
                    solver: PairSolver::default(),
                });
                c.prepare(rate as f32);
                move |x| f64::from(c.tick(x as f32))
            })
            .operating_point("out", 0.0),
        "opamp_clipper/hand" => base
            .stage(|rate| {
                let mut c = OpAmpClipper::new(OpAmpClipperConfig::TS808);
                c.prepare(rate as f32);
                move |x| c.tick_f64(x)
            })
            .operating_point("out", 0.0),
        "common_cathode/hand" => {
            let (stage, plate) = cathode(Koren::T12AX7, 0.0);
            base.stage(stage).operating_point("p", plate)
        }
        "common_cathode_grid_current/hand" => {
            let (stage, plate) = cathode(Dempwolf::RSD_1, 33e3);
            base.stage(stage).operating_point("p", plate)
        }
        // The supply ticks to the end of each step with that step's load
        // (the plan's current rides on the deck's 20 mA): sample 0 is the
        // switch-on state.
        "power_supply/hand" => base.stage(|rate| {
            let mut s = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
            s.prepare(rate as f32);
            let mut first = true;
            move |x| {
                if std::mem::take(&mut first) {
                    0.0
                } else {
                    s.tick_f64(0.02 + x)
                }
            }
        }),
        other => panic!("no hand topology {other}"),
    };
    // Each hand topology pins its fingerprint (DSP.md R18) at T0.
    budgets(case, tier, c).run().pinned().assert();
}
