//! A trained network's sample rate, and what happens when a stage is
//! prepared at another.

use crate::primitives::time::interpolators::Lagrange3;

/// What a network does when [`prepare`](crate::Stage::prepare)d at a
/// rate other than the one it was trained at.
///
/// A network trained at 48 kHz has learned 48 kHz: at 44.1 kHz every
/// filter it learned sits 8% low and its aliasing is different. So the
/// default refuses: the stage outputs silence and
/// [`rate_mismatch`](super::WaveNet::rate_mismatch) says why. The plugin
/// should run the network at its own rate instead (an
/// [`Oversampled2x`](crate::Oversampled2x) region when the host runs at
/// half the trained rate, [`Oversampled`](crate::Oversampled) for other
/// whole ratios), or, for a recurrent network, choose
/// [`Interpolated`](Self::Interpolated).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum RatePolicy {
    /// Output silence at any rate but the trained one (default).
    #[default]
    Strict,
    /// Run at whatever rate the stage is prepared at, knowingly: every
    /// time constant the network learned scales with the rate.
    Any,
    /// Recurrent networks ([`Lstm`](super::Lstm), [`Gru`](super::Gru)):
    /// run at any rate from half to sixteen times the trained one by
    /// feeding each step the state one *trained* period back, read from
    /// a short delay line of past states by cubic Lagrange
    /// interpolation — Carson, Wright, Chowdhury, Välimäki & Bilbao,
    /// "Sample rate independent recurrent neural networks for audio
    /// effects processing", DAFx 2024 (their CIDL). The recursion then
    /// keeps the trained network's time constants at the new rate.
    /// Above the trained rate the read interpolates; below it (a 48 kHz
    /// network at 44.1 kHz) it extrapolates a fraction of a sample
    /// ahead of the newest state, which the paper leaves open and
    /// `crates/stave-dsp/tests/learned_rates.rs` measures. Outside half
    /// to sixteen times, and for a [`WaveNet`](super::WaveNet) (its
    /// memory is a learned FIR, not a state), this behaves as
    /// [`Strict`](Self::Strict). Choosing it allocates the delay line:
    /// call [`set_rate_policy`](super::Lstm::set_rate_policy) off the
    /// audio thread.
    Interpolated,
}

/// A stage prepared at a rate its network was not trained at.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RateMismatch {
    /// Hz the network was trained at.
    pub trained: f32,
    /// Hz the stage was prepared at.
    pub prepared: f32,
}

/// Prepared ÷ trained rates [`RatePolicy::Interpolated`] serves: from a
/// 48 kHz network at 24 kHz to the same network at 768 kHz. Below half
/// the extrapolation reaches past half a sample; above sixteen the delay
/// line grows past the capacity allocated when the policy is chosen.
pub(crate) const INTERPOLATED_RATIOS: (f64, f64) = (0.5, 16.0);

/// Delay-line slots the widest ratio needs: the read reaches
/// `γ + 3 = ⌊16⌋ + 2` states back, and the newest write takes one more.
const SLOTS: usize = 16 + 3;

/// The trained rate, the prepared rate and the policy between them.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RateGuard {
    trained: f32,
    prepared: Option<f32>,
    policy: RatePolicy,
    /// Whether the network has a recurrent state to delay.
    recurrent: bool,
    /// The stage outputs silence; cached for `tick`.
    muted: bool,
}

impl RateGuard {
    /// A feed-forward network's guard: [`RatePolicy::Interpolated`] is
    /// strict.
    pub(crate) fn new(trained: f32) -> Self {
        Self {
            trained,
            prepared: None,
            policy: RatePolicy::Strict,
            recurrent: false,
            muted: false,
        }
    }

    /// A recurrent network's guard.
    pub(crate) fn recurrent(trained: f32) -> Self {
        Self {
            recurrent: true,
            ..Self::new(trained)
        }
    }

    pub(crate) fn trained(&self) -> f32 {
        self.trained
    }

    pub(crate) fn policy(&self) -> RatePolicy {
        self.policy
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
        self.muted = match (self.policy, self.mismatch()) {
            (_, None) | (RatePolicy::Any, _) => false,
            (RatePolicy::Strict, Some(_)) => true,
            (RatePolicy::Interpolated, Some(_)) => self.delay().is_none(),
        };
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

    /// The state delay in prepared samples (prepared ÷ trained), when
    /// [`RatePolicy::Interpolated`] applies: a recurrent network, the
    /// rates differ, and the ratio is in [`INTERPOLATED_RATIOS`].
    pub(crate) fn delay(&self) -> Option<f64> {
        let m = self.mismatch()?;
        let ratio = f64::from(m.prepared) / f64::from(m.trained);
        let (low, high) = INTERPOLATED_RATIOS;
        (self.recurrent && self.policy == RatePolicy::Interpolated && (low..=high).contains(&ratio))
            .then_some(ratio)
    }

    /// Whether `tick` must output silence.
    #[inline]
    pub(crate) fn muted(&self) -> bool {
        self.muted
    }
}

/// The past states of a recurrent network, and the read one trained
/// period back (Carson et al. 2024, eqs. 12–14).
///
/// A snapshot is every layer's state laid end to end (`width` floats).
/// At delay `M` (prepared ÷ trained) the state `M` samples back is the
/// cubic through the snapshots `γ … γ + 3` samples back, read at `Δ`
/// past the newest of them:
///
/// ```text
/// γ = max(1, ⌊M⌋ − 1),   Δ = M − γ
/// h̃[n − M] = Σₖ lₖ(Δ) · h̃[n − γ − k],   k = 0 … 3
/// ```
///
/// For `M ≥ 2` that centres the read (`Δ ∈ [1, 2)`); for `1 ≤ M < 2` it
/// is one-sided (`Δ ∈ [0, 1)`), as the paper notes; below 1 it
/// extrapolates (`Δ < 0`). A whole `M` reads one snapshot exactly.
#[derive(Clone, Debug, Default)]
pub(crate) struct StateDelay {
    width: usize,
    /// `SLOTS` snapshots; `newest` is the last one written.
    ring: Vec<f32>,
    newest: usize,
    gamma: usize,
    weights: [f32; 4],
    active: bool,
}

impl StateDelay {
    /// A delay line for snapshots `width` floats wide. Allocates.
    pub(crate) fn new(width: usize) -> Self {
        Self {
            width,
            ring: vec![0.0; SLOTS * width],
            ..Self::default()
        }
    }

    /// Whether the line has been allocated (the policy was chosen).
    pub(crate) fn allocated(&self) -> bool {
        !self.ring.is_empty()
    }

    /// Set the delay (`None`: off, the network steps from its own last
    /// state) and fill every slot with `rest`, the state the network
    /// rests in.
    pub(crate) fn configure(&mut self, delay: Option<f64>, rest: impl Fn(&mut [f32])) {
        self.active = delay.is_some() && self.allocated();
        if let Some(m) = delay.filter(|_| self.active) {
            self.gamma = (m.floor() as usize).saturating_sub(1).max(1);
            let delta = m - self.gamma as f64;
            self.weights = Lagrange3::weights((delta - 1.0) as f32);
        }
        self.fill(rest);
    }

    /// Every slot back to the resting state.
    pub(crate) fn fill(&mut self, rest: impl Fn(&mut [f32])) {
        if self.width > 0 {
            for slot in self.ring.chunks_exact_mut(self.width) {
                rest(slot);
            }
        }
    }

    #[inline]
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    #[inline]
    fn slot(&self, back: usize) -> usize {
        (self.newest + SLOTS - back) % SLOTS
    }

    /// The delayed state's floats `at .. at + out.len()` into `out`.
    #[inline]
    pub(crate) fn read(&self, at: usize, out: &mut [f32]) {
        out.fill(0.0);
        for (k, &l) in self.weights.iter().enumerate() {
            // γ samples back is `γ − 1` slots behind the newest write.
            let s = self.slot(self.gamma - 1 + k) * self.width + at;
            let n = out.len();
            for (o, &v) in out.iter_mut().zip(&self.ring[s..s + n]) {
                *o += l * v;
            }
        }
    }

    /// The new state's floats `at ..` into the next slot (the oldest,
    /// which no read of this step reaches).
    #[inline]
    pub(crate) fn write(&mut self, at: usize, state: &[f32]) {
        let s = ((self.newest + 1) % SLOTS) * self.width + at;
        self.ring[s..s + state.len()].copy_from_slice(state);
    }

    /// Make the slot just written the newest.
    #[inline]
    pub(crate) fn advance(&mut self) {
        self.newest = (self.newest + 1) % SLOTS;
    }
}
