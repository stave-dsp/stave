//! A trained network's sample rate, and what happens when a stage is
//! prepared at another.

/// What a network does when [`prepare`](crate::Stage::prepare)d at a
/// rate other than the one it was trained at.
///
/// A network trained at 48 kHz has learned 48 kHz: at 44.1 kHz every
/// filter it learned sits 8% low and its aliasing is different. So the
/// default refuses: the stage outputs silence and
/// [`rate_mismatch`](super::WaveNet::rate_mismatch) says why. The plugin
/// should run the network at its own rate instead (an
/// [`Oversampled2x`](crate::Oversampled2x) region when the host runs at
/// half the trained rate; arbitrary ratios arrive with DSP.md A2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum RatePolicy {
    /// Output silence at any rate but the trained one (default).
    #[default]
    Strict,
    /// Run at whatever rate the stage is prepared at, knowingly.
    Any,
}

/// A stage prepared at a rate its network was not trained at.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RateMismatch {
    /// Hz the network was trained at.
    pub trained: f32,
    /// Hz the stage was prepared at.
    pub prepared: f32,
}

/// The trained rate, the prepared rate and the policy between them.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RateGuard {
    trained: f32,
    prepared: Option<f32>,
    policy: RatePolicy,
    /// `policy` is strict and the rates differ; cached for `tick`.
    muted: bool,
}

impl RateGuard {
    pub(crate) fn new(trained: f32) -> Self {
        Self {
            trained,
            prepared: None,
            policy: RatePolicy::Strict,
            muted: false,
        }
    }

    pub(crate) fn trained(&self) -> f32 {
        self.trained
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        self.prepared = Some(sample_rate);
        self.update();
    }

    pub(crate) fn set_policy(&mut self, policy: RatePolicy) {
        self.policy = policy;
        self.update();
    }

    fn update(&mut self) {
        self.muted = self.policy == RatePolicy::Strict && self.mismatch().is_some();
    }

    /// The mismatch, if prepared at another rate (within 0.01%).
    pub(crate) fn mismatch(&self) -> Option<RateMismatch> {
        let prepared = self.prepared?;
        let same = (prepared - self.trained).abs() <= 1e-4 * self.trained;
        (!same).then_some(RateMismatch {
            trained: self.trained,
            prepared,
        })
    }

    /// Whether `tick` must output silence.
    #[inline]
    pub(crate) fn muted(&self) -> bool {
        self.muted
    }
}
