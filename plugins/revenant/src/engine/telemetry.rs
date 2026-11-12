//! Telemetry — SURPRISE (how wrong its last guesses were, relative to
//! what you played: 0 calm … 1 lost) and the live activity of the
//! neurons the face shows. Zero-cost with no bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

use super::brain::Brain;
use crate::{NEURON_TAPS, SURPRISE_TAP};

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    surprise: Tap,
    neurons: [Tap; NEURON_TAPS.len()],
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self {
            bus: None,
            surprise: Tap::new(SURPRISE_TAP),
            neurons: core::array::from_fn(|i| Tap::new(NEURON_TAPS[i])),
        }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    pub(super) fn publish(&self, surprise: f32, brain: &Brain) {
        if let Some(bus) = &self.bus {
            self.surprise.set(bus, f64::from(surprise));
            for (i, tap) in self.neurons.iter().enumerate() {
                tap.set(bus, f64::from(brain.neuron(i)));
            }
        }
    }
}
