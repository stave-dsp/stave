//! NonlinearRc — one-state implicit stage (diode + capacitor),
//! warm-started Newton per tick.
use crate::behavioral::curve::Curve;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::solve::NewtonStats;

/// A clipper's charge time, conduction threshold and knee hardness
/// (higher = more abrupt, more diode-like).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RcClipperConfig {
    pub tau_ms: f32,
    pub threshold: f32,
    pub hardness: f32,
}

/// Circuit arrangement of the one-state stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RcTopology {
    /// Series: cap charged THROUGH the nonlinearity by (x − v);
    /// output = x − v (voltage across the nonlinearity). The 808-style
    /// history-dependent clip.
    Series,
    /// Shunt feedback (Tube-Screamer class): cap driven by the input
    /// current, discharged through the nonlinearity of v itself;
    /// output = x + v. Since the diodes bound |v| ≈ threshold, the
    /// output never drops below the dry signal — the TS "always at
    /// least unity" boost-plus-clip character.
    Shunt { input_gain: f64 },
}

/// `τ·dv/dt = i(x − v)` where `i(·)` is a tabulated conductance curve and
/// `v` is the state (capacitor voltage). Discretised implicitly
/// (backward Euler) and solved per tick by warm-started Newton — the
/// triode load-line trick plus one state. The output is the voltage
/// ACROSS the nonlinearity (`x − v`): the history-dependent clip of the
/// Tube-Screamer/808 family, where the cap charges THROUGH the diode's
/// conduction so the effective threshold moves with what came before.
pub struct NonlinearRc {
    i_curve: Curve,
    tau_ms: f32,
    topology: RcTopology,
    /// k = dt/τ at the prepared rate.
    k: f64,
    /// The prepared rate (0 until `prepare`) — in-place re-voices
    /// ([`set_ts_feedback_clipper`](Self::set_ts_feedback_clipper))
    /// recompute `k` from it without a state-dumping re-prepare.
    sample_rate: f64,
    v: f64,
    stats: NewtonStats,
}

const SERIES_ITERATIONS: u32 = 8;
const CONVERGED_STEP: f64 = 1e-7;

impl NonlinearRc {
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// Generic stage from a conductance shape (odd, monotone ⇒ Newton is
    /// globally stable: F'(v) = 1 + k·i′ ≥ 1).
    pub fn from_conductance(tau_ms: f32, i: impl FnMut(f64) -> f64) -> Self {
        let mut c = Curve::flat();
        let mut i = i;
        c.rebuild(-8.0, 8.0, 0.0, &mut i);
        Self {
            i_curve: c,
            tau_ms,
            topology: RcTopology::Series,
            k: 0.0,
            sample_rate: 0.0,
            v: 0.0,
            stats: NewtonStats::default(),
        }
    }

    /// Tube-Screamer-class stage: op-amp shunt feedback — the cap is
    /// driven by the input current (`input_gain` ≈ the stage's boost) and
    /// discharged through the diode pair, so the clip threshold rides on
    /// history and the output floor is the dry signal.
    pub fn ts_feedback_clipper(config: &RcClipperConfig, input_gain: f32) -> Self {
        let mut rc = Self {
            i_curve: Curve::flat(),
            tau_ms: 1.0,
            topology: RcTopology::Series,
            k: 0.0,
            sample_rate: 0.0,
            v: 0.0,
            stats: NewtonStats::default(),
        };
        rc.set_ts_feedback_clipper(config, input_gain);
        rc
    }

    /// Re-voice the Tube-Screamer stage IN PLACE — block-rate safe, no
    /// allocation, and the cap state `v` is KEPT (a knob move must not
    /// dump the charge history mid-note).
    pub fn set_ts_feedback_clipper(&mut self, config: &RcClipperConfig, input_gain: f32) {
        let RcClipperConfig {
            tau_ms,
            threshold,
            hardness,
        } = *config;
        debug_assert!(
            tau_ms > 0.0 && threshold > 0.0,
            "τ and threshold must be positive"
        );
        debug_assert!(
            hardness >= 2.0 && input_gain >= 0.0,
            "steep conduction needs h ≥ 2"
        );
        let th = threshold as f64;
        let h = hardness as f64;
        // STEEP power-law conduction (diode-like): past ±th the current
        // rises as t^h, so the equilibrium voltage pins near the
        // threshold regardless of drive (v_eq ≈ th·(g·x)^(1/h)) — the
        // series preset's softer curve would let v walk far past it.
        // Tight ±2 domain for table resolution where the state lives.
        self.i_curve.rebuild(-2.0, 2.0, 0.0, move |u: f64| {
            let t = u / th;
            t.signum() * t.abs().powf(h) + 0.05 * u
        });
        self.tau_ms = tau_ms;
        self.topology = RcTopology::Shunt {
            input_gain: input_gain as f64,
        };
        if self.sample_rate > 0.0 {
            self.k = 1.0 / (self.tau_ms as f64 * 1e-3 * self.sample_rate);
        }
    }

    /// Diode-pair preset: conduction turns on past ±`threshold` with the
    /// given knee hardness (higher = more abrupt, more diode-like).
    pub fn diode_clipper(config: &RcClipperConfig) -> Self {
        let mut rc = Self::from_conductance(config.tau_ms, |u| u);
        rc.set_diode_clipper(config);
        rc
    }

    /// Re-voice the diode pair in place: no allocation, cap state kept.
    pub fn set_diode_clipper(&mut self, config: &RcClipperConfig) {
        let RcClipperConfig {
            tau_ms,
            threshold,
            hardness,
        } = *config;
        debug_assert!(
            tau_ms > 0.0 && threshold > 0.0,
            "τ and threshold must be positive"
        );
        debug_assert!(hardness >= 1.0, "hardness below 1 is not a clipper");
        let th = threshold as f64;
        let h = hardness as f64;
        // A small linear leak in parallel (the bleed resistor every real
        // circuit has): without it the cap cannot discharge through a
        // non-conducting diode and the state latches forever.
        const LEAK: f64 = 0.05;
        self.i_curve.rebuild(-8.0, 8.0, 0.0, move |u: f64| {
            let t = u / th;
            t.signum() * (t.abs().powf(h) / (1.0 + t.abs().powf(h - 1.0))) + LEAK * u
        });
        self.tau_ms = tau_ms;
        self.topology = RcTopology::Series;
        if self.sample_rate > 0.0 {
            self.k = 1.0 / (self.tau_ms as f64 * 1e-3 * self.sample_rate);
        }
    }

    #[inline]
    fn i_and_slope(&self, u: f64) -> (f64, f64) {
        // The segment's ANALYTIC derivative, from the same fetch as
        // the value — one table walk instead of three (the finite
        // difference this replaced was 22% of a diode-clipper chain's
        // cost, sampled), and it is the true df/dx of the curve
        // Newton is actually solving. Clamped ≥ 0: the conductance
        // shapes are monotone by contract, and F' = 1 + k·i' ≥ 1
        // keeps Newton globally stable.
        let (i, di) = self.i_curve.f_and_slope_at(u);
        (i, di.max(0.0))
    }
}

impl Stage for NonlinearRc {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = f64::from(sample_rate);
        self.k = 1.0 / (self.tau_ms as f64 * 1e-3 * self.sample_rate);
        self.v = 0.0;
        self.stats = NewtonStats::default();
    }
    fn reset(&mut self) {
        self.v = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let v0 = self.v;
        let mut v = v0; // warm start
        match self.topology {
            // Backward Euler: v = v0 + k·i(x − v); Newton on
            // F(v) = v − v0 − k·i(x − v), F′(v) = 1 + k·i′ ≥ 1.
            RcTopology::Series => {
                let (mut iterations, mut step) = (0, 0.0);
                while iterations < SERIES_ITERATIONS {
                    let (i, di) = self.i_and_slope(x - v);
                    let f = v - v0 - self.k * i;
                    let fp = 1.0 + self.k * di;
                    step = f / fp;
                    v -= step;
                    iterations += 1;
                    if step.abs() < CONVERGED_STEP {
                        break;
                    }
                }
                self.stats
                    .record(iterations, step.abs() < CONVERGED_STEP, step);
                self.v = v;
                (x - v) as f32
            }
            // v = v0 + k·(g·x − i(v)); Newton on
            // F(v) = v − v0 − k·(g·x − i(v)), F′(v) = 1 + k·i′(v) ≥ 1.
            // Semi-implicit: one
            // Newton step from the warm state is the semi-implicit
            // Euler discretization of the cap ODE; k = 1/(tau*fs)
            // ~ 3e-3 makes it unconditionally stable (F' >= 1) and
            // the iterated solve was refining digits the state
            // forgets within its own tau. Measured against the
            // 3-iteration solve on a DI recording: null −68 dB rms /
            // −58 dB peak (a slow bias in the cap state, masked under
            // program); a guarded second step did not improve the
            // null. One iteration is the discretization, not an
            // approximation of a better one.
            RcTopology::Shunt { input_gain } => {
                let (i, di) = self.i_and_slope(v);
                let f = v - v0 - self.k * (input_gain * x - i);
                let fp = 1.0 + self.k * di;
                let step = f / fp;
                v -= step;
                self.stats.record(1, true, step);
                self.v = v;
                (x + v) as f32
            }
        }
    }
    /// The capacitor discharges through the curve's small-signal
    /// conductance: time constant τ / i′(0).
    fn tail(&self) -> Tail {
        let (_, slope) = self.i_and_slope(0.0);
        if slope <= 0.0 {
            return Tail::Infinite;
        }
        Tail::from_time_constant(
            f64::from(self.tau_ms) * 1e-3 / slope,
            self.sample_rate as f32,
        )
    }
}
