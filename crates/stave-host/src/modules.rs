//! stave-dsp modules by name, so every host tool loads a module the way
//! it loads a plugin: `module:<name>` with its settings as controls.
//! A setting change is applied in place at the next block: no
//! allocation, state kept, safe from any thread.

use stave_dsp::{
    JilesAtherton, OpAmpClipper, OpAmpClipperConfig, RcClipperConfig, Transformer,
    TransformerConfig,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use stave_dsp::{
    AudioAdapter, AudioAdapterConfig, CommonCathode, CommonCathodeConfig, Coupler, DiodeClipper,
    DiodeClipperConfig, FilterSpec, FilterStage, Hysteresis, NewtonStats, NonlinearRc, PairSolver,
    Shockley, Slew, Stage, Tube,
};
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

use crate::adapt::{StageInstance, StageInstanceConfig};
use crate::config::HostConfig;
use crate::control::{Control, ControlError, ControlFlags};
use crate::instance::{Instance, Layout, Probe, Tail};
use crate::note::NoteEvent;

pub const PREFIX: &str = "module:";

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Setting {
    pub name: &'static str,
    pub default: f64,
    pub range: Range,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Range {
    Continuous {
        min: f64,
        max: f64,
        unit: &'static str,
    },
    /// One of the named options, held as its index.
    Choice(&'static [&'static str]),
}

impl Range {
    pub fn min(&self) -> f64 {
        match self {
            Self::Continuous { min, .. } => *min,
            Self::Choice(_) => 0.0,
        }
    }

    pub fn max(&self) -> f64 {
        match self {
            Self::Continuous { max, .. } => *max,
            Self::Choice(options) => options.len().saturating_sub(1) as f64,
        }
    }

    pub fn admits(&self, value: f64) -> bool {
        let within = (self.min()..=self.max()).contains(&value);
        match self {
            Self::Continuous { .. } => within,
            Self::Choice(_) => within && value.fract() == 0.0,
        }
    }

    /// Position 0..1 across the range (linear; a lone option is 0).
    pub fn normalize(&self, value: f64) -> f64 {
        let span = self.max() - self.min();
        if span > 0.0 {
            (value - self.min()) / span
        } else {
            0.0
        }
    }

    /// The value at position 0..1; a choice lands on its nearest option.
    pub fn denormalize(&self, normalized: f64) -> f64 {
        let value = self.min() + normalized.clamp(0.0, 1.0) * (self.max() - self.min());
        match self {
            Self::Continuous { .. } => value,
            Self::Choice(_) => value.round(),
        }
    }

    fn unit(&self) -> &'static str {
        match self {
            Self::Continuous { unit, .. } => unit,
            Self::Choice(_) => "",
        }
    }

    fn options(&self) -> &'static [&'static str] {
        match self {
            Self::Continuous { .. } => &[],
            Self::Choice(options) => options,
        }
    }
}

/// A stage whose settings can change while it runs.
pub trait Configurable: Stage + Send {
    fn configure(&mut self, values: &SettingValues);

    /// What its nonlinear solver has done since `prepare`, if it has one.
    fn solver(&self) -> Option<NewtonStats> {
        None
    }
}

pub type BoxedModule = Box<dyn Configurable>;

pub struct Module {
    pub name: &'static str,
    pub settings: &'static [Setting],
    pub build: fn(&SettingValues) -> BoxedModule,
}

/// A module's current setting values, read by name.
pub struct SettingValues<'a> {
    settings: &'a [Setting],
    values: &'a [f64],
}

impl SettingValues<'_> {
    pub fn get(&self, name: &str) -> f64 {
        let i = self
            .settings
            .iter()
            .position(|s| s.name == name)
            .unwrap_or_else(|| panic!("module reads undeclared setting '{name}'"));
        self.values[i]
    }

    fn f32(&self, name: &str) -> f32 {
        self.get(name) as f32
    }

    /// A choice setting's option index.
    pub fn choice(&self, name: &str) -> usize {
        self.get(name) as usize
    }
}

/// A stage plus the rate it was prepared at, which its setters need.
struct ModuleStage<S> {
    stage: S,
    rate: f32,
    configure: fn(&mut S, &SettingValues, f32),
    solver: Option<fn(&S) -> NewtonStats>,
}

fn stage<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &SettingValues, f32),
) -> BoxedModule {
    Box::new(ModuleStage {
        stage,
        rate: 0.0,
        configure,
        solver: None,
    })
}

fn stage_with_solver<S: Stage + Send + 'static>(
    stage: S,
    configure: fn(&mut S, &SettingValues, f32),
    solver: fn(&S) -> NewtonStats,
) -> BoxedModule {
    Box::new(ModuleStage {
        stage,
        rate: 0.0,
        configure,
        solver: Some(solver),
    })
}

impl<S: Stage + Send> Stage for ModuleStage<S> {
    fn prepare(&mut self, rate: f32) {
        self.rate = rate;
        self.stage.prepare(rate);
    }
    fn reset(&mut self) {
        self.stage.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.stage.tick(x)
    }
    fn process_block(&mut self, io: &mut [f32]) {
        self.stage.process_block(io);
    }
    fn latency_samples(&self) -> u32 {
        self.stage.latency_samples()
    }
    fn tail_samples(&self) -> Option<u32> {
        self.stage.tail_samples()
    }
}

impl<S: Stage + Send> Configurable for ModuleStage<S> {
    fn configure(&mut self, values: &SettingValues) {
        (self.configure)(&mut self.stage, values, self.rate);
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

pub const CATALOG: &[Module] = &[
    Module {
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
        build: |v| stage(FilterStage::new(peak(v)), |f, v, _| f.set(peak(v))),
    },
    Module {
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
        build: |v| stage(FilterStage::new(lowpass(v)), |f, v, _| f.set(lowpass(v))),
    },
    Module {
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
        build: |v| {
            stage(Coupler::new(v.f32("hz")), |c, v, rate| {
                c.set_corner(v.f32("hz"), rate)
            })
        },
    },
    Module {
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
            stage(Slew::new(v.f32("rise"), v.f32("fall")), |s, v, rate| {
                s.set_rates(v.f32("rise"), v.f32("fall"), rate)
            })
        },
    },
    Module {
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
                |c, v, _| {
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
    Module {
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
                |c, v, _| c.configure(&diode_clipper(v)),
                DiodeClipper::stats,
            )
        },
    },
    Module {
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
                |c, v, _| c.configure(&opamp_clipper(v)),
                OpAmpClipper::stats,
            )
        },
    },
    Module {
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
                |c, v, _| c.configure(&transformer(v)),
                Transformer::stats,
            )
        },
    },
    Module {
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
                |port, v, rate| {
                    port.circuit.configure(&common_cathode(v), f64::from(rate));
                    port.configure(audio_port(v));
                },
                |port| port.circuit.stats(),
            )
        },
    },
    Module {
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
            stage(
                Hysteresis::new(v.f32("drive"), v.f32("depth")),
                |h, v, _| {
                    h.set_drive(v.f32("drive"));
                    h.set_depth(v.f32("depth"));
                },
            )
        },
    },
];

pub fn find(name: &str) -> Option<&'static Module> {
    CATALOG.iter().find(|m| m.name == name)
}

/// Setting values shared between whoever writes them and the instance
/// that applies them; `edits` counts writes so the instance knows when.
struct Shared {
    module: &'static Module,
    values: Box<[AtomicU64]>,
    edits: AtomicU64,
}

/// Writes a running module's settings from any thread.
#[derive(Clone)]
pub struct ModuleHandle(Arc<Shared>);

impl ModuleHandle {
    pub fn set(&self, name: &str, plain: f64) -> Result<(), ControlError> {
        let shared = &self.0;
        let i = shared
            .module
            .settings
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| ControlError::NotFound(name.into()))?;
        let range = shared.module.settings[i].range;
        if !range.admits(plain) {
            return Err(match range {
                Range::Choice(options) => ControlError::NotAnOption {
                    name: name.into(),
                    options: options.iter().map(|o| o.to_string()).collect(),
                },
                Range::Continuous { min, max, .. } => ControlError::OutOfRange {
                    name: name.into(),
                    min,
                    max,
                },
            });
        }
        shared.values[i].store(plain.to_bits(), Ordering::Relaxed);
        shared.edits.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Set at a normalized position: linear across a continuous range,
    /// the nearest option of a choice.
    pub fn set_normalized(&self, name: &str, normalized: f64) -> Result<(), ControlError> {
        let setting = self.setting(name)?;
        self.set(name, setting.range.denormalize(normalized))
    }

    /// The value at a normalized position as text: the option's name, or
    /// the number with its unit.
    pub fn display(&self, name: &str, normalized: f64) -> Option<String> {
        let range = self.setting(name).ok()?.range;
        let value = range.denormalize(normalized);
        Some(match range {
            Range::Choice(options) => options[value as usize].to_string(),
            Range::Continuous { unit, .. } => format!("{value:.3} {unit}"),
        })
    }

    fn setting(&self, name: &str) -> Result<&'static Setting, ControlError> {
        self.0
            .module
            .settings
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| ControlError::NotFound(name.into()))
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        let shared = &self.0;
        let i = shared.module.settings.iter().position(|s| s.name == name)?;
        Some(f64::from_bits(shared.values[i].load(Ordering::Relaxed)))
    }

    pub fn controls(&self) -> Vec<Control> {
        let module = self.0.module;
        module
            .settings
            .iter()
            .enumerate()
            .map(|(id, s)| Control {
                id: id as u32,
                name: s.name.into(),
                group: module.name.into(),
                unit: s.range.unit().into(),
                min: s.range.min(),
                max: s.range.max(),
                default: s.default,
                value: self.get(s.name).unwrap_or(s.default),
                normalized: s.range.normalize(self.get(s.name).unwrap_or(s.default)),
                steps: s.range.options().len().saturating_sub(1) as u32,
                step_names: s.range.options().iter().map(|o| o.to_string()).collect(),
                // A setting re-voices the stage between blocks; it has no
                // glide, so it does not claim automation.
                flags: ControlFlags::default(),
            })
            .collect()
    }
}

/// A catalog module running as an [`Instance`], one stage per channel.
pub struct ModuleInstance {
    shared: Arc<Shared>,
    applied_edits: u64,
    scratch: Vec<f64>,
    inner: StageInstance<BoxedModule>,
}

impl ModuleInstance {
    pub fn new(module: &'static Module, config: &HostConfig) -> Self {
        let defaults: Vec<f64> = module.settings.iter().map(|s| s.default).collect();
        let read = SettingValues {
            settings: module.settings,
            values: &defaults,
        };
        let inner = StageInstance::new(StageInstanceConfig {
            stages: (0..config.channels.get())
                .map(|_| (module.build)(&read))
                .collect(),
            sample_rate: config.sample_rate,
            max_block: config.max_block,
        });
        let shared = Arc::new(Shared {
            module,
            values: defaults
                .iter()
                .map(|v| AtomicU64::new(v.to_bits()))
                .collect(),
            edits: AtomicU64::new(0),
        });
        Self {
            shared,
            applied_edits: 0,
            scratch: defaults,
            inner,
        }
    }

    pub fn name(&self) -> &'static str {
        self.shared.module.name
    }

    pub fn handle(&self) -> ModuleHandle {
        ModuleHandle(Arc::clone(&self.shared))
    }

    fn apply_edits(&mut self) {
        let edits = self.shared.edits.load(Ordering::Acquire);
        if edits == self.applied_edits {
            return;
        }
        self.applied_edits = edits;
        for (slot, value) in self.scratch.iter_mut().zip(self.shared.values.iter()) {
            *slot = f64::from_bits(value.load(Ordering::Relaxed));
        }
        let values = SettingValues {
            settings: self.shared.module.settings,
            values: &self.scratch,
        };
        for stage in self.inner.stages_mut() {
            stage.configure(&values);
        }
    }
}

impl Instance for ModuleInstance {
    fn layout(&self) -> Layout {
        self.inner.layout()
    }

    fn latency(&self) -> u32 {
        self.inner.latency()
    }

    fn tail(&self) -> Tail {
        self.inner.tail()
    }

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[NoteEvent]) {
        self.apply_edits();
        self.inner.process(inputs, outputs, notes);
    }

    fn controls(&self) -> Vec<Control> {
        self.handle().controls()
    }

    fn probes(&self) -> Vec<Probe> {
        let mut solver: Option<NewtonStats> = None;
        for stats in self.inner.stages().iter().filter_map(|s| s.solver()) {
            solver
                .get_or_insert_with(NewtonStats::default)
                .merge(&stats);
        }
        let Some(s) = solver else { return vec![] };
        let probe = |name: &str, value: f64| Probe {
            name: name.into(),
            latest: value,
            min: value,
            max: value,
        };
        vec![
            probe(Probe::NEWTON_SOLVES, s.solves as f64),
            probe(Probe::NEWTON_MEAN_ITERATIONS, s.mean_iterations()),
            probe(Probe::NEWTON_CAPPED, s.capped as f64),
            probe(Probe::NEWTON_MAX_FINAL_STEP, s.max_final_step),
        ]
    }

    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.handle().set(name, plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapt::InstanceChannel;
    use crate::validate::{Status, Suite};
    use stave_measure::tone_test::ToneTest;

    fn config() -> HostConfig {
        HostConfig {
            max_block: std::num::NonZeroUsize::new(64).unwrap(),
            ..HostConfig::default()
        }
    }

    fn tone() -> ToneTest {
        ToneTest {
            bin: 100,
            window: 4800,
            sample_rate: 48_000.0,
            amplitude: 0.25,
            warmup_windows: 4,
        }
    }

    #[test]
    fn names_are_unique_and_defaults_in_range() {
        for (i, m) in CATALOG.iter().enumerate() {
            assert!(CATALOG[..i].iter().all(|o| o.name != m.name), "{}", m.name);
            for s in m.settings {
                assert!(s.range.admits(s.default), "{}.{}", m.name, s.name);
            }
        }
    }

    #[test]
    fn settings_are_controls_and_a_change_reaches_the_audio() {
        let mut peak = ModuleInstance::new(find("filter/peak").unwrap(), &config());
        let names: Vec<_> = peak.controls().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["hz", "db", "q"]);
        let gain = |inst: &mut ModuleInstance| {
            tone().gain_db(InstanceChannel {
                instance: inst,
                channel: 0,
            })
        };
        assert!((gain(&mut peak) - 6.0).abs() < 0.1);
        peak.set_control("db", -12.0).unwrap();
        assert!((gain(&mut peak) + 12.0).abs() < 0.1);
    }

    #[test]
    fn out_of_range_and_unknown_settings_are_refused() {
        let mut m = ModuleInstance::new(find("coupler").unwrap(), &config());
        assert!(matches!(
            m.set_control("hz", 1e6),
            Err(ControlError::OutOfRange { .. })
        ));
        assert!(matches!(
            m.set_control("nope", 1.0),
            Err(ControlError::NotFound(_))
        ));
    }

    #[test]
    fn the_diode_clipper_reports_its_solver() {
        let mut clipper = ModuleInstance::new(find("clipper/diode").unwrap(), &config());
        let input = vec![0.8f32; 64];
        let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        clipper.process(&[&input, &input], &mut [&mut l, &mut r], &[]);
        let probes = clipper.probes();
        let read = |n: &str| probes.iter().find(|p| p.name == n).unwrap().latest;
        assert_eq!(read(Probe::NEWTON_SOLVES), 128.0);
        assert!((1.0..=3.0).contains(&read(Probe::NEWTON_MEAN_ITERATIONS)));
        assert_eq!(read(Probe::NEWTON_CAPPED), 0.0);
        assert!(find("filter/peak")
            .map(|m| ModuleInstance::new(m, &config()).probes().is_empty())
            .unwrap());
    }

    #[test]
    fn every_module_passes_validation() {
        for module in CATALOG {
            let mut fresh =
                || Ok(Box::new(ModuleInstance::new(module, &config())) as Box<dyn Instance>);
            let report = Suite::default().run(&mut fresh, |_| {});
            let failed: Vec<_> = report
                .checks
                .iter()
                .filter(|c| c.status == Status::Fail)
                .collect();
            assert!(failed.is_empty(), "{}: {failed:#?}", module.name);
        }
    }
}
