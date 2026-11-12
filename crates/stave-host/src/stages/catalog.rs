//! The catalog: every stave-dsp stage a host can open by name, with its
//! settings and how they build and reconfigure the stage. Static data.

use stave_dsp::{
    AudioAdapter, AudioAdapterConfig, Biquad, CommonCathode, CommonCathodeConfig, Coupler,
    DiodeClipper, DiodeClipperConfig, FilterSpec, Hysteresis, JilesAtherton, NewtonStats,
    NonlinearRc, OpAmpClipper, OpAmpClipperConfig, PairSolver, RcClipperConfig, Shockley, Slew,
    Stage, Transformer, TransformerConfig, Tube,
};
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

use super::{BoxedStage, Configurable, StageSpec, Range, Setting, SettingValues};

/// A stage plus how its settings apply to it.
struct CatalogStage<S> {
    stage: S,
    configure: fn(&mut S, &SettingValues),
    solver: Option<fn(&S) -> NewtonStats>,
}

fn stage<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &SettingValues),
) -> BoxedStage {
    Box::new(CatalogStage {
        stage,
        configure,
        solver: None,
    })
}

fn stage_with_solver<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &SettingValues),
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
    fn configure(&mut self, values: &SettingValues) {
        (self.configure)(&mut self.stage, values);
    }

    fn solver(&self) -> Option<NewtonStats> {
        self.solver.map(|stats| stats(&self.stage))
    }
}

fn diode_clipper(v: &SettingValues) -> DiodeClipperConfig {
    DiodeClipperConfig {
        input_ohms: v.get("input") * 1e3,
        capacitor_farads: v.get("capacitor") * 1e-9,
        diode: Shockley::D1N4148,
        solver: PairSolver::ALL[v.choice("solver")],
    }
}

/// The TS808 stage with its drive pot in series with the 51 kΩ.
fn opamp_clipper(v: &SettingValues) -> OpAmpClipperConfig {
    OpAmpClipperConfig {
        feedback_ohms: OpAmpClipperConfig::TS808.feedback_ohms + v.get("drive") * 1e3,
        ..OpAmpClipperConfig::TS808
    }
}

/// Jiles & Atherton's 1986 steel in a 2000-turn, 6 cm², 10 cm core.
fn transformer(v: &SettingValues) -> TransformerConfig {
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

fn common_cathode(v: &SettingValues) -> CommonCathodeConfig<Tube> {
    CommonCathodeConfig {
        tube: Tube::ALL[v.choice("tube")],
        source_ohms: v.get("source") * 1e3,
        supply_volts: v.get("supply"),
        plate_load_ohms: v.get("plate_load") * 1e3,
        cathode_ohms: v.get("cathode") * 1e3,
        bypass_farads: v.get("cathode_cap") * 1e-6,
        newton: Default::default(),
    }
}

fn audio_port(v: &SettingValues) -> AudioAdapterConfig {
    AudioAdapterConfig {
        full_scale_in_volts: v.get("full_scale_in"),
        full_scale_out_volts: v.get("full_scale_out"),
    }
}

fn peak(v: &SettingValues) -> FilterSpec {
    FilterSpec::Peak {
        hz: v.f32("hz"),
        db: v.f32("db"),
        q: v.f32("q"),
    }
}

fn lowpass(v: &SettingValues) -> FilterSpec {
    FilterSpec::Lowpass {
        hz: v.f32("hz"),
        q: v.f32("q"),
    }
}

pub const CATALOG: &[StageSpec] = &[
    StageSpec {
        name: "filter/peak",
        settings: &[
            Setting {
                name: "hz",
                default: 1_000.0,
                range: Range::Continuous {
                    min: AUDIBLE_MIN_HZ,
                    max: AUDIBLE_MAX_HZ,
                    unit: "Hz",
                },
            },
            Setting {
                name: "db",
                default: 6.0,
                range: Range::Continuous {
                    min: -24.0,
                    max: 24.0,
                    unit: "dB",
                },
            },
            Setting {
                name: "q",
                default: 1.0,
                range: Range::Continuous {
                    min: 0.1,
                    max: 10.0,
                    unit: "",
                },
            },
        ],
        build: |v| stage(Biquad::new(peak(v)), |f, v| f.set(peak(v))),
    },
    StageSpec {
        name: "filter/lowpass",
        settings: &[
            Setting {
                name: "hz",
                default: 5_000.0,
                range: Range::Continuous {
                    min: AUDIBLE_MIN_HZ,
                    max: AUDIBLE_MAX_HZ,
                    unit: "Hz",
                },
            },
            Setting {
                name: "q",
                default: 0.707,
                range: Range::Continuous {
                    min: 0.1,
                    max: 10.0,
                    unit: "",
                },
            },
        ],
        build: |v| stage(Biquad::new(lowpass(v)), |f, v| f.set(lowpass(v))),
    },
    StageSpec {
        name: "coupler",
        settings: &[Setting {
            name: "hz",
            default: 20.0,
            range: Range::Continuous {
                min: 0.1,
                max: 500.0,
                unit: "Hz",
            },
        }],
        build: |v| stage(Coupler::new(v.f32("hz")), |c, v| c.set_hz(v.f32("hz"))),
    },
    StageSpec {
        name: "slew",
        settings: &[
            Setting {
                name: "rise",
                default: 2.0,
                range: Range::Continuous {
                    min: 0.01,
                    max: 100.0,
                    unit: "/ms",
                },
            },
            Setting {
                name: "fall",
                default: 2.0,
                range: Range::Continuous {
                    min: 0.01,
                    max: 100.0,
                    unit: "/ms",
                },
            },
        ],
        build: |v| {
            stage(Slew::new(v.f32("rise"), v.f32("fall")), |s, v| {
                s.set_rates(v.f32("rise"), v.f32("fall"))
            })
        },
    },
    StageSpec {
        name: "clipper/diode",
        settings: &[
            Setting {
                name: "tau",
                default: 0.5,
                range: Range::Continuous {
                    min: 0.01,
                    max: 10.0,
                    unit: "ms",
                },
            },
            Setting {
                name: "threshold",
                default: 0.3,
                range: Range::Continuous {
                    min: 0.01,
                    max: 2.0,
                    unit: "",
                },
            },
            Setting {
                name: "hardness",
                default: 3.0,
                range: Range::Continuous {
                    min: 1.0,
                    max: 8.0,
                    unit: "",
                },
            },
        ],
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
        settings: &[
            Setting {
                name: "solver",
                default: 0.0,
                range: Range::Choice(&PairSolver::NAMES),
            },
            Setting {
                name: "input",
                default: 2.2,
                range: Range::Continuous {
                    min: 0.1,
                    max: 100.0,
                    unit: "kΩ",
                },
            },
            Setting {
                name: "capacitor",
                default: 10.0,
                range: Range::Continuous {
                    min: 0.1,
                    max: 1_000.0,
                    unit: "nF",
                },
            },
        ],
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
        settings: &[Setting {
            name: "drive",
            default: 0.0,
            range: Range::Continuous {
                min: 0.0,
                max: 500.0,
                unit: "kΩ",
            },
        }],
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
        settings: &[Setting {
            name: "source",
            default: 1.0,
            range: Range::Continuous {
                min: 0.01,
                max: 10.0,
                unit: "kΩ",
            },
        }],
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
        settings: &[
            Setting {
                name: "tube",
                default: 0.0,
                range: Range::Choice(&Tube::NAMES),
            },
            Setting {
                name: "source",
                default: 1.0,
                range: Range::Continuous {
                    min: 0.0,
                    max: 220.0,
                    unit: "kΩ",
                },
            },
            Setting {
                name: "full_scale_in",
                default: 1.0,
                range: Range::Continuous {
                    min: 0.01,
                    max: 20.0,
                    unit: "V/FS",
                },
            },
            Setting {
                name: "full_scale_out",
                default: 100.0,
                range: Range::Continuous {
                    min: 10.0,
                    max: 300.0,
                    unit: "V/FS",
                },
            },
            Setting {
                name: "supply",
                default: 300.0,
                range: Range::Continuous {
                    min: 100.0,
                    max: 450.0,
                    unit: "V",
                },
            },
            Setting {
                name: "plate_load",
                default: 100.0,
                range: Range::Continuous {
                    min: 10.0,
                    max: 470.0,
                    unit: "kΩ",
                },
            },
            Setting {
                name: "cathode",
                default: 1.5,
                range: Range::Continuous {
                    min: 0.1,
                    max: 10.0,
                    unit: "kΩ",
                },
            },
            Setting {
                name: "cathode_cap",
                default: 22.0,
                range: Range::Continuous {
                    min: 0.1,
                    max: 100.0,
                    unit: "µF",
                },
            },
        ],
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
        settings: &[
            Setting {
                name: "drive",
                default: 1.0,
                range: Range::Continuous {
                    min: 0.01,
                    max: 10.0,
                    unit: "",
                },
            },
            Setting {
                name: "depth",
                default: 0.5,
                range: Range::Continuous {
                    min: 0.0,
                    max: 1.0,
                    unit: "",
                },
            },
        ],
        build: |v| {
            stage(Hysteresis::new(v.f32("drive"), v.f32("depth")), |h, v| {
                h.set_drive(v.f32("drive"));
                h.set_depth(v.f32("depth"));
            })
        },
    },
];

