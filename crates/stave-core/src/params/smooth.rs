//! A parameter's declared smoothing. The host/state-visible value is
//! always the unsmoothed target; the glide is render state, owned by the
//! processor ([`ContinuousControl`](super::ContinuousControl)). Time
//! constants are rate-derived (`1 − exp(−1/(τ·fs))`), never bare
//! per-sample coefficients.

/// How a parameter's audio-rate value approaches its target.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Smooth {
    /// No smoothing: the ramp yields the target immediately. For params
    /// where stepping is correct (discrete selectors) or the DSP owns
    /// its own slew.
    Off,
    /// One-pole exponential glide with time constant `ms`. The default
    /// feel for gains and mixes: fast attack on the knob, no zipper.
    Exp { ms: f64 },
    /// Straight-line ramp reaching the target in exactly `ms`. Right
    /// when overshoot-free arrival time matters (delay times, pitch).
    Linear { ms: f64 },
}

impl Smooth {
    /// Default smoothing for continuous params: 15 ms exponential —
    /// inaudible lag, no zipper at GUI/automation rates.
    pub const DEFAULT: Smooth = Smooth::Exp { ms: 15.0 };
}
