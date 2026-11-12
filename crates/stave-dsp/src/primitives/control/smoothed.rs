//! Block-rate parameter smoother. The host writes a target value at any
//! time; the audio thread pulls smoothly toward it sample-by-sample.
//!
//! Two flavours wrapped in one type:
//! - `set_target(v)` + `tick()` per sample = one-pole exponential glide.
//! - `set_target(v)` + `next_block(n)` = linear ramp over `n` samples,
//!   useful when you want to read once per block and apply N times.

/// Default time constant in seconds. ~5 ms = no zipper at GUI rate, no
/// audible lag.
pub const DEFAULT_TC_SECS: f32 = 0.005;

#[derive(Clone, Copy, Debug)]
pub struct SmoothedParam {
    target: f32,
    current: f32,
    /// Coefficient α for the one-pole: cur += α * (target - cur).
    /// Recomputed when sample-rate or time-constant changes.
    alpha: f32,
    sample_rate: f32,
    tc_secs: f32,
}

impl SmoothedParam {
    pub fn new(initial: f32) -> Self {
        let mut s = Self {
            target: initial,
            current: initial,
            alpha: 0.0,
            sample_rate: 48_000.0,
            tc_secs: DEFAULT_TC_SECS,
        };
        s.recompute();
        s
    }

    pub fn prepare(&mut self, sample_rate: f32, tc_secs: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        debug_assert!(tc_secs > 0.0, "time constant must be positive");
        self.sample_rate = sample_rate;
        self.tc_secs = tc_secs;
        self.recompute();
    }

    fn recompute(&mut self) {
        // α = 1 − exp(−1/(tc·fs)); a one-pole can't be faster than one sample.
        let tau = self.tc_secs * self.sample_rate;
        self.alpha = 1.0 - (-1.0 / tau.max(1.0)).exp();
    }

    pub fn set_target(&mut self, v: f32) {
        self.target = v;
    }

    pub fn snap_to(&mut self, v: f32) {
        self.target = v;
        self.current = v;
    }

    /// Per-sample tick. Returns the freshly-smoothed value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.current += self.alpha * (self.target - self.current);
        self.current
    }

    /// Block-rate linear ramp (the second flavour the module docs
    /// promise): advance toward the target by one block's worth of the
    /// time constant and return the NEW value, to be held for the
    /// block. Reaches the target in ~`tc_secs` regardless of block
    /// size; never overshoots.
    pub fn next_block(&mut self, n: usize) -> f32 {
        let steps_total = (self.sample_rate * self.tc_secs()).max(1.0);
        let frac = (n as f32 / steps_total).min(1.0);
        self.current += (self.target - self.current) * frac;
        if (self.current - self.target).abs() < 1e-9 {
            self.current = self.target;
        }
        self.current
    }

    /// Time constant currently in effect (derived back from α).
    fn tc_secs(&self) -> f32 {
        // α = 1 − e^(−1/(τ·fs))  ⇒  τ = −1 / (fs·ln(1 − α))
        let one_minus = (1.0 - self.alpha).max(1e-9);
        -1.0 / (self.sample_rate * one_minus.ln())
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }
}

/// A host parameter IS a control
/// source — the ramp fed targets at block rate joins the same
/// class as detectors and LFOs, so a route from a knob and a route
/// from an envelope are the same data. The distinguishing invariant:
/// `tick` IGNORES its detector — a param stream is driven by its
/// writer (host automation, editor gestures), never by program
/// material. `prepare` retunes the coefficient for the new rate
/// KEEPING the declared time constant; `reset` completes the glide —
/// a param's rest state is its target (zeroing would fabricate a
/// value the writer never set). stave-core's `ContinuousControl` is the
/// table-parameter instance of this same concept; the crates are
/// deliberately independent, so the correspondence is this documented
/// contract, not a trait impl.
impl crate::kernel::control::Source for SmoothedParam {
    fn prepare(&mut self, rate: f32) {
        SmoothedParam::prepare(self, rate, self.tc_secs);
    }
    fn reset(&mut self) {
        self.current = self.target;
    }
    #[inline]
    fn tick(&mut self, _detector: f32) -> f32 {
        SmoothedParam::tick(self)
    }
    fn state(&self) -> f32 {
        self.current
    }
}
