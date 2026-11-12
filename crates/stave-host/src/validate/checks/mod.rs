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

pub use bypass::BypassPolicy;
pub use channels::ChannelsPolicy;
pub use deadline::DeadlinePolicy;
pub use glide::GlidePolicy;
pub use noise::NoisePolicy;
pub use output::OutputPolicy;
pub use solver::SolverPolicy;
pub use state::StatePolicy;
pub use sweep::SweepPolicy;
pub use tail::TailPolicy;

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
    let policy = super::SuitePolicy::default();
    check(&mut super::Ctx {
        factory: &mut f,
        policy: &policy,
        logged: Default::default(),
    })
    .unwrap()
}
