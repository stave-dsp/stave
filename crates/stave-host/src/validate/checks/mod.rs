mod bypass;
mod channels;
mod deadline;
mod glide;
mod noise;
mod output;
mod solver;
mod state;
mod sweep;
mod tail;

pub use bypass::BypassConfig;
pub use channels::ChannelsConfig;
pub use deadline::DeadlineConfig;
pub use glide::GlideConfig;
pub use noise::NoiseConfig;
pub use output::OutputConfig;
pub use solver::SolverConfig;
pub use state::StateConfig;
pub use sweep::SweepConfig;
pub use tail::TailConfig;

use super::Check;

pub const ALL: &[Check] = &[
    Check {
        name: "reproducible",
        run: output::reproducible,
    },
    Check {
        name: "no_nan_inf",
        run: output::no_nan_inf,
    },
    Check {
        name: "param_sweep",
        run: sweep::param_sweep,
    },
    Check {
        name: "controls_glide",
        run: glide::controls_glide,
    },
    Check {
        name: "self_noise",
        run: noise::self_noise,
    },
    Check {
        name: "silence_tail",
        run: tail::silence_tail,
    },
    Check {
        name: "deadline_headroom",
        run: deadline::deadline_headroom,
    },
    Check {
        name: "state_roundtrip",
        run: state::state_roundtrip,
    },
    Check {
        name: "solver_converges",
        run: solver::solver_converges,
    },
    Check {
        name: "channels_linked",
        run: channels::channels_linked,
    },
    Check {
        name: "channels_split",
        run: channels::channels_split,
    },
    Check {
        name: "bypass_honest",
        run: bypass::bypass_honest,
    },
];

#[cfg(test)]
pub(crate) fn outcome(
    check: fn(&mut super::Ctx) -> Result<super::Outcome, super::CheckError>,
    make: impl Fn() -> Box<dyn crate::instance::Instance>,
) -> super::Outcome {
    let mut f = || Ok(make());
    let config = super::SuiteConfig::default();
    check(&mut super::Ctx {
        factory: &mut f,
        config: &config,
        logged: Default::default(),
    })
    .unwrap()
}
