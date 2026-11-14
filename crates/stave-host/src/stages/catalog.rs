//! The catalog: every stave-dsp stage a host can open by name, with its
//! settings and how they build and reconfigure the stage. Static data.

use stave_dsp::{
    AudioAdapter, AudioAdapterConfig, Biquad, CommonCathode, CommonCathodeConfig, Coupler,
    DiodeClipper, DiodeClipperConfig, FilterSpec, Hysteresis, JilesAtherton, NewtonStats,
    NonlinearRc, OpAmpClipper, OpAmpClipperConfig, PairSolver, RcClipperConfig, Shockley, Slew,
    Stage, Transformer, TransformerConfig, Tube,
};
use stave_dsp::circuit::devices::triode::koren::Koren;
use stave_core::params::{AnyParam, EnumParam, FloatParam, ParamEnum, ParamId, Range, Unit};
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

use super::{BoxedStage, Configurable, StageParams, StageSpec};
use stave_dsp::units::{Db, Hz};



/// A stage plus how its settings apply to it.
struct CatalogStage<S> {
    stage: S,
    configure: fn(&mut S, &StageParams),
    solver: Option<fn(&S) -> NewtonStats>,
}

pub(super) fn stage<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &StageParams),
) -> BoxedStage {
    Box::new(CatalogStage {
        stage,
        configure,
        solver: None,
    })
}

pub(super) fn stage_with_solver<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &StageParams),
    solver: fn(&S) -> NewtonStats,
) -> BoxedStage {
    Box::new(CatalogStage {
        stage,
        configure,
        solver: Some(solver),
    })
}

impl<S: Stage + Send> Stage for CatalogStage<S> {
    fn prepare(&mut self, sample_rate: f32) {
        self.stage.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.stage.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.stage.tick(x)
    }
    fn process(&mut self, block: &mut [f32]) {
        self.stage.process(block);
    }
    fn latency(&self) -> u32 {
        self.stage.latency()
    }
    fn tail(&self) -> stave_dsp::Tail {
        self.stage.tail()
    }
}

impl<S: Stage + Send> Configurable for CatalogStage<S> {
    fn configure(&mut self, values: &StageParams) {
        (self.configure)(&mut self.stage, values);
    }

    fn solver(&self) -> Option<NewtonStats> {
        self.solver.map(|stats| stats(&self.stage))
    }
}

fn diode_clipper(v: &StageParams) -> DiodeClipperConfig {
    DiodeClipperConfig {
        input_ohms: v.get("input") * 1e3,
        capacitor_farads: v.get("capacitor") * 1e-9,
        diode: Shockley::N4148,
        solver: PairSolver::ALL[v.choice("solver")],
    }
}

/// The TS808 stage with its drive pot in series with the 51 kΩ.
fn opamp_clipper(v: &StageParams) -> OpAmpClipperConfig {
    OpAmpClipperConfig {
        feedback_ohms: OpAmpClipperConfig::TS808.feedback_ohms + v.get("drive") * 1e3,
        ..OpAmpClipperConfig::TS808
    }
}

/// Jiles & Atherton's 1986 steel in a 2000-turn, 6 cm², 10 cm core.
fn transformer(v: &StageParams) -> TransformerConfig {
    TransformerConfig {
        core: JilesAtherton {
            saturation: 1.6e6,
            shape: 1_100.0,
            coupling: 1.6e-3,
            pinning: 400.0,
            reversibility: 0.2,
        },
        primary_turns: 2_000.0,
        core_area: 6e-4,
        path_length: 0.1,
        turns_ratio: 1.0,
        source_ohms: v.get("source") * 1e3,
        newton: Default::default(),
    }
}

fn common_cathode(v: &StageParams) -> CommonCathodeConfig<Tube> {
    CommonCathodeConfig {
        tube: TUBES[v.choice("tube")],
        source_ohms: v.get("source") * 1e3,
        supply_volts: v.get("supply"),
        plate_load_ohms: v.get("plate_load") * 1e3,
        cathode_ohms: v.get("cathode") * 1e3,
        bypass_farads: v.get("cathode_cap") * 1e-6,
        newton: Default::default(),
    }
}

fn audio_port(v: &StageParams) -> AudioAdapterConfig {
    AudioAdapterConfig {
        full_scale_in_volts: v.get("full_scale_in"),
        full_scale_out_volts: v.get("full_scale_out"),
    }
}

fn peak(v: &StageParams) -> FilterSpec {
    FilterSpec::Peak {
        frequency: Hz::new(v.f32("hz")),
        gain: Db::new(v.f32("db")),
        q: v.f32("q"),
    }
}

fn lowpass(v: &StageParams) -> FilterSpec {
    FilterSpec::Lowpass {
        frequency: Hz::new(v.f32("hz")),
        q: v.f32("q"),
    }
}

/// The bare-number unit of the catalog's dimensionless params.
const PLAIN: Unit = Unit::Plain { decimals: 3 };

/// A continuous param, linear across `min..=max`. A param change
/// re-voices the stage between blocks, with no glide, so it is not
/// offered for automation.
pub(super) fn float(
    id: u32,
    name: &'static str,
    min: f64,
    max: f64,
    default: f64,
    unit: Unit,
) -> Box<dyn AnyParam> {
    Box::new(
        FloatParam::new(ParamId(id), name, Range::linear(min, max))
            .unit(unit)
            .default(default)
            .not_automatable(),
    )
}

/// A choice among `T`'s options, starting at the first.
pub(super) fn choice<T: ParamEnum>(id: u32, name: &'static str) -> Box<dyn AnyParam> {
    choice_at::<T>(id, name, 0)
}

/// A choice among `T`'s options, starting at option `index`.
pub(super) fn choice_at<T: ParamEnum>(id: u32, name: &'static str, index: u32) -> Box<dyn AnyParam> {
    Box::new(EnumParam::new(ParamId(id), name, T::from_index(index)))
}

options_list!(
    /// The diode pair's solver.
    Solver = PairSolver::NAMES
);
/// The triodes a common cathode offers: [`Tube::ALL`], then Koren's
/// law fitted to Philips' ECC83 sheet (DSP.md E11) beside index 0's
/// published 12AX7, for the A/B.
const TUBES: [Tube; 9] = {
    let mut out = [Tube::Koren(Koren::ECC83_PHILIPS); 9];
    let mut i = 0;
    while i < Tube::ALL.len() {
        out[i] = Tube::ALL[i];
        i += 1;
    }
    out
};

const TUBE_NAMES: [&str; 9] = {
    let mut out = [""; 9];
    let mut i = 0;
    while i < out.len() {
        out[i] = TUBES[i].name();
        i += 1;
    }
    out
};

options_list!(
    /// The triode model.
    TubeChoice = TUBE_NAMES
);

pub const CATALOG: &[StageSpec] = &[
    StageSpec {
        name: "filter/peak",
        params: || {
            StageParams::new(vec![
                float(0, "hz", AUDIBLE_MIN_HZ, AUDIBLE_MAX_HZ, 1_000.0, Unit::Hz),
                float(1, "db", -24.0, 24.0, 6.0, Unit::Db),
                float(2, "q", 0.1, 10.0, 1.0, PLAIN),
            ])
        },
        build: |v| stage(Biquad::new(peak(v)), |f, v| f.set(peak(v))),
    },
    StageSpec {
        name: "filter/lowpass",
        params: || {
            StageParams::new(vec![
                float(0, "hz", AUDIBLE_MIN_HZ, AUDIBLE_MAX_HZ, 5_000.0, Unit::Hz),
                float(1, "q", 0.1, 10.0, 0.707, PLAIN),
            ])
        },
        build: |v| stage(Biquad::new(lowpass(v)), |f, v| f.set(lowpass(v))),
    },
    StageSpec {
        name: "coupler",
        params: || {
            StageParams::new(vec![
                float(0, "hz", 0.1, 500.0, 20.0, Unit::Hz),
            ])
        },
        build: |v| stage(Coupler::new(Hz::new(v.f32("hz"))), |c, v| c.set_frequency(Hz::new(v.f32("hz")))),
    },
    StageSpec {
        name: "slew",
        params: || {
            StageParams::new(vec![
                float(0, "rise", 0.01, 100.0, 2.0, Unit::Custom { label: "/ms", decimals: 2 }),
                float(1, "fall", 0.01, 100.0, 2.0, Unit::Custom { label: "/ms", decimals: 2 }),
            ])
        },
        build: |v| {
            stage(Slew::new(v.f32("rise"), v.f32("fall")), |s, v| {
                s.set_rates(v.f32("rise"), v.f32("fall"))
            })
        },
    },
    StageSpec {
        name: "clipper/diode",
        params: || {
            StageParams::new(vec![
                float(0, "tau", 0.01, 10.0, 0.5, Unit::Ms),
                float(1, "threshold", 0.01, 2.0, 0.3, PLAIN),
                float(2, "hardness", 1.0, 8.0, 3.0, PLAIN),
            ])
        },
        build: |v| {
            stage_with_solver(
                NonlinearRc::diode_clipper(&RcClipperConfig {
                    tau_ms: v.f32("tau"),
                    threshold: v.f32("threshold"),
                    hardness: v.f32("hardness"),
                }),
                |c, v| {
                    c.set_diode_clipper(&RcClipperConfig {
                        tau_ms: v.f32("tau"),
                        threshold: v.f32("threshold"),
                        hardness: v.f32("hardness"),
                    })
                },
                NonlinearRc::stats,
            )
        },
    },
    StageSpec {
        name: "circuit/diode_clipper",
        params: || {
            StageParams::new(vec![
                choice::<Solver>(0, "solver"),
                float(1, "input", 0.1, 100.0, 2.2, Unit::Custom { label: "kΩ", decimals: 2 }),
                float(2, "capacitor", 0.1, 1_000.0, 10.0, Unit::Custom { label: "nF", decimals: 2 }),
            ])
        },
        build: |v| {
            stage_with_solver(
                DiodeClipper::new(diode_clipper(v)),
                |c, v| c.configure(&diode_clipper(v)),
                DiodeClipper::stats,
            )
        },
    },
    StageSpec {
        name: "circuit/opamp_clipper",
        params: || {
            StageParams::new(vec![
                float(0, "drive", 0.0, 500.0, 0.0, Unit::Custom { label: "kΩ", decimals: 2 }),
            ])
        },
        build: |v| {
            stage_with_solver(
                OpAmpClipper::new(opamp_clipper(v)),
                |c, v| c.configure(&opamp_clipper(v)),
                OpAmpClipper::stats,
            )
        },
    },
    StageSpec {
        name: "circuit/transformer",
        params: || {
            StageParams::new(vec![
                float(0, "source", 0.01, 10.0, 1.0, Unit::Custom { label: "kΩ", decimals: 2 }),
            ])
        },
        build: |v| {
            stage_with_solver(
                Transformer::new(transformer(v)),
                |c, v| c.configure(&transformer(v)),
                Transformer::stats,
            )
        },
    },
    StageSpec {
        name: "circuit/common_cathode",
        params: || {
            StageParams::new(vec![
                choice::<TubeChoice>(0, "tube"),
                float(1, "source", 0.0, 220.0, 1.0, Unit::Custom { label: "kΩ", decimals: 2 }),
                float(2, "full_scale_in", 0.01, 20.0, 1.0, Unit::Custom { label: "V/FS", decimals: 2 }),
                float(3, "full_scale_out", 10.0, 300.0, 100.0, Unit::Custom { label: "V/FS", decimals: 2 }),
                float(4, "supply", 100.0, 450.0, 300.0, Unit::Custom { label: "V", decimals: 2 }),
                float(5, "plate_load", 10.0, 470.0, 100.0, Unit::Custom { label: "kΩ", decimals: 2 }),
                float(6, "cathode", 0.1, 10.0, 1.5, Unit::Custom { label: "kΩ", decimals: 2 }),
                float(7, "cathode_cap", 0.1, 100.0, 22.0, Unit::Custom { label: "µF", decimals: 2 }),
            ])
        },
        build: |v| {
            stage_with_solver(
                AudioAdapter::new(CommonCathode::new(common_cathode(v)), audio_port(v)),
                |port, v| {
                    port.circuit.configure(&common_cathode(v));
                    port.configure(audio_port(v));
                },
                |port| port.circuit.stats(),
            )
        },
    },
    StageSpec {
        name: "hysteresis",
        params: || {
            StageParams::new(vec![
                float(0, "drive", 0.01, 10.0, 1.0, PLAIN),
                float(1, "depth", 0.0, 1.0, 0.5, PLAIN),
            ])
        },
        build: |v| {
            stage(Hysteresis::new(v.f32("drive"), v.f32("depth")), |h, v| {
                h.set_drive(v.f32("drive"));
                h.set_depth(v.f32("depth"));
            })
        },
    },
];

/// The reference models a host tool can open by name beside the
/// catalog: whole verified netlists and the ladder filters, offline (a
/// netlist costs microseconds a sample, so they are not held to the
/// realtime validation suite the catalog passes). For measuring and
/// auditioning against a plugin (`stave-play --characterise`, the Voicing
/// tab).
pub const REFERENCES: &[StageSpec] = &[
    StageSpec {
        name: "circuit/ts808",
        params: super::pedals::ts808_params,
        build: super::pedals::ts808,
    },
    StageSpec {
        name: "circuit/ts808_lfo",
        params: super::pedals::ts808_lfo_params,
        build: super::pedals::ts808_lfo,
    },
    StageSpec {
        name: "circuit/klon_centaur",
        params: super::pedals::klon_params,
        build: super::pedals::klon,
    },
    StageSpec {
        name: "circuit/mxr_distortion_plus",
        params: super::pedals::distortion_plus_params,
        build: super::pedals::distortion_plus,
    },
    StageSpec {
        name: "circuit/boss_ds1",
        params: super::pedals::ds1_params,
        build: super::pedals::ds1,
    },
    StageSpec {
        name: "circuit/proco_rat",
        params: super::pedals::rat_params,
        build: super::pedals::rat,
    },
    StageSpec {
        name: "circuit/big_muff",
        params: super::pedals::big_muff_params,
        build: super::pedals::big_muff,
    },
    StageSpec {
        name: "circuit/fuzz_face",
        params: super::pedals::fuzz_face_params,
        build: super::pedals::fuzz_face,
    },
    StageSpec {
        name: "circuit/rangemaster",
        params: super::pedals::rangemaster_params,
        build: super::pedals::rangemaster,
    },
    StageSpec {
        name: "circuit/phase90",
        params: super::pedals::phase90_params,
        build: super::pedals::phase90,
    },
    StageSpec {
        name: "reverb/plate_emt140",
        params: super::effects::plate_params,
        build: super::effects::plate,
    },
    StageSpec {
        name: "rotary/leslie_122h",
        params: super::effects::leslie_params,
        build: super::effects::leslie,
    },
    StageSpec {
        name: "tape/chowdhury",
        params: super::effects::tape_params,
        build: super::effects::tape,
    },
    StageSpec {
        name: "dynamics/opto",
        params: super::effects::opto_params,
        build: super::effects::opto,
    },
    StageSpec {
        name: "converter/sampler",
        params: super::effects::sampler_params,
        build: super::effects::sampler,
    },
    StageSpec {
        name: "quantise/dither",
        params: super::effects::quantiser_params,
        build: super::effects::quantiser,
    },
    StageSpec {
        name: "cabinet/close_mic",
        params: super::effects::close_mic_params,
        build: super::effects::close_mic,
    },
    StageSpec {
        name: "instrument/string",
        params: super::effects::string_params,
        build: super::effects::string,
    },
    StageSpec {
        name: "instrument/drum",
        params: super::effects::drum_params,
        build: super::effects::drum,
    },
    StageSpec {
        name: "circuit/jcm800_2203",
        params: super::classics::jcm800_params,
        build: super::classics::jcm800,
    },
    StageSpec {
        name: "circuit/moog_904a",
        params: super::classics::moog_params,
        build: super::classics::moog,
    },
    StageSpec {
        name: "filter/ladder",
        params: super::classics::ladder_params,
        build: super::classics::ladder,
    },
];
