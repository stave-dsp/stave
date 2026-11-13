//! The wave digital element set (N5): adapted one-ports (the leaves a
//! tree is built from), the two- and three-port adaptors that join them
//! (series, parallel, polarity inverter, ideal transformer), and the
//! unadaptable one-ports that can only sit at a tree's root (ideal
//! sources, short, open, switch).
//!
//! Every element follows one convention (Fettweis; Werner's thesis
//! §1.2): at a port of resistance `R` with voltage `v` and current `j`
//! flowing *into* the element at its `+` terminal, the wave arriving at
//! the element is `α = v + R·j` and the wave it sends back is
//! `β = v − R·j`. [`OnePort::reflected`] is `β`, [`OnePort::incident`]
//! takes `α`. An adapted element's `β` never depends on this sample's
//! `α`, which is what lets a tree compute in one pass up and one down.

use super::OnePort;

/// A resistor: `β = 0` (its port resistance is its own).
#[derive(Clone, Debug)]
pub struct Resistor {
    pub ohms: f64,
}

impl OnePort for Resistor {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        0.0
    }
    fn incident(&mut self, _: f64) {}
}

/// An ideal voltage source behind a series resistance `ohms`:
/// `v = volts + ohms·j`, so `β = volts`.
#[derive(Clone, Debug)]
pub struct ResistiveSource {
    pub ohms: f64,
    pub volts: f64,
}

impl OnePort for ResistiveSource {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        self.volts
    }
    fn incident(&mut self, _: f64) {}
}

/// An ideal current source of `amps`, pushed out of its `+` terminal,
/// shunted by `ohms`: `j = v/ohms − amps`, so `β = ohms·amps`.
#[derive(Clone, Debug)]
pub struct ResistiveCurrentSource {
    pub ohms: f64,
    pub amps: f64,
}

impl OnePort for ResistiveCurrentSource {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        self.ohms * self.amps
    }
    fn incident(&mut self, _: f64) {}
}

/// A reactance discretised by the bilinear transform: its port
/// resistance for a sample rate, and the wave it stored last sample.
#[derive(Clone, Debug)]
struct Reactance {
    value: f64,
    sample_rate: f64,
    ohms: f64,
    state: f64,
}

/// A capacitor by the bilinear transform: `R = T/2C`, `β[n] = α[n−1]`.
#[derive(Clone, Debug)]
pub struct Capacitor(Reactance);

impl Capacitor {
    pub fn new(farads: f64) -> Self {
        let mut c = Self(Reactance {
            value: farads,
            sample_rate: 1.0,
            ohms: 1.0,
            state: 0.0,
        });
        c.set_farads(farads);
        c
    }

    /// Re-derive the port resistance for `sample_rate`; clears state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.0.sample_rate = f64::from(sample_rate);
        self.set_farads(self.0.value);
        self.reset();
    }

    pub const fn farads(&self) -> f64 {
        self.0.value
    }

    /// A value change keeps the stored wave: no allocation, no reset.
    pub fn set_farads(&mut self, farads: f64) {
        self.0.value = farads;
        self.0.ohms = 1.0 / (2.0 * farads * self.0.sample_rate);
    }

    /// Start from `volts` across it with no current (`α = β = v`).
    pub fn set_volts(&mut self, volts: f64) {
        self.0.state = volts;
    }

    pub fn reset(&mut self) {
        self.0.state = 0.0;
    }
}

impl OnePort for Capacitor {
    fn resistance(&self) -> f64 {
        self.0.ohms
    }
    fn reflected(&mut self) -> f64 {
        self.0.state
    }
    fn incident(&mut self, wave: f64) {
        self.0.state = wave;
    }
    fn prepare(&mut self, sample_rate: f32) {
        Capacitor::prepare(self, sample_rate);
    }
}

/// An inductor by the bilinear transform: `R = 2L/T`, `β[n] = −α[n−1]`.
#[derive(Clone, Debug)]
pub struct Inductor(Reactance);

impl Inductor {
    pub fn new(henries: f64) -> Self {
        let mut l = Self(Reactance {
            value: henries,
            sample_rate: 1.0,
            ohms: 1.0,
            state: 0.0,
        });
        l.set_henries(henries);
        l
    }

    /// Re-derive the port resistance for `sample_rate`; clears state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.0.sample_rate = f64::from(sample_rate);
        self.set_henries(self.0.value);
        self.reset();
    }

    pub const fn henries(&self) -> f64 {
        self.0.value
    }

    /// A value change keeps the stored wave: no allocation, no reset.
    pub fn set_henries(&mut self, henries: f64) {
        self.0.value = henries;
        self.0.ohms = 2.0 * henries * self.0.sample_rate;
    }

    /// Start carrying `amps` with no voltage across it (`α = R·j`).
    pub fn set_amps(&mut self, amps: f64) {
        self.0.state = self.0.ohms * amps;
    }

    pub fn reset(&mut self) {
        self.0.state = 0.0;
    }
}

impl OnePort for Inductor {
    fn resistance(&self) -> f64 {
        self.0.ohms
    }
    fn reflected(&mut self) -> f64 {
        -self.0.state
    }
    fn incident(&mut self, wave: f64) {
        self.0.state = wave;
    }
    fn prepare(&mut self, sample_rate: f32) {
        Inductor::prepare(self, sample_rate);
    }
}

/// Two ports sharing one voltage: `G = G₁ + G₂`, `β = Σ (Gₖ/G)·βₖ`,
/// `αₖ = α + β − βₖ`.
#[derive(Clone, Debug)]
pub struct Parallel<A, B> {
    pub left: A,
    pub right: B,
    left_wave: f64,
    right_wave: f64,
    up_wave: f64,
}

impl<A: OnePort, B: OnePort> Parallel<A, B> {
    pub fn new(left: A, right: B) -> Self {
        Self {
            left,
            right,
            left_wave: 0.0,
            right_wave: 0.0,
            up_wave: 0.0,
        }
    }

    fn left_share(&self) -> f64 {
        let (gl, gr) = (1.0 / self.left.resistance(), 1.0 / self.right.resistance());
        gl / (gl + gr)
    }
}

impl<A: OnePort, B: OnePort> OnePort for Parallel<A, B> {
    fn resistance(&self) -> f64 {
        let (rl, rr) = (self.left.resistance(), self.right.resistance());
        rl * rr / (rl + rr)
    }
    fn reflected(&mut self) -> f64 {
        self.left_wave = self.left.reflected();
        self.right_wave = self.right.reflected();
        let share = self.left_share();
        self.up_wave = share * self.left_wave + (1.0 - share) * self.right_wave;
        self.up_wave
    }
    fn incident(&mut self, wave: f64) {
        self.left.incident(wave + self.up_wave - self.left_wave);
        self.right.incident(wave + self.up_wave - self.right_wave);
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.left.prepare(sample_rate);
        self.right.prepare(sample_rate);
    }
}

/// Two ports carrying one current, `left` from the `+` terminal to the
/// middle and `right` from the middle on: `R = R₁ + R₂`, `β = β₁ + β₂`,
/// `αₖ = βₖ + (Rₖ/R)·(α − β)`.
#[derive(Clone, Debug)]
pub struct Series<A, B> {
    pub left: A,
    pub right: B,
    left_wave: f64,
    right_wave: f64,
    up_wave: f64,
}

impl<A: OnePort, B: OnePort> Series<A, B> {
    pub fn new(left: A, right: B) -> Self {
        Self {
            left,
            right,
            left_wave: 0.0,
            right_wave: 0.0,
            up_wave: 0.0,
        }
    }
}

impl<A: OnePort, B: OnePort> OnePort for Series<A, B> {
    fn resistance(&self) -> f64 {
        self.left.resistance() + self.right.resistance()
    }
    fn reflected(&mut self) -> f64 {
        self.left_wave = self.left.reflected();
        self.right_wave = self.right.reflected();
        self.up_wave = self.left_wave + self.right_wave;
        self.up_wave
    }
    fn incident(&mut self, wave: f64) {
        let (rl, rr) = (self.left.resistance(), self.right.resistance());
        let excess = (wave - self.up_wave) / (rl + rr);
        self.left.incident(self.left_wave + rl * excess);
        self.right.incident(self.right_wave + rr * excess);
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.left.prepare(sample_rate);
        self.right.prepare(sample_rate);
    }
}

/// A port turned round (its terminals swapped): `v' = −v`, `j' = −j`, so
/// both waves change sign. Joins a subtree to an adaptor that meets it
/// the other way up.
#[derive(Clone, Debug)]
pub struct Inverter<A> {
    pub inner: A,
}

impl<A: OnePort> OnePort for Inverter<A> {
    fn resistance(&self) -> f64 {
        self.inner.resistance()
    }
    fn reflected(&mut self) -> f64 {
        -self.inner.reflected()
    }
    fn incident(&mut self, wave: f64) {
        self.inner.incident(-wave);
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.inner.prepare(sample_rate);
    }
}

/// An ideal transformer of `turns` (secondary to primary), its primary
/// facing the parent and `secondary` the child: `v₂ = n·v₁` and the
/// secondary's load draws `j₁/n`, so the primary port resistance is
/// `R₂/n²` and both waves scale by `n` across it: `β = β₂/n`,
/// `α₂ = n·α`. Adapted and lossless.
#[derive(Clone, Debug)]
pub struct Transformer<A> {
    pub secondary: A,
    pub turns: f64,
}

impl<A: OnePort> OnePort for Transformer<A> {
    fn resistance(&self) -> f64 {
        self.secondary.resistance() / (self.turns * self.turns)
    }
    fn reflected(&mut self) -> f64 {
        self.secondary.reflected() / self.turns
    }
    fn incident(&mut self, wave: f64) {
        self.secondary.incident(self.turns * wave);
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.secondary.prepare(sample_rate);
    }
}

/// An element whose reflection depends on this sample's arriving wave,
/// so it cannot be adapted: it sits at a tree's root, and the root
/// closes the loop — `reflect(a, R)` takes the wave the tree sends up
/// through a port of resistance `R` and returns the one sent back.
pub trait Root {
    fn reflect(&mut self, wave: f64, ohms: f64) -> f64;
}

/// An ideal voltage source: `v = volts`, so `b = 2·volts − a`.
#[derive(Clone, Debug)]
pub struct VoltageSource {
    pub volts: f64,
}

impl Root for VoltageSource {
    fn reflect(&mut self, wave: f64, _: f64) -> f64 {
        2.0 * self.volts - wave
    }
}

/// An ideal current source pushing `amps` out of its `+` terminal into
/// the tree: `b = a + 2R·amps`.
#[derive(Clone, Debug)]
pub struct CurrentSource {
    pub amps: f64,
}

impl Root for CurrentSource {
    fn reflect(&mut self, wave: f64, ohms: f64) -> f64 {
        wave + 2.0 * ohms * self.amps
    }
}

/// A short circuit: `v = 0`, `b = −a`.
#[derive(Copy, Clone, Debug)]
pub struct Short;

impl Root for Short {
    fn reflect(&mut self, wave: f64, _: f64) -> f64 {
        -wave
    }
}

/// An open circuit: `j = 0`, `b = a` (the tree's Thévenin voltage).
#[derive(Copy, Clone, Debug)]
pub struct Open;

impl Root for Open {
    fn reflect(&mut self, wave: f64, _: f64) -> f64 {
        wave
    }
}

/// An ideal switch: a short when `closed`, else an open. Toggling it
/// re-adapts nothing, so it costs nothing and clicks as a real one does.
#[derive(Copy, Clone, Debug)]
pub struct Switch {
    pub closed: bool,
}

impl Root for Switch {
    fn reflect(&mut self, wave: f64, _: f64) -> f64 {
        if self.closed {
            -wave
        } else {
            wave
        }
    }
}

/// One sample of a tree under `root`: up, reflect, down. Returns the
/// root port's voltage.
pub fn sample<T: OnePort, R: Root>(tree: &mut T, root: &mut R) -> f64 {
    let a = tree.reflected();
    let b = root.reflect(a, tree.resistance());
    tree.incident(b);
    0.5 * (a + b)
}

#[cfg(test)]
mod tests;
