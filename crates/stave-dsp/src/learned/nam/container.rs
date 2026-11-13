//! A `SlimmableContainer`: whole submodels of rising cost, one running,
//! chosen by NAM's 0–1 size value.

use super::Network;
use crate::kernel::node::{Stage, Tail};
use crate::learned::rate::{RateMismatch, RatePolicy};

/// Every submodel of a `.nam` `SlimmableContainer`, loaded and prepared
/// together, one of them running: a size knob that switches on the
/// audio thread.
///
/// # Models
///
/// The device the submodels were each trained on, at the cost the
/// chosen one runs at: Atkinson's "Slimmable NAM" (2025) container,
/// whose submodels (any [`WaveNet`](crate::learned::WaveNet) or
/// [`Lstm`](crate::learned::Lstm)) trade accuracy for multiply-adds.
///
/// # Derivation
///
/// The layout and selection rule NeuralAmpModelerCore's container reads
/// (`NAM/container.cpp`, `container.h`, `slimmable.h`; MIT, © Steven
/// Atkinson): `config.submodels[i] = {"max_value", "model"}`, strictly
/// ascending by `max_value`, the last at least 1. A size value `v`
/// selects the first submodel with `v < max_value`, else the last; full
/// size (the last) by default. NAM resets the newly chosen submodel
/// before it runs; so does this, but every submodel is built and
/// prepared up front, so the switch neither allocates nor locks.
///
/// # Verified
///
/// Selection at and around every breakpoint, the default, each refusal
/// and the switch's reset against the standalone submodel
/// (`crates/stave-dsp/src/learned/nam/container/tests.rs::switching_resets_the_chosen_submodel`);
/// each submodel of a trainer-exported container against the trainer's
/// forward pass of it, ≤ 1e-5
/// (`crates/stave-dsp/tests/nam_parity.rs::container_submodels_match_the_trainer`);
/// the switch allocation-free
/// (`crates/stave-dsp/tests/learned_no_alloc.rs::container_switches_without_allocating`).
///
/// # Cost
///
/// The running submodel's ([`multiply_adds`](Self::multiply_adds)),
/// plus one match per sample; see [`WaveNet`](crate::learned::WaveNet)
/// (`"WaveNet (NAM standard)"`) on the datasheet. A switch costs one
/// [`reset`](Stage::reset) of the chosen submodel (a WaveNet settles
/// every layer once). Memory: every submodel stays resident.
///
/// # Aliasing
///
/// The running submodel's: none added beyond what it learned at its
/// trained rate (see [`Network`]).
///
/// # Valid ranges
///
/// Size values: any; below the first `max_value` is the smallest,
/// at or above the last is the full size. The switch is a hard cut, as
/// in NAM: the chosen submodel starts from its reset (silence) state,
/// so the output steps by the difference between the two submodels plus
/// the new one's warm-up over its receptive field. Measured on the
/// trainer-exported fixture, a 330 Hz sine at 0.3: a step of 0.39
/// against 0.0016 for the largest step either submodel takes alone
/// (`crates/stave-dsp/tests/nam_parity.rs::container_switch_is_a_measured_cut`)
/// — a click. That fixture's submodels share no training target, the
/// worst case; a real container's approximate one device, so its step
/// is their disagreement plus the warm-up. No crossfade is built in
/// (it would double the cost through the fade and part from NAM's
/// output): switch between notes, or behind a fade the plugin owns.
/// Sample rate: the submodels' shared trained rate, as [`Network`].
#[derive(Clone, Debug)]
pub struct Container {
    /// Never a container; never empty.
    submodels: Vec<Network>,
    /// Strictly ascending, one per submodel, the last ≥ 1.
    max_values: Vec<f64>,
    active: usize,
}

impl Container {
    /// Built by the loader, which checked the ordering and rates.
    pub(super) fn new(submodels: Vec<Network>, max_values: Vec<f64>) -> Self {
        debug_assert_eq!(submodels.len(), max_values.len());
        let active = submodels.len() - 1;
        Self {
            submodels,
            max_values,
            active,
        }
    }

    /// The submodel a size value selects: the first whose `max_value`
    /// is above `value`, else the last (NAM's rule; a value exactly at a
    /// `max_value` selects the next submodel).
    pub fn size_for(&self, value: f64) -> usize {
        self.max_values
            .iter()
            .position(|&m| value < m)
            .unwrap_or(self.max_values.len() - 1)
    }

    /// Switch to the submodel `value` selects. Real-time safe: no
    /// allocation or lock. A change of submodel resets the chosen one
    /// (its state from silence, as NAM does); the same submodel keeps
    /// running untouched.
    pub fn set_size(&mut self, value: f64) {
        let next = self.size_for(value);
        if next != self.active {
            self.submodels[next].reset();
            self.active = next;
        }
    }

    /// The running submodel's index.
    pub fn active(&self) -> usize {
        self.active
    }

    /// Every submodel, in the file's (ascending `max_value`) order.
    pub fn submodels(&self) -> &[Network] {
        &self.submodels
    }

    /// The `max_value` of each submodel, ascending.
    pub fn max_values(&self) -> &[f64] {
        &self.max_values
    }

    /// The size values at which the selection changes: every
    /// `max_value` but the last.
    pub fn breakpoints(&self) -> &[f64] {
        &self.max_values[..self.max_values.len() - 1]
    }

    /// The submodels' shared trained rate, in Hz.
    pub fn sample_rate(&self) -> f32 {
        self.submodels[0].sample_rate()
    }

    /// The trained and prepared rates, when they differ.
    pub fn rate_mismatch(&self) -> Option<RateMismatch> {
        self.submodels[self.active].rate_mismatch()
    }

    /// The running submodel's multiply-adds per sample.
    pub fn multiply_adds(&self) -> usize {
        self.submodels[self.active].multiply_adds()
    }

    /// Every submodel's rate policy (see [`Network::set_rate_policy`]);
    /// allocates, so call it off the audio thread.
    pub fn set_rate_policy(&mut self, policy: RatePolicy) {
        for n in &mut self.submodels {
            n.set_rate_policy(policy);
        }
    }
}

impl Stage for Container {
    /// Prepares every submodel, so any can be switched to.
    fn prepare(&mut self, sample_rate: f32) {
        for n in &mut self.submodels {
            n.prepare(sample_rate);
        }
    }
    fn reset(&mut self) {
        self.submodels[self.active].reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.submodels[self.active].tick(x)
    }
    fn process(&mut self, block: &mut [f32]) {
        self.submodels[self.active].process(block);
    }
    fn latency(&self) -> u32 {
        0
    }
    /// The longest of any submodel's, so a switch never outlasts it.
    fn tail(&self) -> Tail {
        self.submodels
            .iter()
            .fold(Tail::None, |t, n| t.max(n.tail()))
    }
}

#[cfg(test)]
mod tests;
