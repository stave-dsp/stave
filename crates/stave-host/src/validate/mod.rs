mod checks;
mod logged;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::control::ControlError;
use crate::instance::{Instance, PluginMessage, StateError};
use crate::opened::OpenError;
use crate::note::{Note, NoteSchedule};
use stave_core::diagnostics::Severity;
use stave_measure::signal::{Noise, Signal, Silence};

use logged::Logged;

pub use checks::{
    BypassConfig, ChannelsConfig, DeadlineConfig, GlideConfig, NoiseConfig, OutputConfig,
    SolverConfig, StateConfig, SweepConfig, TailConfig, ALL,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    /// A judgment call, with the numbers to make it.
    Info,
    /// The instance has nothing this check can test.
    NotApplicable,
    Fail,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Outcome {
    pub status: Status,
    pub details: serde_json::Value,
}

impl Outcome {
    pub fn pass(details: serde_json::Value) -> Self {
        Self {
            status: Status::Pass,
            details,
        }
    }
    pub fn info(details: serde_json::Value) -> Self {
        Self {
            status: Status::Info,
            details,
        }
    }
    pub fn not_applicable(reason: &str) -> Self {
        Self {
            status: Status::NotApplicable,
            details: serde_json::json!({ "reason": reason }),
        }
    }
    pub fn fail(details: serde_json::Value) -> Self {
        Self {
            status: Status::Fail,
            details,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CheckResult {
    pub check: &'static str,
    pub status: Status,
    pub duration_ms: f64,
    pub details: serde_json::Value,
    /// What the plugin logged during the check. A contract violation
    /// either way, or a fatal error, fails the check.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugin_messages: Vec<PluginMessage>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SuiteReport {
    pub checks: Vec<CheckResult>,
    pub failures: usize,
}

impl SuiteReport {
    pub fn from_results(checks: Vec<CheckResult>) -> Self {
        let failures = checks.iter().filter(|r| r.status == Status::Fail).count();
        Self { checks, failures }
    }

    pub fn passed(&self) -> bool {
        self.failures == 0
    }
}

pub type Factory<'a> = dyn FnMut() -> Result<Box<dyn Instance>, OpenError> + 'a;

/// Why a check could not run to a verdict.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("could not load a fresh instance")]
    Load(#[from] OpenError),
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    State(#[from] StateError),
}

pub struct Ctx<'a, 'f> {
    factory: &'a mut Factory<'f>,
    pub config: &'a SuiteConfig,
    logged: Rc<RefCell<Vec<PluginMessage>>>,
}

impl Ctx<'_, '_> {
    pub fn fresh(&mut self) -> Result<Box<dyn Instance>, OpenError> {
        Ok(Box::new(Logged {
            inner: (self.factory)()?,
            into: Rc::clone(&self.logged),
        }))
    }
}

/// An error and its sources, outermost first: where a report finally
/// turns a typed error into text.
fn causes(error: &dyn std::error::Error) -> Vec<String> {
    std::iter::successors(Some(error), |e| e.source())
        .map(ToString::to_string)
        .collect()
}

pub struct Check {
    pub name: &'static str,
    pub run: fn(&mut Ctx) -> Result<Outcome, CheckError>,
}

/// Every threshold and duration the checks judge by.
#[derive(Clone, Debug, Default)]
pub struct SuiteConfig {
    pub excitation: Excitation,
    pub output: OutputConfig,
    pub sweep: SweepConfig,
    pub glide: GlideConfig,
    pub noise: NoiseConfig,
    pub channels: ChannelsConfig,
    pub solver: SolverConfig,
    pub tail: TailConfig,
    pub deadline: DeadlineConfig,
    pub state: StateConfig,
    pub bypass: BypassConfig,
}

/// What drives the instance under test: noise for an effect, a held
/// chord for an instrument, since its input has nowhere to go.
#[derive(Clone, Debug)]
pub struct Excitation {
    pub effect_noise: Noise,
    pub instrument_chord: Vec<i16>,
    pub velocity: f32,
}

impl Default for Excitation {
    fn default() -> Self {
        Self {
            effect_noise: Noise {
                seed: 0x0BAD_5EED,
                amplitude: 0.4,
            },
            instrument_chord: vec![60, 64, 67],
            velocity: Note::DEFAULT_VELOCITY,
        }
    }
}

impl Excitation {
    pub fn for_instance(&self, inst: &dyn Instance) -> (Box<dyn Signal>, NoteSchedule) {
        if inst.layout().is_instrument() {
            (
                Box::new(Silence),
                NoteSchedule::held(&self.instrument_chord, self.velocity),
            )
        } else {
            (Box::new(self.effect_noise), NoteSchedule::default())
        }
    }
}

pub struct Suite {
    pub checks: &'static [Check],
    pub config: SuiteConfig,
}

impl Default for Suite {
    fn default() -> Self {
        Self {
            checks: ALL,
            config: SuiteConfig::default(),
        }
    }
}

impl Suite {
    pub fn run(
        &self,
        factory: &mut Factory,
        mut on_result: impl FnMut(&CheckResult),
    ) -> SuiteReport {
        let results = self
            .checks
            .iter()
            .map(|check| {
                let result = self.run_check(factory, check);
                on_result(&result);
                result
            })
            .collect();
        SuiteReport::from_results(results)
    }

    pub fn run_check(&self, factory: &mut Factory, check: &Check) -> CheckResult {
        let started = Instant::now();
        let logged = Rc::default();
        let mut ctx = Ctx {
            factory,
            config: &self.config,
            logged: Rc::clone(&logged),
        };
        let outcome = (check.run)(&mut ctx)
            .unwrap_or_else(|e| Outcome::fail(serde_json::json!({ "error": causes(&e) })));
        let plugin_messages = logged.take();
        let violation = plugin_messages.iter().any(|m| {
            matches!(
                m.severity,
                Severity::Fatal | Severity::HostMisbehaving | Severity::PluginMisbehaving
            )
        });
        CheckResult {
            check: check.name,
            status: if violation {
                Status::Fail
            } else {
                outcome.status
            },
            duration_ms: started.elapsed().as_secs_f64() * 1e3,
            details: outcome.details,
            plugin_messages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Drift, Gain};

    fn report_for(make: fn() -> Box<dyn Instance>) -> SuiteReport {
        let mut f = || Ok(make());
        Suite::default().run(&mut f, |_| {})
    }

    #[test]
    fn a_well_behaved_effect_passes_everything_it_can_be_tested_on() {
        let r = report_for(|| Box::new(Gain::new(2, 2)));
        let bad: Vec<_> = r
            .checks
            .iter()
            .filter(|c| !matches!(c.status, Status::Pass | Status::NotApplicable))
            .collect();
        assert!(bad.is_empty(), "{bad:#?}");
        assert!(r.passed());
    }

    /// A gain stage whose plugin logged one message.
    struct Logging(Gain, Severity);

    impl Instance for Logging {
        fn layout(&self) -> crate::instance::Layout {
            self.0.layout()
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], n: &[crate::note::NoteEvent]) {
            self.0.process(i, o, n)
        }
        fn messages(&self, _from: u64) -> Vec<PluginMessage> {
            vec![PluginMessage {
                index: 0,
                severity: self.1,
                text: "said something".into(),
            }]
        }
    }

    fn first_check(severity: Severity) -> CheckResult {
        let mut f = || Ok(Box::new(Logging(Gain::new(2, 2), severity)) as Box<dyn Instance>);
        Suite::default().run_check(&mut f, &ALL[0])
    }

    #[test]
    fn a_contract_violation_the_plugin_logs_fails_the_check() {
        let r = first_check(Severity::HostMisbehaving);
        assert_eq!(r.status, Status::Fail);
        assert_eq!(r.plugin_messages[0].text, "said something");
    }

    #[test]
    fn an_ordinary_error_the_plugin_logs_is_reported_not_failed() {
        let r = first_check(Severity::Error);
        assert_ne!(r.status, Status::Fail);
        assert!(!r.plugin_messages.is_empty());
    }

    #[test]
    fn load_failure_fails_every_check_with_the_error() {
        let mut f = || {
            Err::<Box<dyn Instance>, _>(OpenError::Plugin {
                path: "/x.vst3".into(),
                source: crate::plugin::HostError::DlopenFailed("no such file".into()),
            })
        };
        let r = Suite::default().run(&mut f, |_| {});
        assert_eq!(r.failures, ALL.len());
        let causes = &r.checks[0].details["error"];
        assert_eq!(causes[0], "could not load a fresh instance");
        assert_eq!(causes[1], "could not open /x.vst3");
        assert_eq!(causes[2], "dlopen failed: no such file");
    }

    #[test]
    fn progress_sees_every_result_in_order() {
        let mut f = || Ok(Box::new(Drift::new()) as Box<dyn Instance>);
        let mut seen = vec![];
        Suite::default().run(&mut f, |r| seen.push(r.check));
        assert_eq!(seen, ALL.iter().map(|c| c.name).collect::<Vec<_>>());
    }
}
