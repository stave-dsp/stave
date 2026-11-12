//! **Control** — kernel form #7 (docs/architecture.md): control
//! is a SIGNAL CLASS, not a fixed rate. Signals flow at whatever
//! clock their domain declares (forge's sag/blocking modulation and
//! multiband's envelopes both run per sample); the WIRING — which
//! source feeds which inlet at what amount — is block-rate data.
//!
//! Three pieces, layered like everything else:
//!  - [`Source`]: the kernel trait a control-signal producer
//!    implements. Envelope followers, LFOs, dividers are CELLS
//!    conforming to it (primitives/control/) — an open trait, never a
//!    closed enum at the kernel boundary (the StageKind lesson).
//!  - [`Route`]/[`Router`]: edges as data. The router owns sources
//!    and wiring; the TARGET type is the consumer's (chain stages
//!    declare typed targets in structures/; hand-rolled paths use
//!    their own). The kernel never names a product inlet.
//!  - The application invariant (owned by every inlet consumer): routes
//!    DISPLACE FROM BASE — knobs set base values, modulation
//!    displaces from them, so automation and modulation never
//!    fight. Ratified from ModMatrix, which becomes an instance.

use crate::kernel::node::Stage;

/// A control-signal producer: fed a detector sample (program level,
/// a sense tap, the signed signal for a divider), yields the control
/// value at its own declared clock.
///
/// The class has exactly TWO kinds (doctrine 3), one surface:
/// **program-driven** sources follow their detector (envelopes,
/// dividers, env-scaled LFOs), and **writer-driven** sources ignore
/// it (param ramps fed targets at block rate — `SmoothedParam` is
/// the kernel-side instance; the host table's ramp is the wrapper-side
/// instance of the same concept). A route from a knob and a route
/// from an envelope are therefore the same data; mixing kinds in one
/// `Router` is a consumer sum type (the `Driver` pattern), never
/// a second trait.
pub trait Source {
    fn prepare(&mut self, rate: f32);
    fn reset(&mut self);
    /// Advance one tick of the source's clock.
    fn tick(&mut self, detector: f32) -> f32;
    /// The current value without advancing (read-side, taps).
    fn state(&self) -> f32;
}

/// One edge: `sources[source]`'s state drives `target` by `amount`.
/// `T` is the consumer's inlet vocabulary — typed, sidecar-emittable.
#[derive(Clone, Copy, Debug)]
pub struct Route<T: Copy> {
    pub source: usize,
    pub target: T,
    pub amount: f32,
}

/// Fixed-capacity control wiring: sources plus edges as data. The
/// router advances sources and exposes states + routes; APPLICATION
/// belongs to the inlet's owner (displace-from-base invariant). No
/// allocation, no dispatch beyond the source impl.
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
    pub fn push_route(&mut self, r: Route<T>) -> bool {
        if self.n_routes < R {
            self.routes[self.n_routes] = Some(r);
            self.n_routes += 1;
            true
        } else {
            false
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        for s in self.sources.iter_mut() {
            s.prepare(rate);
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

    /// The live edges, for the inlet owner's apply pass and for the
    /// sidecar ("what modulates what" is data).
    #[inline]
    pub fn routes(&self) -> impl Iterator<Item = &Route<T>> {
        self.routes[..self.n_routes].iter().flatten()
    }
}

/// A `Stage` viewed as a control source (detector-shaped nodes:
/// rectifiers, smoothers built from audio primitives). The bridge between
/// the audio and control signal classes without a third vocabulary.
pub struct StageSource<S: Stage>(pub S);

impl<S: Stage> Source for StageSource<S> {
    fn prepare(&mut self, rate: f32) {
        self.0.prepare(rate);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    #[inline]
    fn tick(&mut self, detector: f32) -> f32 {
        self.0.tick(detector)
    }
    fn state(&self) -> f32 {
        0.0 // Stage has no read-side; use a Source where taps need it.
    }
}
