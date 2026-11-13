//! A circuit solver's health as telemetry taps (DSP.md N8): how hard
//! each block's solves worked, so a plugin's face, `stave-play` and
//! `stave-bench --describe` show where a circuit struggles — at which
//! knob setting, on which material — instead of a solve that fails
//! silently.
//!
//! A plugin registers one [`SolverTaps`] per circuit when it is built
//! and publishes each block's account from `process`:
//!
//! ```
//! use stave_core::solver_taps::{SolverTapIds, SolverTaps};
//! use stave_core::telemetry::Telemetry;
//! use stave_dsp::circuit::methods::dk::Dk;
//! use stave_dsp::circuit::netlist::Netlist;
//! use stave_dsp::circuit::Circuit;
//! use stave_dsp::Stage;
//!
//! let netlist = Netlist::parse_spice(
//!     "* diode clipper
//!      V1 in 0 0
//!      R1 in out 2.2k
//!      C1 out 0 10n
//!      D1 out 0 DI
//!      D2 0 out DI
//!      .model DI D(Is=2.52n N=1.752)
//!      .control
//!      wrdata out v(out)
//!      .endc",
//! )
//! .unwrap();
//! let mut clipper = Circuit::<Dk>::new(&netlist).unwrap();
//! clipper.drive("V1").unwrap();
//! clipper.prepare(96_000.0);
//!
//! let telemetry = Telemetry::new();
//! let taps = SolverTaps::new(&telemetry, SolverTapIds::DEFAULT); // in Plugin::new
//!
//! // In process: the block, then its account (`take_health` starts the next).
//! let mut block = [0.5f32; 64];
//! clipper.process(&mut block);
//! taps.publish(&clipper.take_health());
//!
//! let ids = SolverTapIds::DEFAULT;
//! assert!(telemetry.value(ids.iterations).unwrap() > 0.0);
//! assert_eq!(telemetry.value(ids.capped), Some(0.0));
//! ```
//!
//! A plugin with more than one circuit names each one's taps with
//! [`solver_tap_ids!`](crate::solver_tap_ids): `solver_tap_ids!("muff")`
//! publishes `muff_iterations`, `muff_capped` and so on.
//!
//! What each tap carries, per published block:
//!
//! | Tap | Value |
//! |---|---|
//! | `iterations` | mean Newton iterations per sample (sweeps, under the scattering iterative method) |
//! | `peak_iterations` | the most one sample took |
//! | `capped` | samples whose solve ended at its cap unconverged — anything but 0 is a defect |
//! | `continuations` | samples whose warm start failed and fell back to continuation |
//! | `residual` | the worst residual a solve ended on, volts (not published for a solver that reports none) |
//!
//! Each tap slot also keeps its minimum, maximum and hit count, so a tool
//! reading [`Telemetry::taps`](crate::telemetry::Telemetry::taps) sees
//! whether `capped` was ever above zero over a whole session.

use crate::telemetry::{Tap, TapId, Telemetry};
use stave_dsp::circuit::methods::dk::Health;

/// The ids one circuit's solver taps publish under.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SolverTapIds {
    pub iterations: TapId,
    pub peak_iterations: TapId,
    pub capped: TapId,
    pub continuations: TapId,
    pub residual: TapId,
}

impl SolverTapIds {
    /// `solver_iterations`, `solver_peak_iterations`, `solver_capped`,
    /// `solver_continuations`, `solver_residual`.
    pub const DEFAULT: Self = crate::solver_tap_ids!("solver");

    /// Every id, in field order.
    pub const fn all(self) -> [TapId; 5] {
        [
            self.iterations,
            self.peak_iterations,
            self.capped,
            self.continuations,
            self.residual,
        ]
    }
}

/// The [`SolverTapIds`] for a circuit called `$name` (a string literal):
/// `$name_iterations`, `$name_peak_iterations`, `$name_capped`,
/// `$name_continuations`, `$name_residual`. A name that makes any id
/// longer than a telemetry slot holds is a compile error in a `const`.
///
/// ```
/// const MUFF: stave_core::solver_taps::SolverTapIds = stave_core::solver_tap_ids!("muff");
/// assert_eq!(MUFF.capped.name(), "muff_capped");
/// ```
#[macro_export]
macro_rules! solver_tap_ids {
    ($name:literal) => {
        $crate::solver_taps::SolverTapIds {
            iterations: $crate::telemetry::TapId::new(concat!($name, "_iterations")),
            peak_iterations: $crate::telemetry::TapId::new(concat!($name, "_peak_iterations")),
            capped: $crate::telemetry::TapId::new(concat!($name, "_capped")),
            continuations: $crate::telemetry::TapId::new(concat!($name, "_continuations")),
            residual: $crate::telemetry::TapId::new(concat!($name, "_residual")),
        }
    };
}

/// One circuit solver's health taps: register when the plugin is built,
/// [`publish`](Self::publish) once a block from `process` (realtime: five
/// tap writes).
pub struct SolverTaps {
    pub iterations: Tap,
    pub peak_iterations: Tap,
    pub capped: Tap,
    pub continuations: Tap,
    pub residual: Tap,
}

impl SolverTaps {
    /// Register `ids` on `telemetry` (off the audio thread).
    pub fn new(telemetry: &Telemetry, ids: SolverTapIds) -> Self {
        Self {
            iterations: Tap::register(telemetry, ids.iterations),
            peak_iterations: Tap::register(telemetry, ids.peak_iterations),
            capped: Tap::register(telemetry, ids.capped),
            continuations: Tap::register(telemetry, ids.continuations),
            residual: Tap::register(telemetry, ids.residual),
        }
    }

    /// Taps bound to nothing (writes dropped).
    pub const fn detached() -> Self {
        Self {
            iterations: Tap::detached(),
            peak_iterations: Tap::detached(),
            capped: Tap::detached(),
            continuations: Tap::detached(),
            residual: Tap::detached(),
        }
    }

    /// Publish one block's account — `Circuit::take_health()` for a
    /// netlist circuit, or `Health::from(stats)` over a hand-derived
    /// topology's Newton counts. A block with no solves publishes
    /// nothing.
    pub fn publish(&self, health: &Health) {
        let newton = &health.newton;
        if newton.solves == 0 {
            return;
        }
        self.iterations.set(newton.mean_iterations());
        self.peak_iterations.set(f64::from(newton.peak_iterations));
        self.capped.set(newton.capped as f64);
        self.continuations.set(health.continuations as f64);
        if !health.max_residual.is_nan() {
            self.residual.set(health.max_residual);
        }
    }
}

impl Default for SolverTaps {
    fn default() -> Self {
        Self::detached()
    }
}
