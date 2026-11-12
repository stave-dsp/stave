//! **Control** — control is a signal class, not a fixed rate. Signals
//! flow at whatever clock their domain declares (a supply-sag
//! modulation and a band envelope both run per sample); the wiring —
//! which source feeds which inlet by how much — is block-rate data.
//!
//! Three pieces:
//!  - [`Source`]: a [`Stage`] whose current value can be read without
//!    advancing it. Envelope followers, LFOs, dividers and parameter
//!    smoothers implement it — an open trait, never a closed enum at the
//!    kernel boundary.
//!  - [`Route`] / [`Router`]: edges as data. The router owns sources and
//!    wiring; the target type is the consumer's (a chain's stage axes, a
//!    synth's slots). The kernel never names a product inlet.
//!  - The application invariant, owned by every inlet consumer: routes
//!    displace from base — knobs set base values, modulation displaces
//!    from them, so automation and modulation never fight.
//!    [`ModMatrix`](crate::ModMatrix) is the chain-typed instance.

use crate::kernel::node::Stage;

/// A control-signal producer: ticked with a detector sample (program
/// level, a sense tap, the signed signal for a divider), it yields the
/// control value at its own clock; [`state`](Source::state) reads that
/// value back without advancing.
///
/// Two kinds share the one surface: **program-driven** sources follow
/// their detector (envelopes, dividers, env-scaled LFOs), and
/// **writer-driven** sources ignore it (parameter ramps fed targets at
/// block rate, [`SmoothedParam`](crate::SmoothedParam)). A route from a
/// knob and a route from an envelope are therefore the same data; mixing
/// kinds in one [`Router`] is a consumer sum type (the
/// [`Driver`](crate::Driver) pattern), never a second trait.
pub trait Source: Stage {
    /// The current value, without advancing (read side, taps).
    fn state(&self) -> f32;
}

/// One edge: `sources[source]`'s state drives `target` by `amount`.
/// `T` is the consumer's inlet vocabulary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route<T: Copy> {
    pub source: usize,
    pub target: T,
    pub amount: f32,
}

/// A fixed-capacity container is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

impl core::fmt::Display for Full {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("fixed capacity exhausted")
    }
}

impl std::error::Error for Full {}

/// Fixed-capacity control wiring: sources plus edges as data. The
/// router advances sources and exposes states and routes; application
/// belongs to the inlet's owner (displace-from-base). No allocation, no
/// dispatch beyond the source impl.
#[derive(Clone)]
pub struct Router<D: Source, T: Copy, const S: usize, const R: usize> {
    pub sources: [D; S],
    routes: [Option<Route<T>>; R],
    n_routes: usize,
}

impl<D: Source, T: Copy, const S: usize, const R: usize> Router<D, T, S, R> {
    pub fn new(sources: [D; S]) -> Self {
        Self {
            sources,
            routes: [None; R],
            n_routes: 0,
        }
    }

    /// Rebuild wiring (block rate; routes are cheap data).
    pub fn clear_routes(&mut self) {
        self.routes = [None; R];
        self.n_routes = 0;
    }

    /// Add an edge; [`Full`] when all `R` slots are taken.
    pub fn push_route(&mut self, r: Route<T>) -> Result<(), Full> {
        let slot = self.routes.get_mut(self.n_routes).ok_or(Full)?;
        *slot = Some(r);
        self.n_routes += 1;
        Ok(())
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        for s in self.sources.iter_mut() {
            s.prepare(sample_rate);
        }
    }
    pub fn reset(&mut self) {
        for s in self.sources.iter_mut() {
            s.reset();
        }
    }

    /// Advance every source one tick and return their states.
    #[inline]
    pub fn tick_states(&mut self, detectors: &[f32; S]) -> [f32; S] {
        let mut out = [0.0; S];
        for (i, s) in self.sources.iter_mut().enumerate() {
            out[i] = s.tick(detectors[i]);
        }
        out
    }

    /// The live edges, for the inlet owner's apply pass and for
    /// reflection ("what modulates what" is data).
    #[inline]
    pub fn routes(&self) -> impl Iterator<Item = &Route<T>> {
        self.routes[..self.n_routes].iter().flatten()
    }
}

/// Any [`Stage`] viewed as a control source (detector-shaped stages:
/// rectifiers, smoothers built from audio primitives). Its state is the
/// last value it produced.
#[derive(Clone)]
pub struct StageSource<S: Stage> {
    pub stage: S,
    last: f32,
}

impl<S: Stage> StageSource<S> {
    pub fn new(stage: S) -> Self {
        Self { stage, last: 0.0 }
    }
}

impl<S: Stage> Stage for StageSource<S> {
    fn prepare(&mut self, sample_rate: f32) {
        self.stage.prepare(sample_rate);
        self.last = 0.0;
    }
    fn reset(&mut self) {
        self.stage.reset();
        self.last = 0.0;
    }
    #[inline]
    fn tick(&mut self, detector: f32) -> f32 {
        self.last = self.stage.tick(detector);
        self.last
    }
    fn latency(&self) -> u32 {
        self.stage.latency()
    }
    fn tail(&self) -> crate::kernel::node::Tail {
        self.stage.tail()
    }
}

impl<S: Stage> Source for StageSource<S> {
    fn state(&self) -> f32 {
        self.last
    }
}
