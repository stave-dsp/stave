//! Telemetry — what saga reports out: the live core clock (the dive
//! made visible), the wet tail's level, and the record's SPIN angle
//! (radians, wrapped) that the pedal's label turns by.
//! Zero-cost with no bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    clock: Tap,
    tail: Tap,
    spin: Tap,
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self { bus: None, clock: Tap::new("clock"), tail: Tap::new("tail"), spin: Tap::new("spin") }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    pub(super) fn publish(&self, clock: f32, wet_peak: f32, spin: f64) {
        if let Some(bus) = &self.bus {
            self.spin.set(bus, spin);
            self.clock.set(bus, f64::from(clock));
            // 60 dB window → 0..1 (ears are logarithmic).
            let db = 20.0 * wet_peak.max(1e-6).log10();
            self.tail.set(bus, f64::from((1.0 + db / 60.0).clamp(0.0, 1.0)));
        }
    }
}
