//! A catalog stage running as an [`Instance`], and the handle that
//! writes its settings from any thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::adapt::{StageInstance, StageInstanceConfig};
use crate::config::HostConfig;
use crate::control::{Control, ControlError, ControlFlags};
use crate::instance::{Instance, Layout, TapReading, Tail};
use crate::note::NoteEvent;

use stave_dsp::NewtonStats;

use super::{BoxedStage, StageSpec, Range, Setting, SettingValues};

/// Setting values shared between whoever writes them and the instance
/// that applies them; `edits` counts writes so the instance knows when.
struct Shared {
    stage: &'static StageSpec,
    values: Box<[AtomicU64]>,
    edits: AtomicU64,
}

/// Writes a running stage's settings from any thread.
#[derive(Clone)]
pub struct CatalogHandle(Arc<Shared>);

impl CatalogHandle {
    pub fn set(&self, name: &str, plain: f64) -> Result<(), ControlError> {
        let shared = &self.0;
        let i = shared
            .stage
            .settings
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| ControlError::NotFound(name.into()))?;
        let range = shared.stage.settings[i].range;
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
            .stage
            .settings
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| ControlError::NotFound(name.into()))
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        let shared = &self.0;
        let i = shared.stage.settings.iter().position(|s| s.name == name)?;
        Some(f64::from_bits(shared.values[i].load(Ordering::Relaxed)))
    }

    pub fn controls(&self) -> Vec<Control> {
        let stage = self.0.stage;
        stage
            .settings
            .iter()
            .enumerate()
            .map(|(id, s)| Control {
                id: id as u32,
                name: s.name.into(),
                group: stage.name.into(),
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

/// A catalog stage running as an [`Instance`], one stage per channel.
pub struct CatalogInstance {
    shared: Arc<Shared>,
    applied_edits: u64,
    scratch: Vec<f64>,
    inner: StageInstance<BoxedStage>,
}

impl CatalogInstance {
    pub fn new(stage: &'static StageSpec, config: &HostConfig) -> Self {
        let defaults: Vec<f64> = stage.settings.iter().map(|s| s.default).collect();
        let read = SettingValues {
            settings: stage.settings,
            values: &defaults,
        };
        let inner = StageInstance::new(StageInstanceConfig {
            stages: (0..config.channels.get())
                .map(|_| (stage.build)(&read))
                .collect(),
            sample_rate: config.sample_rate,
            max_block: config.max_block,
        });
        let shared = Arc::new(Shared {
            stage,
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
        self.shared.stage.name
    }

    pub fn handle(&self) -> CatalogHandle {
        CatalogHandle(Arc::clone(&self.shared))
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
            settings: self.shared.stage.settings,
            values: &self.scratch,
        };
        for stage in self.inner.stages_mut() {
            stage.configure(&values);
        }
    }
}

impl Instance for CatalogInstance {
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

    fn taps(&self) -> Vec<TapReading> {
        let mut solver: Option<NewtonStats> = None;
        for stats in self.inner.stages().iter().filter_map(|s| s.solver()) {
            solver
                .get_or_insert_with(NewtonStats::default)
                .merge(&stats);
        }
        let Some(s) = solver else { return vec![] };
        let tap = |name: &str, value: f64| TapReading {
            name: name.into(),
            latest: value,
            min: value,
            max: value,
        };
        vec![
            tap(TapReading::NEWTON_SOLVES, s.solves as f64),
            tap(TapReading::NEWTON_MEAN_ITERATIONS, s.mean_iterations()),
            tap(TapReading::NEWTON_CAPPED, s.capped as f64),
            tap(TapReading::NEWTON_MAX_FINAL_STEP, s.max_final_step),
        ]
    }

    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.handle().set(name, plain)
    }
}

