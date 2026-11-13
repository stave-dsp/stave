//! Click-free impulse-response swaps: two convolvers, the incoming one
//! warmed on the live input, then an output crossfade.

use super::{Convolver, Inline, Worker};
use crate::kernel::node::{Stage, Tail};

/// Where a swap stands.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// One convolver sounds.
    Steady,
    /// The incoming convolver hears the input, silently, until its
    /// response is full.
    Warming { left: usize },
    /// The output moves from the outgoing to the incoming convolver.
    Fading { at: usize },
}

/// Swaps a convolver's impulse response without a discontinuity: the
/// incoming [`Convolver`] runs on the live input until its whole
/// response has heard it, then the output crossfades to it.
///
/// For a mono convolver with nothing deferred,
/// [`Convolver::swap_response`] swaps in place at no extra running cost;
/// `Crossfade` works for any convolver (stereo, true stereo, deferred
/// runs on worker threads, a different length or schedule) and blends
/// two responses for as long as both run.
///
/// # Models
/// Filter exchange by output crossfading (Wefers 2015's term for
/// changing a partitioned convolver's filter): during the fade the
/// output is `(1 − g)·(x ∗ h₀) + g·(x ∗ h₁)`, which is `x ∗ ((1 − g)·h₀ +
/// g·h₁)` for a slowly moving `g` — a blend of the two responses, the
/// operation H7's microphone blending needs. `g` is a raised cosine
/// (continuous in value and slope, so the gain change itself puts no
/// edge on the output).
///
/// # Derivation
/// A convolver started on the live input is exact once `len(h₁)` samples
/// have passed (convolution reaches only that far back), so the incoming
/// one warms that long before it is heard: no cold start, no level dip.
/// Both convolvers must have the same latency (block and head).
///
/// # Verified
/// `crates/stave-dsp/tests/convolver_crossfade.rs`: during a swap the
/// output matches the crossfade of the two direct-form convolutions
/// within 1e-5; a coherent tone swapped between two microphone-like
/// responses (different delays and levels) sprays no more energy off its
/// carrier than the steady tone does, where a hard swap sprays tens of
/// dB more (the test prints both), and the swap's largest second
/// difference stays within the steady output's.
///
/// # Cost
/// One convolver, plus the incoming one during a swap (warm-up and
/// fade): twice the cost for `len(h₁) + fade` samples.
///
/// # Aliasing
/// None (linear; the gain moves slower than the signal).
///
/// # Valid ranges
/// Fades from 1 sample (a hard swap) up; any response lengths.
#[derive(Clone, Debug)]
pub struct Crossfade<W = Inline> {
    slots: [Option<Box<Convolver<W>>>; 2],
    /// The slot that sounds (when steady) or is fading out.
    active: usize,
    phase: Phase,
    fade: usize,
}

impl<W: Worker> Crossfade<W> {
    /// `conv` sounding; swaps fade over `fade` samples.
    pub fn new(conv: Convolver<W>, fade: usize) -> Self {
        Self {
            slots: [Some(Box::new(conv)), None],
            active: 0,
            phase: Phase::Steady,
            fade: fade.max(1),
        }
    }

    /// Fade length for the next swap, samples.
    pub fn set_fade(&mut self, fade: usize) {
        self.fade = fade.max(1);
    }

    /// Start a swap to `next` (boxed and prepared off the audio thread;
    /// passing the box allocates nothing): it hears the input from now,
    /// is heard once its whole response has heard it (its tail, latency
    /// plus length), and has taken over `fade` samples after that. The
    /// convolver it displaces (the one before the current) comes back for
    /// the caller to drop off the audio thread.
    ///
    /// # Errors
    ///
    /// `Err(next)` while a swap is in progress or when `next`'s latency
    /// differs from the sounding convolver's.
    #[allow(clippy::type_complexity)]
    pub fn load(
        &mut self,
        mut next: Box<Convolver<W>>,
    ) -> Result<Option<Box<Convolver<W>>>, Box<Convolver<W>>> {
        let sounding = self.slots[self.active].as_deref().map(Stage::latency);
        if self.phase != Phase::Steady || sounding.is_some_and(|l| l != next.latency()) {
            return Err(next);
        }
        next.reset();
        let warm = match next.tail() {
            Tail::Samples(n) => n as usize,
            _ => 0,
        };
        let displaced = self.slots[1 - self.active].replace(next);
        self.phase = Phase::Warming { left: warm };
        Ok(displaced)
    }

    /// Whether a swap is in progress.
    pub fn swapping(&self) -> bool {
        self.phase != Phase::Steady
    }

    /// The sounding convolver (the outgoing one during a swap).
    pub fn current(&self) -> Option<&Convolver<W>> {
        self.slots[self.active].as_deref()
    }

    /// One stereo sample (stereo or true-stereo responses).
    #[inline]
    pub fn tick_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let (a, b) = self.both(|c| c.tick_stereo(left, right));
        let g = self.advance();
        (a.0 + g * (b.0 - a.0), a.1 + g * (b.1 - a.1))
    }

    /// Both convolvers' outputs (the idle one silent when steady).
    #[inline]
    fn both<T: Default>(&mut self, mut f: impl FnMut(&mut Convolver<W>) -> T) -> (T, T) {
        let active = self.slots[self.active]
            .as_deref_mut()
            .map(&mut f)
            .unwrap_or_default();
        let incoming = if self.phase == Phase::Steady {
            T::default()
        } else {
            self.slots[1 - self.active]
                .as_deref_mut()
                .map(&mut f)
                .unwrap_or_default()
        };
        (active, incoming)
    }

    /// Advance the swap one sample; the incoming convolver's gain for
    /// the sample just computed.
    #[inline]
    fn advance(&mut self) -> f32 {
        match self.phase {
            Phase::Steady => 0.0,
            Phase::Warming { left } => {
                self.phase = if left <= 1 {
                    Phase::Fading { at: 0 }
                } else {
                    Phase::Warming { left: left - 1 }
                };
                0.0
            }
            Phase::Fading { at } => {
                let t = (at + 1) as f32 / self.fade as f32;
                if at + 1 >= self.fade {
                    self.active = 1 - self.active;
                    self.phase = Phase::Steady;
                    1.0
                } else {
                    self.phase = Phase::Fading { at: at + 1 };
                    0.5 - 0.5 * (core::f32::consts::PI * t).cos()
                }
            }
        }
    }
}

impl<W: Worker> Stage for Crossfade<W> {
    fn prepare(&mut self, sample_rate: f32) {
        for conv in self.slots.iter_mut().flatten() {
            conv.prepare(sample_rate);
        }
        self.phase = Phase::Steady;
    }

    fn reset(&mut self) {
        for conv in self.slots.iter_mut().flatten() {
            conv.reset();
        }
        self.phase = Phase::Steady;
    }

    /// One sample through mono responses.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (a, b) = self.both(|c| c.tick(x));
        let g = self.advance();
        a + g * (b - a)
    }

    fn latency(&self) -> u32 {
        self.current().map_or(0, Stage::latency)
    }

    fn tail(&self) -> Tail {
        let mut tail = self.current().map_or(Tail::None, Stage::tail);
        if self.swapping() {
            if let Some(next) = &self.slots[1 - self.active] {
                tail = tail.max(next.tail());
            }
        }
        tail
    }
}
