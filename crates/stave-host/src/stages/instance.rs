//! A catalog stage running as an [`Instance`], and the handle that
//! writes its settings from any thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::adapt::{StageInstance, StageInstanceConfig};
use crate::config::HostConfig;
use crate::instance::{Instance, Layout, TapReading, Tail};
use crate::param::{Param, ParamError};
use stave_core::params::{AnyParam, Params};
use stave_core::Event;

use stave_dsp::NewtonStats;

use super::{BoxedStage, StageParams, StageSpec};

/// The params shared between whoever writes them and the instance that
/// applies them; `edits` counts writes so the instance knows when.
struct Shared {
    stage: &'static StageSpec,
    params: StageParams,
    edits: AtomicU64,
}

/// Writes a running stage's params from any thread.
#[derive(Clone)]
pub struct CatalogHandle(Arc<Shared>);

impl CatalogHandle {
    /// Set `name` to `value`, in its own units. A value outside the
    /// range, or between a choice's options, is refused.
    pub fn set(&self, name: &str, value: f64) -> Result<(), ParamError> {
        let p = self.param(name)?;
        let options = p.step_count();
        let (min, max) = (p.denormalize(0.0), p.denormalize(1.0));
        let admits = (min..=max).contains(&value) && (options == 0 || value.fract() == 0.0);
        if !admits {
            return Err(if options == 0 {
                ParamError::OutOfRange {
                    name: name.into(),
                    min,
                    max,
                }
            } else {
                ParamError::NotAnOption {
                    name: name.into(),
                    options: (0..options)
                        .filter_map(|i| p.step_name(i))
                        .map(str::to_string)
                        .collect(),
                }
            });
        }
        p.set_value(value);
        self.0.edits.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Set at a normalized position: linear across a continuous range,
    /// the nearest option of a choice.
    pub fn set_normalized(&self, name: &str, normalized: f64) -> Result<(), ParamError> {
        let p = self.param(name)?;
        self.set(name, p.denormalize(normalized))
    }

    /// The value at a normalized position as text: the option's name, or
    /// the number with its unit.
    pub fn display(&self, name: &str, normalized: f64) -> Option<String> {
        let p = self.param(name).ok()?;
        Some(p.format(p.denormalize(normalized)).to_string())
    }

    fn param(&self, name: &str) -> Result<&dyn AnyParam, ParamError> {
        self.0
            .params
            .by_name(name)
            .ok_or_else(|| ParamError::NotFound(name.into()))
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        self.0.params.by_name(name).map(|p| p.value())
    }

    pub fn params(&self) -> Vec<Param> {
        let Shared { stage, params, .. } = &*self.0;
        (0..params.count())
            .filter_map(|i| params.by_index(i))
            .map(|p| Param {
                group: stage.name.into(),
                ..Param::of(p)
            })
            .collect()
    }
}

/// A catalog stage running as an [`Instance`], one stage per channel.
pub struct CatalogInstance {
    shared: Arc<Shared>,
    applied_edits: u64,
    inner: StageInstance<BoxedStage>,
}

impl CatalogInstance {
    pub fn new(stage: &'static StageSpec, config: &HostConfig) -> Self {
        let params = (stage.params)();
        let inner = StageInstance::new(StageInstanceConfig {
            stages: (0..config.channels.get())
                .map(|_| (stage.build)(&params))
                .collect(),
            sample_rate: config.sample_rate,
            max_block: config.max_block,
        });
        let shared = Arc::new(Shared {
            stage,
            params,
            edits: AtomicU64::new(0),
        });
        Self {
            shared,
            applied_edits: 0,
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
        for stage in self.inner.stages_mut() {
            stage.configure(&self.shared.params);
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

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], events: &[Event]) {
        self.apply_edits();
        self.inner.process(inputs, outputs, events);
    }

    fn params(&self) -> Vec<Param> {
        self.handle().params()
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

    fn set_param(&mut self, name: &str, value: f64) -> Result<(), ParamError> {
        self.handle().set(name, value)
    }
}

